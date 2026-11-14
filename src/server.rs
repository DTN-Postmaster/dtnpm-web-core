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
        .route("/auth/me", get(routes::auth::get_me))
        .route("/auth/login", post(routes::auth::login))
        .route("/auth/logout", post(routes::auth::logout))
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
        .route("/bundles/inject", post(routes::bundles::send_bundle))
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

    // Restrict CORS origins to exact localhost / loopback hosts to prevent
    // drive-by cross-origin localhost CSRF from third-party websites or lookalike domains
    // (e.g. `http://localhost.evil.com`).
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
                origin
                    .to_str()
                    .ok()
                    .and_then(|s| s.parse::<axum::http::Uri>().ok())
                    .is_some_and(|uri| {
                        matches!(uri.scheme_str(), Some("http" | "https"))
                            && matches!(uri.host(), Some("localhost" | "127.0.0.1" | "[::1]"))
                    })
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
        .layer(axum::middleware::from_fn_with_state(
            ctx,
            crate::auth::pbac_middleware,
        ))
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024)) // 64 MiB multipart/body limit
        .layer(cors)
}

fn load_tls_acceptor(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> anyhow::Result<tokio_rustls::TlsAcceptor> {
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::{CertificateDer, PrivateKeyDer};

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert_path)
        .map_err(|e| anyhow::anyhow!("Failed to read TLS cert {}: {e}", cert_path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Invalid TLS cert PEM {}: {e}", cert_path.display()))?;
    if certs.is_empty() {
        anyhow::bail!("No certificates found in {}", cert_path.display());
    }

    let key = PrivateKeyDer::from_pem_file(key_path)
        .map_err(|e| anyhow::anyhow!("Failed to read TLS key {}: {e}", key_path.display()))?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls_config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| anyhow::anyhow!("TLS protocol version config error: {e}"))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("TLS certificate/key mismatch: {e}"))?;
    tls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(tls_config)))
}

/// Spawns the Axum web server (HTTP or HTTPS depending on `[tls]`) in the background.
pub async fn start_web_server(
    ctx: ModuleContext,
    modules: Vec<Arc<dyn WebModule>>,
    listen_addr: SocketAddr,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let tls_cfg = ctx.config.tls.clone();
    let app = create_router(ctx, modules);
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;

    if tls_cfg.enabled {
        let cert_path = tls_cfg.cert_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("[tls] enabled = true requires [tls].cert_path to be configured")
        })?;
        let key_path = tls_cfg.key_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("[tls] enabled = true requires [tls].key_path to be configured")
        })?;
        let acceptor = load_tls_acceptor(cert_path, key_path)?;
        info!("Axum HTTPS (TLS) web server listening on https://{listen_addr}");

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        let _ = stream.set_nodelay(true);
                        let acceptor = acceptor.clone();
                        let app_clone = app.clone();
                        tokio::spawn(async move {
                            let tls_stream = match acceptor.accept(stream).await {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::debug!("TLS handshake error from {peer_addr}: {e}");
                                    return;
                                }
                            };
                            let svc = tower::service_fn(
                                move |mut req: axum::http::Request<hyper::body::Incoming>| {
                                    let mut router = app_clone.clone();
                                    req.extensions_mut()
                                        .insert(axum::extract::ConnectInfo(peer_addr));
                                    let req = req.map(axum::body::Body::new);
                                    async move { tower::Service::call(&mut router, req).await }
                                },
                            );
                            let io = hyper_util::rt::TokioIo::new(tls_stream);
                            let builder = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            );
                            let hyper_svc = hyper_util::service::TowerToHyperService::new(svc);
                            if let Err(e) = builder.serve_connection(io, hyper_svc).await {
                                tracing::debug!("HTTPS connection ended ({peer_addr}): {e}");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("TLS accept error: {e}");
                    }
                }
            }
        });
        return Ok(handle);
    }

    info!("Axum web server listening on http://{listen_addr}");
    let handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
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
        assert_eq!(
            mod_resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert_eq!(mod_resp.headers().get("x-frame-options").unwrap(), "DENY");
    }

    #[tokio::test]
    async fn test_cors_origin_rejects_lookalike_domains() {
        use axum::body::Body;
        use axum::http::Request;

        let srv = crate::test_harness::TestServerBuilder::new()
            .build()
            .await
            .unwrap();

        let bad_req = Request::builder()
            .method(Method::GET)
            .uri("/api/capabilities")
            .header(header::ORIGIN, "http://localhost.evil.com")
            .body(Body::empty())
            .unwrap();
        let bad_resp = srv.oneshot(bad_req).await;
        assert!(
            bad_resp
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );

        let good_req = Request::builder()
            .method(Method::GET)
            .uri("/api/capabilities")
            .header(header::ORIGIN, "http://localhost:5173")
            .body(Body::empty())
            .unwrap();
        let good_resp = srv.oneshot(good_req).await;
        assert_eq!(
            good_resp
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "http://localhost:5173"
        );
    }
}
