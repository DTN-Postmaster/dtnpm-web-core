//! Standalone test server harness backed by [`dtnpm_core::MockBpa`] and in-memory SQLite.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request};
use axum::response::Response;
use dtnpm_core::cla_manager::ClaManager;
use dtnpm_core::config::{HamConfig, WebConfig};
use dtnpm_core::internal_storage::InternalStorage;
use dtnpm_core::log_buffer::LogBuffer;
use dtnpm_core::routing::RouteManager;
use dtnpm_core::security::DirectionalKeyStore;
use dtnpm_core::{MockBpa, ServiceTracker};
use hardy_bpa::bpa::BpaRegistration;
use tempfile::TempDir;
use tower::ServiceExt;

use crate::context::ModuleContext;
use crate::injector::WebBundleInjector;
use crate::module::WebModule;
use crate::server::{create_router, register_modules};

/// Fluent builder to spin up an in-memory web server + [`MockBpa`] for testing system routes
/// and standalone [`WebModule`] crates without a full Hardy node.
pub struct TestServerBuilder {
    config: HamConfig,
    modules: Vec<Arc<dyn WebModule>>,
}

impl Default for TestServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TestServerBuilder {
    /// Creates a new builder configured for `dtn://test/` with in-memory storage.
    #[must_use]
    #[allow(clippy::field_reassign_with_default)]
    pub fn new() -> Self {
        let mut config = HamConfig::default();
        config.node_id = "dtn://test/".to_string();
        config.callsign = Some("TEST".to_string());
        config.storage.in_memory = true;
        config.web = Some(WebConfig {
            address: SocketAddr::from(([127, 0, 0, 1], 0)),
            static_dir: std::path::PathBuf::from("ui/dist"),
        });
        Self {
            config,
            modules: Vec::new(),
        }
    }

    /// Overrides the local station node EID (e.g. `"dtn://f4klo/"`).
    #[must_use]
    pub fn node_id(mut self, node_id: impl Into<String>) -> Self {
        self.config.node_id = node_id.into();
        self
    }

    /// Overrides the station callsign.
    #[must_use]
    pub fn callsign(mut self, callsign: impl Into<String>) -> Self {
        self.config.callsign = Some(callsign.into());
        self
    }

    /// Replaces the full station configuration used by this test server.
    #[must_use]
    pub fn with_config(mut self, config: HamConfig) -> Self {
        self.config = config;
        self
    }

    /// Registers an additional [`WebModule`] to mount and initialize.
    #[must_use]
    pub fn with_module(mut self, module: Arc<dyn WebModule>) -> Self {
        self.modules.push(module);
        self
    }

    /// Registers a collection of [`WebModule`]s.
    #[must_use]
    pub fn with_modules(mut self, modules: impl IntoIterator<Item = Arc<dyn WebModule>>) -> Self {
        self.modules.extend(modules);
        self
    }

    /// Builds the [`ModuleContext`], runs migrations, registers injector & module services
    /// on [`MockBpa`], and constructs the Axum [`Router`].
    pub async fn build(mut self) -> anyhow::Result<TestServer> {
        let temp_dir = TempDir::new()?;
        self.config.normalize();
        if self.config.storage.data_dir.is_none() {
            self.config.storage.data_dir = Some(temp_dir.path().to_path_buf());
        }

        let mock_bpa = MockBpa::new(&self.config.node_id);
        let bpa_arc = Arc::new(mock_bpa.clone());
        let service_tracker = ServiceTracker::new(bpa_arc.clone());

        let injector = WebBundleInjector::new();
        bpa_arc
            .register_cla(
                "web-injector".to_string(),
                Arc::new(injector.clone()),
                None,
                hardy_bpa::cla::ClaInit::default(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to register web injector on MockBpa: {e}"))?;

        let internal_storage =
            InternalStorage::new_in_memory_with_quota(self.config.application.quota.max_storage)?;
        let cla_manager = ClaManager::new();
        let route_manager = Arc::new(RouteManager::new(self.config.routes.clone(), None));
        let log_buffer = LogBuffer::new(250);
        let keystore = Arc::new(DirectionalKeyStore::empty());

        let ctx = ModuleContext::new(
            Arc::new(self.config),
            bpa_arc,
            injector,
            service_tracker,
            internal_storage,
            cla_manager,
            route_manager,
            log_buffer,
            keystore,
        )
        .with_capabilities(&self.modules);

        register_modules(&ctx, &self.modules).await?;

        let router = create_router(ctx.clone(), self.modules);

        Ok(TestServer {
            router,
            ctx,
            mock_bpa,
            _temp_dir: temp_dir,
        })
    }
}

/// Running in-memory test server with convenience request and HTTP binding helpers.
pub struct TestServer {
    pub router: Router,
    pub ctx: ModuleContext,
    pub mock_bpa: MockBpa,
    _temp_dir: TempDir,
}

impl TestServer {
    /// Executes an Axum [`Request`] directly in-memory via `tower::ServiceExt::oneshot`.
    pub async fn oneshot(&self, req: Request<Body>) -> Response {
        self.router
            .clone()
            .oneshot(req)
            .await
            .expect("infallible router response")
    }

    /// Performs an in-memory `GET` request.
    pub async fn get(&self, uri: &str) -> Response {
        self.oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .body(Body::empty())
                .expect("valid GET test request"),
        )
        .await
    }

    /// Performs an in-memory `POST` request with a JSON body.
    pub async fn post_json(&self, uri: &str, body: serde_json::Value) -> Response {
        self.oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("valid POST test request"),
        )
        .await
    }

    /// Binds an ephemeral local TCP port (`127.0.0.1:0`) and serves requests until dropped.
    pub async fn spawn_http(&self) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let app = self.router.clone();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((addr, handle))
    }
}
