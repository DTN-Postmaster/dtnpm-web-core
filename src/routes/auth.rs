//! Authentication and current-operator identity endpoints (`/api/v1/auth/*`).

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use dtnpm_core::config::{AuthMode, parse_duration_secs};
use dtnpm_core::iam::{Capabilities, ContextInput, UserInput};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{
    AuthenticatedUser, ClientIpAddress, SESSION_COOKIE_NAME, extract_session_cookie,
};
use crate::context::ModuleContext;

/// Response payload for `GET /api/v1/auth/me` and `POST /api/v1/auth/login`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMeResponse {
    pub authenticated: bool,
    pub username: String,
    pub roles: Vec<String>,
    pub auth_mode: String,
    pub capabilities: Capabilities,
}

/// Request body for `POST /api/v1/auth/login`.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

fn build_auth_me(ctx: &ModuleContext, user: &UserInput, client_ip: &str) -> AuthMeResponse {
    let context = ContextInput {
        auth_mode: ctx.config.auth.mode.as_str().to_string(),
        station_callsign: ctx.config.callsign.clone(),
    };
    let capabilities = ctx
        .policy_engine
        .evaluate_capabilities(user, &context, client_ip);
    AuthMeResponse {
        authenticated: user.authenticated,
        username: user.username.clone(),
        roles: user.roles.clone(),
        auth_mode: context.auth_mode,
        capabilities,
    }
}

/// `GET /api/v1/auth/me`: returns identity, effective roles, auth mode, and dynamic UI capabilities.
pub async fn get_me(
    State(ctx): State<ModuleContext>,
    user_ext: Option<Extension<AuthenticatedUser>>,
    client_ip_ext: Option<Extension<ClientIpAddress>>,
) -> Json<AuthMeResponse> {
    let user = match user_ext {
        Some(Extension(AuthenticatedUser(u))) => u,
        None => UserInput::default(),
    };
    let client_ip = match client_ip_ext {
        Some(Extension(ClientIpAddress(ip))) => ip,
        None => "127.0.0.1".to_string(),
    };
    Json(build_auth_me(&ctx, &user, &client_ip))
}

fn is_https_request(ctx: &ModuleContext, headers: &HeaderMap, client_ip: &str) -> bool {
    if ctx.config.tls.enabled {
        return true;
    }
    let Ok(peer_ip) = client_ip.parse::<std::net::IpAddr>() else {
        return false;
    };
    if !crate::auth::is_ip_trusted(peer_ip, &ctx.config.auth.proxy.trusted_proxies) {
        return false;
    }
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("https"))
}

/// `POST /api/v1/auth/login`: verifies local Argon2id credentials and sets an `HttpOnly` session cookie.
pub async fn login(
    State(ctx): State<ModuleContext>,
    headers: HeaderMap,
    client_ip_ext: Option<Extension<ClientIpAddress>>,
    Json(req): Json<LoginRequest>,
) -> Response {
    if ctx.config.auth.mode != AuthMode::Local {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Local login is disabled unless [auth].mode = 'local'" })),
        )
            .into_response();
    }

    let client_ip = match client_ip_ext {
        Some(Extension(ClientIpAddress(ip))) => ip,
        None => "127.0.0.1".to_string(),
    };

    if let Some(retry_after) = ctx.login_rate_limiter.check_blocked(&client_ip) {
        let mut resp = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": format!("Too many failed login attempts. Try again in {retry_after}s.")
            })),
        )
            .into_response();
        if let Ok(val) = HeaderValue::from_str(&retry_after.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, val);
        }
        return resp;
    }

    match ctx
        .internal_storage
        .verify_iam_user_password(&req.username, &req.password)
    {
        Ok(Some(user_rec)) => {
            ctx.login_rate_limiter.record_success(&client_ip);
            let ttl_secs = parse_duration_secs(&ctx.config.auth.session_ttl);
            let user_agent = headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok());

            let session = match ctx.internal_storage.create_iam_session(
                &user_rec.username,
                &user_rec.role,
                ttl_secs,
                user_agent,
                Some(&client_ip),
            ) {
                Ok(s) => s,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": format!("Failed to create session: {e}") })),
                    )
                        .into_response();
                }
            };

            let secure_attr = if is_https_request(&ctx, &headers, &client_ip) {
                "; Secure"
            } else {
                ""
            };
            let cookie_val = format!(
                "{SESSION_COOKIE_NAME}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={ttl_secs}{secure_attr}",
                session.session_id
            );

            let user_input = UserInput {
                authenticated: true,
                username: user_rec.username,
                roles: vec![user_rec.role],
                auth_method: "session".to_string(),
            };
            let payload = build_auth_me(&ctx, &user_input, &client_ip);
            let mut resp = (StatusCode::OK, Json(payload)).into_response();
            if let Ok(hdr_val) = HeaderValue::from_str(&cookie_val) {
                resp.headers_mut().insert(header::SET_COOKIE, hdr_val);
            }
            resp
        }
        Ok(None) => {
            ctx.login_rate_limiter.record_failure(&client_ip);
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Invalid username or password" })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Authentication check error: {e}") })),
        )
            .into_response(),
    }
}

/// `POST /api/v1/auth/logout`: revokes the current SQLite session and clears the session cookie.
pub async fn logout(
    State(ctx): State<ModuleContext>,
    headers: HeaderMap,
    client_ip_ext: Option<Extension<ClientIpAddress>>,
) -> Response {
    if let Some(session_id) = extract_session_cookie(&headers) {
        let _ = ctx.internal_storage.delete_iam_session(&session_id);
    }

    let client_ip = match client_ip_ext {
        Some(Extension(ClientIpAddress(ip))) => ip,
        None => "127.0.0.1".to_string(),
    };
    let secure_attr = if is_https_request(&ctx, &headers, &client_ip) {
        "; Secure"
    } else {
        ""
    };
    let clear_cookie =
        format!("{SESSION_COOKIE_NAME}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{secure_attr}");

    let mut resp = (StatusCode::OK, Json(json!({ "status": "logged_out" }))).into_response();
    if let Ok(hdr_val) = HeaderValue::from_str(&clear_cookie) {
        resp.headers_mut().insert(header::SET_COOKIE, hdr_val);
    }
    resp
}
