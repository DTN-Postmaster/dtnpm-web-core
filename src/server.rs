//! Axum web server, capabilities discovery, and [`crate::module::WebModule`] mounting.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, Method, header};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};

use crate::context::ModuleContext;
use crate::module::{ModuleDescriptor, WebModule};
use crate::routes;

/// Backwards-compatible alias for handler state type.
pub type AppState = ModuleContext;

/// Response schema for `GET /api/capabilities` and `GET /api/v1/capabilities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesResponse {
    pub modules: Vec<ModuleDescriptor>,
}

/// Returns active module capabilities installed in this node.
pub async fn get_capabilities(State(state): State<ModuleContext>) -> Json<CapabilitiesResponse> {
    Json(CapabilitiesResponse {
        modules: (*state.capabilities).clone(),
    })
}

/// Executes database migrations and registers BPA services for all active [`WebModule`]s.
pub async fn register_modules(
    ctx: &ModuleContext,
    modules: &[Arc<dyn WebModule>],
) -> anyhow::Result<()> {
    ctx.run_migrations(modules)?;
    for module in modules {
        module.register_bpa(ctx).await?;
    }
    Ok(())
}

/// Builds the Axum router with system API endpoints, dynamic module mounts under
/// `/api/{id}/v1`, capabilities discovery, and SPA static asset fallback.
pub fn create_router(ctx: ModuleContext, modules: Vec<Arc<dyn WebModule>>) -> Router {
    let ctx = if ctx.capabilities.is_empty() && !modules.is_empty() {
        ctx.with_capabilities(&modules)
    } else {
        ctx
    };

    let mut static_dir = ctx
        .config
        .web
        .as_ref()
        .map(|w| w.static_dir.clone())
        .unwrap_or_else(dtnpm_core::config::default_web_static_dir);

    // If configured path does not exist directly, check relative to executable or ui/dist
    if !static_dir.exists()
        && let Ok(exe) = std::env::current_exe()
    {
        let exe_dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
        let candidate = exe_dir.join(&static_dir);
        if candidate.exists() {
            static_dir = candidate;
        } else if let Some(parent) = exe_dir.parent() {
            let candidate_parent = parent.join(&static_dir);
            if candidate_parent.exists() {
                static_dir = candidate_parent;
            } else {
                let alt = exe_dir.join("ui").join("dist");
                if alt.exists() {
                    static_dir = alt;
                } else {
                    let alt_web = exe_dir.join("web").join("dist");
                    if alt_web.exists() {
                        static_dir = alt_web;
                    } else {
                        let alt_parent = parent.join("web").join("dist");
                        if alt_parent.exists() {
                            static_dir = alt_parent;
                        }
                    }
                }
            }
        }
    }

    let index_file = static_dir.join("index.html");

    let system_routes = Router::new()
        .route("/capabilities", get(get_capabilities))
        .route("/config", get(routes::config::get_config))
        .route("/services", get(routes::services::get_services))
        .route("/events", get(routes::services::sse_events))
        .route("/logs", get(routes::services::get_logs))
        .route(
            "/bundles",
            get(routes::bundles::list_bundles)
                .post(routes::bundles::send_bundle)
                .delete(routes::bundles::delete_bundle_handler),
        )
        .route(
            "/bundles/payload",
            get(routes::bundles::get_bundle_payload_handler),
        )
        .route("/clas/connect", post(routes::clas::connect_peer_handler))
        .route("/clas/status", get(routes::clas::get_peer_statuses))
        .route(
            "/routes",
            get(routes::static_routes::list_routes)
                .post(routes::static_routes::add_route)
                .delete(routes::static_routes::delete_route),
        )
        .route(
            "/security/policy",
            get(routes::security::get_policy).put(routes::security::update_policy),
        )
        .route(
            "/security/keys",
            get(routes::security::list_keys).post(routes::security::add_key),
        )
        .route("/security/keys/{id}", delete(routes::security::delete_key));

    if static_dir.exists() {
        info!(
            "serving web frontend from directory {}",
            static_dir.display()
        );
    } else {
        warn!(
            "web static-dir '{}' does not exist; SPA assets will not be served",
            static_dir.display()
        );
    }
    let serve_dir = ServeDir::new(&static_dir).fallback(ServeFile::new(index_file));

    // Restrict CORS origins to localhost / loopback interfaces to prevent
    // drive-by cross-origin localhost CSRF from third-party websites.
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::RANGE, header::AUTHORIZATION])
        .allow_origin(tower_http::cors::AllowOrigin::predicate(
            |origin: &HeaderValue, _| {
                if let Ok(s) = origin.to_str() {
                    s.starts_with("http://localhost")
                        || s.starts_with("http://127.0.0.1")
                        || s.starts_with("http://[::1]")
                } else {
                    false
                }
            },
        ));

    let mut router = Router::new()
        .route("/api/capabilities", get(get_capabilities))
        .nest("/api/v1", system_routes)
        .with_state(ctx.clone());

    for module in &modules {
        let prefix = format!("/api/{}/v1", module.id());
        info!(module = %module.id(), mount = %prefix, "Mounting WebModule routes");
        router = router.nest(&prefix, module.api_routes(&ctx));
    }

    router
        .fallback_service(serve_dir)
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024)) // 64 MiB multipart/body limit
        .layer(cors)
}

/// Spawns the Axum web server in the background.
pub async fn start_web_server(
    ctx: ModuleContext,
    modules: Vec<Arc<dyn WebModule>>,
    listen_addr: SocketAddr,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let app = create_router(ctx, modules);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    info!("Axum web server listening on http://{listen_addr}");

    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Axum web server error: {e}");
        }
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::routing::get;
    use hardy_async::async_trait;

    struct DummyModule;

    #[async_trait]
    impl WebModule for DummyModule {
        fn id(&self) -> &'static str {
            "dummy"
        }

        fn name(&self) -> &'static str {
            "Dummy Module"
        }

        fn api_routes(&self, _ctx: &ModuleContext) -> Router<()> {
            Router::new().route("/ping", get(|| async { "pong" }))
        }
    }

    #[tokio::test]
    async fn test_capabilities_and_module_routing() {
        let srv = crate::test_harness::TestServerBuilder::new()
            .with_module(Arc::new(DummyModule))
            .build()
            .await
            .unwrap();

        let cap_resp = srv.get("/api/capabilities").await;
        assert_eq!(cap_resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(cap_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: CapabilitiesResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.modules.len(), 1);
        assert_eq!(parsed.modules[0].id, "dummy");
        assert_eq!(parsed.modules[0].base_path, "/api/dummy/v1");

        let mod_resp = srv.get("/api/dummy/v1/ping").await;
        assert_eq!(mod_resp.status(), StatusCode::OK);
    }
}
