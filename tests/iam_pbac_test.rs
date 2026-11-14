//! Integration tests covering all 6 DTN-HDY-HAM-IAM authentication & PBAC acceptance scenarios.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use axum::routing::{delete, post};
use dtnpm_core::config::{AuthMode, HamConfig, ProxyAuthConfig};
use dtnpm_web_core::routes::auth::AuthMeResponse;
use dtnpm_web_core::{ModuleContext, TestServerBuilder, WebModule};
use hardy_async::async_trait;
use serde_json::json;

struct DummyApiModule;

#[async_trait]
impl WebModule for DummyApiModule {
    fn id(&self) -> &'static str {
        "chat"
    }

    fn name(&self) -> &'static str {
        "Dummy Chat"
    }

    fn api_routes(&self, _ctx: &ModuleContext) -> Router<()> {
        Router::new()
            .route("/messages", post(|| async { StatusCode::OK }))
            .route("/admin_only", delete(|| async { StatusCode::OK }))
    }
}

#[tokio::test]
async fn test_scenario_1_auth_mode_none_is_open() {
    let mut cfg = HamConfig::default();
    cfg.auth.mode = AuthMode::None;
    let srv = TestServerBuilder::new()
        .with_config(cfg)
        .build()
        .await
        .unwrap();

    let me_resp = srv.get("/api/v1/auth/me").await;
    assert_eq!(me_resp.status(), StatusCode::OK);
    let me_bytes = axum::body::to_bytes(me_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let me: AuthMeResponse = serde_json::from_slice(&me_bytes).unwrap();
    assert!(me.authenticated);
    assert_eq!(me.auth_mode, "none");
    assert_eq!(me.roles, vec!["Admin"]);
    assert!(me.capabilities.can_manage_clas);

    let cfg_resp = srv.get("/api/v1/config").await;
    assert_eq!(cfg_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_scenarios_2_3_proxy_operator_and_admin_roles() {
    let mut cfg = HamConfig {
        callsign: Some("F4KLO".to_string()),
        node_id: "dtn://f4klo/".to_string(),
        ..Default::default()
    };
    cfg.auth.mode = AuthMode::Proxy;
    cfg.auth.admin_users = vec!["f4chief".to_string()];
    cfg.auth.admin_groups = vec!["dtn_admins".to_string(), "admins".to_string()];
    cfg.auth.proxy = ProxyAuthConfig {
        header_user: "Remote-User".to_string(),
        header_groups: "Remote-Groups".to_string(),
        trusted_proxies: vec!["127.0.0.1/32".to_string()],
    };

    let srv = TestServerBuilder::new()
        .with_config(cfg)
        .with_module(Arc::new(DummyApiModule))
        .build()
        .await
        .unwrap();

    // Operator: GET allowed, bundle injection & chat message allowed, admin mutations forbidden
    let me_req = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/auth/me")
        .header("Remote-User", "f4abc")
        .header("Remote-Groups", "users,ham")
        .body(Body::empty())
        .unwrap();
    let me_resp = srv.oneshot(me_req).await;
    assert_eq!(me_resp.status(), StatusCode::OK);
    let me: AuthMeResponse = serde_json::from_slice(
        &axum::body::to_bytes(me_resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(me.authenticated);
    assert_eq!(me.username, "f4abc");
    assert_eq!(me.roles, vec!["Operator"]);
    assert!(!me.capabilities.can_manage_clas);
    assert!(!me.capabilities.can_restart_server);
    assert!(me.capabilities.can_inject_bundles);

    // Operator GET /api/v1/config -> 200 OK
    let op_get = srv
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/config")
                .header("Remote-User", "f4abc")
                .header("Remote-Groups", "users")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(op_get.status(), StatusCode::OK);

    // Operator POST /api/chat/v1/messages -> 200 OK
    let op_chat = srv
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/chat/v1/messages")
                .header("Remote-User", "f4abc")
                .header("Remote-Groups", "users")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(op_chat.status(), StatusCode::OK);

    // Operator POST /api/v1/bundles/inject -> allowed through PBAC (not 401/403)
    let op_inject = srv
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/bundles/inject")
                .header("Remote-User", "f4abc")
                .header("Remote-Groups", "users")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "destination": "dtn://peer/echo",
                        "payload": "CQ TEST"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_ne!(op_inject.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(op_inject.status(), StatusCode::FORBIDDEN);

    // Operator admin mutations MUST return 403 Forbidden
    for (method, uri) in [
        (Method::POST, "/api/v1/clas/connect"),
        (Method::DELETE, "/api/v1/bundles"),
        (Method::POST, "/api/v1/routes"),
        (Method::PUT, "/api/v1/security/policy"),
        (Method::DELETE, "/api/chat/v1/admin_only"),
    ] {
        let resp = srv
            .oneshot(
                Request::builder()
                    .method(method.clone())
                    .uri(uri)
                    .header("Remote-User", "f4abc")
                    .header("Remote-Groups", "users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "Operator must be denied {method} {uri}"
        );
    }

    // Admin via group `dtn_admins`: full access
    let adm_me_resp = srv
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/auth/me")
                .header("Remote-User", "f4xyz")
                .header("Remote-Groups", "users,dtn_admins")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let adm_me: AuthMeResponse = serde_json::from_slice(
        &axum::body::to_bytes(adm_me_resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(adm_me.roles, vec!["Admin"]);
    assert!(adm_me.capabilities.can_manage_clas);

    let adm_del = srv
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/chat/v1/admin_only")
                .header("Remote-User", "f4xyz")
                .header("Remote-Groups", "dtn_admins")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(adm_del.status(), StatusCode::OK);

    // Admin via `admin_users` even without admin group
    let chief_del = srv
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/chat/v1/admin_only")
                .header("Remote-User", "f4chief")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(chief_del.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_scenario_4_proxy_header_spoofing_rejected_from_untrusted_ip() {
    let mut cfg = HamConfig::default();
    cfg.auth.mode = AuthMode::Proxy;
    cfg.auth.proxy.trusted_proxies = vec!["10.99.0.1/32".to_string()];

    let srv = TestServerBuilder::new()
        .with_config(cfg)
        .build()
        .await
        .unwrap();

    // 1. Direct oneshot with ConnectInfo from untrusted IP 192.168.1.99
    let mut spoof_req = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/bundles")
        .header("Remote-User", "f4klo")
        .header("Remote-Groups", "admins")
        .body(Body::empty())
        .unwrap();
    spoof_req
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 99], 54321))));

    let resp = srv.oneshot(spoof_req).await;
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "Identity headers from untrusted IP must immediately yield 403 Forbidden"
    );

    // 2. Real TCP loopback connection (127.0.0.1 is NOT in trusted_proxies = [10.99.0.1/32])
    let (addr, _handle) = srv.spawn_http().await.unwrap();
    let client = reqwest::Client::new();
    let http_resp = client
        .get(format!("http://{addr}/api/v1/config"))
        .header("Remote-User", "f4klo")
        .header("Remote-Groups", "admins")
        .send()
        .await
        .unwrap();
    assert_eq!(http_resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_scenario_5_local_auth_login_cookies_rate_limit_and_bearer_tokens() {
    let mut cfg = HamConfig::default();
    cfg.auth.mode = AuthMode::Local;
    cfg.auth.session_ttl = "1h".to_string();

    let srv = TestServerBuilder::new()
        .with_config(cfg)
        .with_module(Arc::new(DummyApiModule))
        .build()
        .await
        .unwrap();

    srv.ctx
        .internal_storage
        .upsert_iam_user("op1", "OpSecret!2026", "Operator")
        .unwrap();
    let (raw_adm_token, _rec) = srv
        .ctx
        .internal_storage
        .create_iam_api_token("cli-admin", "Admin")
        .unwrap();

    // Unauthenticated API call fails with 401
    let unauth = srv.get("/api/v1/bundles").await;
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    // Unauthenticated /api/v1/auth/me succeeds cleanly so SPA renders login form
    let me_unauth = srv.get("/api/v1/auth/me").await;
    assert_eq!(me_unauth.status(), StatusCode::OK);
    let me_body: AuthMeResponse = serde_json::from_slice(
        &axum::body::to_bytes(me_unauth.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(!me_body.authenticated);
    assert_eq!(me_body.auth_mode, "local");

    // Wrong password triggers 401 then 429 rate-limit from brute-force IP 198.51.100.7
    for attempt in 1..=6 {
        let mut bad_login = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "username": "op1", "password": "wrong" }).to_string(),
            ))
            .unwrap();
        bad_login
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([198, 51, 100, 7], 40000))));
        let resp = srv.oneshot(bad_login).await;
        if attempt < 6 {
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        } else {
            assert_eq!(
                resp.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "Repeated failed logins must trigger 429 rate limiting"
            );
        }
    }

    // Valid login over real HTTP cookie jar (from loopback IP)
    let (addr, _handle) = srv.spawn_http().await.unwrap();
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    let login_resp = client
        .post(format!("http://{addr}/api/v1/auth/login"))
        .json(&json!({ "username": "op1", "password": "OpSecret!2026" }))
        .send()
        .await
        .unwrap();
    assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
    let set_cookie = login_resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.contains("dtnpm_session="));
    assert!(set_cookie.contains("HttpOnly"));

    // Cookie session allows GET /api/v1/bundles and denies DELETE /api/chat/v1/admin_only
    let bundles_resp = client
        .get(format!("http://{addr}/api/v1/bundles"))
        .send()
        .await
        .unwrap();
    assert_eq!(bundles_resp.status(), reqwest::StatusCode::OK);

    let admin_del_resp = client
        .delete(format!("http://{addr}/api/chat/v1/admin_only"))
        .send()
        .await
        .unwrap();
    assert_eq!(admin_del_resp.status(), reqwest::StatusCode::FORBIDDEN);

    // Logout revokes cookie session
    let logout_resp = client
        .post(format!("http://{addr}/api/v1/auth/logout"))
        .send()
        .await
        .unwrap();
    assert_eq!(logout_resp.status(), reqwest::StatusCode::OK);

    let after_logout = client
        .get(format!("http://{addr}/api/v1/bundles"))
        .send()
        .await
        .unwrap();
    assert_eq!(after_logout.status(), reqwest::StatusCode::UNAUTHORIZED);

    // Admin Bearer API token works on admin endpoints
    let bearer_resp = srv
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/chat/v1/admin_only")
                .header(header::AUTHORIZATION, format!("Bearer {raw_adm_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(bearer_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_scenario_6_custom_external_rego_policy_override() {
    let dir = tempfile::tempdir().unwrap();
    let policy_path = dir.path().join("custom.rego");
    std::fs::write(
        &policy_path,
        r#"
package dtn.ham.iam
import future.keywords.in

default allow = false

allow {
    input.user.authenticated == true
    input.user.username == "f4special"
}
"#,
    )
    .unwrap();

    let mut cfg = HamConfig::default();
    cfg.auth.mode = AuthMode::Proxy;
    cfg.auth.policy_path = Some(policy_path);
    cfg.auth.proxy.trusted_proxies = vec!["127.0.0.1/32".to_string()];

    let srv = TestServerBuilder::new()
        .with_config(cfg)
        .with_module(Arc::new(DummyApiModule))
        .build()
        .await
        .unwrap();

    // f4special allowed on DELETE even without Admin role under custom policy
    let allow_resp = srv
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/chat/v1/admin_only")
                .header("Remote-User", "f4special")
                .header("Remote-Groups", "users")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(allow_resp.status(), StatusCode::OK);

    // Any other authenticated user denied on GET under custom policy
    let deny_resp = srv
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/bundles")
                .header("Remote-User", "f4other")
                .header("Remote-Groups", "admins")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(deny_resp.status(), StatusCode::FORBIDDEN);
}
