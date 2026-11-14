//! Axum authentication, proxy header isolation, rate limiting, and Rego PBAC middleware.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use dtnpm_core::config::AuthMode;
use dtnpm_core::iam::{ContextInput, RegoInput, RequestInput, UserInput};
use dtnpm_core::internal_storage::InternalStorage;
use ipnet::IpNet;
use serde_json::json;
use subtle::ConstantTimeEq;
use tracing::warn;

use crate::context::ModuleContext;

/// Name of the HttpOnly session cookie issued by `POST /api/v1/auth/login`.
pub const SESSION_COOKIE_NAME: &str = "dtnpm_session";

/// Resolved user identity inserted into HTTP request extensions by [`pbac_middleware`].
#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub UserInput);

/// Resolved client IP address inserted into HTTP request extensions by [`pbac_middleware`].
#[derive(Debug, Clone)]
pub struct ClientIpAddress(pub String);

#[derive(Debug, Clone)]
struct AttemptRecord {
    failures: u32,
    window_start: Instant,
    blocked_until: Option<Instant>,
}

const MAX_RATE_LIMIT_ENTRIES: usize = 4096;

/// Per-IP login failure rate limiter with exponential backoff.
pub struct LoginRateLimiter {
    attempts: Mutex<HashMap<String, AttemptRecord>>,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginRateLimiter {
    /// Creates an empty rate limiter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            attempts: Mutex::new(HashMap::new()),
        }
    }

    fn prune_expired(guard: &mut HashMap<String, AttemptRecord>, now: Instant) {
        guard.retain(|_, v| {
            v.blocked_until.is_some_and(|b| b > now)
                || now.duration_since(v.window_start) < Duration::from_secs(300)
        });
    }

    /// Returns `Some(retry_after_secs)` if the client IP is temporarily blocked.
    pub fn check_blocked(&self, ip: &str) -> Option<u64> {
        let now = Instant::now();
        let Ok(mut guard) = self.attempts.lock() else {
            return None;
        };
        Self::prune_expired(&mut guard, now);
        if let Some(rec) = guard.get(ip)
            && let Some(until) = rec.blocked_until
            && until > now
        {
            return Some(until.duration_since(now).as_secs().max(1));
        }
        None
    }

    /// Records a failed login attempt and activates exponential backoff after 5 failures.
    pub fn record_failure(&self, ip: &str) {
        let now = Instant::now();
        let Ok(mut guard) = self.attempts.lock() else {
            return;
        };
        if guard.len() >= MAX_RATE_LIMIT_ENTRIES && !guard.contains_key(ip) {
            Self::prune_expired(&mut guard, now);
            while guard.len() >= MAX_RATE_LIMIT_ENTRIES {
                let oldest_key = guard
                    .iter()
                    .min_by_key(|(_, rec)| rec.window_start)
                    .map(|(k, _)| k.clone());
                if let Some(k) = oldest_key {
                    guard.remove(&k);
                } else {
                    break;
                }
            }
        }
        let entry = guard.entry(ip.to_string()).or_insert(AttemptRecord {
            failures: 0,
            window_start: now,
            blocked_until: None,
        });
        if now.duration_since(entry.window_start) > Duration::from_secs(120) {
            entry.failures = 0;
            entry.window_start = now;
            entry.blocked_until = None;
        }
        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= 5 {
            let exp = (entry.failures - 5).min(5);
            let penalty_secs = (15u64 << exp).min(300);
            entry.blocked_until = Some(now + Duration::from_secs(penalty_secs));
        }
    }

    /// Clears failure counters after a successful login.
    pub fn record_success(&self, ip: &str) {
        if let Ok(mut guard) = self.attempts.lock() {
            guard.remove(ip);
        }
    }
}

pub(crate) fn canonicalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

pub(crate) fn is_ip_trusted(ip: IpAddr, trusted_proxies: &[String]) -> bool {
    let ip = canonicalize_ip(ip);
    for entry in trusted_proxies {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(net) = trimmed.parse::<IpNet>() {
            let canonical_net = match net {
                IpNet::V6(v6) => v6
                    .addr()
                    .to_ipv4_mapped()
                    .map(|v4| IpNet::from(ipnet::Ipv4Net::new_assert(v4, 32)))
                    .unwrap_or(IpNet::V6(v6)),
                v4 => v4,
            };
            if net.contains(&ip) || canonical_net.contains(&ip) {
                return true;
            }
        } else if let Ok(addr) = trimmed.parse::<IpAddr>()
            && canonicalize_ip(addr) == ip
        {
            return true;
        }
    }
    false
}

/// Extracts `dtnpm_session` from HTTP `Cookie` headers.
pub fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let Ok(cookie_str) = value.to_str() else {
            continue;
        };
        for pair in cookie_str.split(';') {
            let trimmed = pair.trim();
            if let Some(val) = trimmed.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")) {
                let tok = val.trim();
                if !tok.is_empty() {
                    return Some(tok.to_string());
                }
            }
        }
    }
    None
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    if let Some(rest) = auth.strip_prefix("Bearer ") {
        let t = rest.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    } else if let Some(rest) = auth.strip_prefix("bearer ") {
        let t = rest.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

fn resolve_local_user(headers: &HeaderMap, ctx: &ModuleContext) -> UserInput {
    if let Some(session_id) = extract_session_cookie(headers)
        && let Ok(Some(session)) = ctx.internal_storage.get_valid_iam_session(&session_id)
    {
        return UserInput {
            authenticated: true,
            username: session.username,
            roles: vec![session.role],
            auth_method: "session".to_string(),
        };
    }

    if let Some(raw_token) = extract_bearer_token(headers) {
        let cand_bytes = raw_token.as_bytes();
        let mut matched_static: Option<(String, String)> = None;
        for entry in &ctx.config.grpc.auth.tokens {
            let entry_bytes = entry.token.as_bytes();
            if entry_bytes.len() == cand_bytes.len() && bool::from(entry_bytes.ct_eq(cand_bytes)) {
                let role = InternalStorage::normalize_iam_role(&entry.role)
                    .unwrap_or("Operator")
                    .to_string();
                matched_static = Some((entry.name.clone(), role));
            }
        }
        if let Some((name, role)) = matched_static {
            return UserInput {
                authenticated: true,
                username: name,
                roles: vec![role],
                auth_method: "token".to_string(),
            };
        }

        if let Ok(Some(tok)) = ctx.internal_storage.verify_iam_api_token(&raw_token) {
            return UserInput {
                authenticated: true,
                username: tok.name,
                roles: vec![tok.role],
                auth_method: "token".to_string(),
            };
        }
    }

    UserInput::default()
}

fn extract_host_only(host_hdr: &str) -> Option<String> {
    let trimmed = host_hdr.trim();
    if trimmed.is_empty() {
        return None;
    }
    format!("http://{trimmed}")
        .parse::<axum::http::Uri>()
        .ok()
        .and_then(|uri| uri.host().map(str::to_ascii_lowercase))
}

fn validate_cookie_csrf(
    headers: &HeaderMap,
    method: &axum::http::Method,
) -> Result<(), &'static str> {
    if !matches!(
        *method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::DELETE
            | axum::http::Method::PATCH
    ) {
        return Ok(());
    }

    if let Some(sfs) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        && sfs.trim().eq_ignore_ascii_case("cross-site")
    {
        return Err("Cross-site state-changing request rejected");
    }

    if let Some(origin_val) = headers.get(header::ORIGIN) {
        let origin_str = origin_val
            .to_str()
            .map_err(|_| "Malformed Origin header on state-changing request")?;
        let origin_host = origin_str
            .trim()
            .parse::<axum::http::Uri>()
            .ok()
            .and_then(|u| u.host().map(str::to_ascii_lowercase))
            .ok_or("Invalid Origin header on state-changing request")?;
        let host_only = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .and_then(extract_host_only)
            .ok_or("Missing or invalid Host header for Origin validation")?;
        if origin_host != host_only {
            return Err("CSRF check failed: Origin host does not match Host");
        }
    }

    Ok(())
}

fn apply_security_headers(mut resp: Response) -> Response {
    let headers = resp.headers_mut();
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert_with(|| header::HeaderValue::from_static("nosniff"));
    headers
        .entry(header::X_FRAME_OPTIONS)
        .or_insert_with(|| header::HeaderValue::from_static("DENY"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert_with(|| header::HeaderValue::from_static("no-referrer"));
    resp
}

/// Axum middleware enforcing authentication, CSRF checks, and OPA Rego PBAC on HTTP requests.
pub async fn pbac_middleware(
    State(ctx): State<ModuleContext>,
    mut req: Request,
    next: Next,
) -> Response {
    let client_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| canonicalize_ip(ci.0.ip()))
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

    let client_ip_str = client_ip.to_string();
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();

    let user = match ctx.config.auth.mode {
        AuthMode::None => UserInput {
            authenticated: true,
            username: "anonymous-admin".to_string(),
            roles: vec!["Admin".to_string()],
            auth_method: "none".to_string(),
        },
        AuthMode::Proxy => {
            let user_hdr_name = ctx.config.auth.proxy.header_user.to_lowercase();
            let groups_hdr_name = ctx.config.auth.proxy.header_groups.to_lowercase();
            let has_identity_header = req.headers().contains_key(user_hdr_name.as_str())
                || req.headers().contains_key(groups_hdr_name.as_str());

            if !is_ip_trusted(client_ip, &ctx.config.auth.proxy.trusted_proxies) {
                if has_identity_header {
                    warn!(
                        peer_ip = %client_ip_str,
                        path = %path,
                        "Rejected proxy identity header from untrusted peer IP"
                    );
                    return apply_security_headers(
                        (
                            StatusCode::FORBIDDEN,
                            Json(json!({ "error": "Untrusted proxy identity header rejected" })),
                        )
                            .into_response(),
                    );
                }
                UserInput::default()
            } else {
                let proxy_user = req
                    .headers()
                    .get(user_hdr_name.as_str())
                    .and_then(|v| v.to_str().ok())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);

                if let Some(username) = proxy_user {
                    let groups_raw = req
                        .headers()
                        .get(groups_hdr_name.as_str())
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    let groups: Vec<&str> = groups_raw
                        .split([',', ';', ' '])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect();

                    let is_admin_user = ctx
                        .config
                        .auth
                        .admin_users
                        .iter()
                        .any(|u| u.eq_ignore_ascii_case(&username));
                    let is_admin_group = groups.iter().any(|g| {
                        ctx.config
                            .auth
                            .admin_groups
                            .iter()
                            .any(|ag| ag.eq_ignore_ascii_case(g))
                    });

                    let roles = if is_admin_user || is_admin_group {
                        vec!["Admin".to_string()]
                    } else {
                        vec!["Operator".to_string()]
                    };

                    UserInput {
                        authenticated: true,
                        username,
                        roles,
                        auth_method: "proxy".to_string(),
                    }
                } else {
                    UserInput::default()
                }
            }
        }
        AuthMode::Local => resolve_local_user(req.headers(), &ctx),
    };

    if user.authenticated
        && user.auth_method == "session"
        && let Err(reason) = validate_cookie_csrf(req.headers(), req.method())
    {
        warn!(
            peer_ip = %client_ip_str,
            path = %path,
            reason = %reason,
            "Rejected potentially forged cookie-authenticated request"
        );
        return apply_security_headers(
            (StatusCode::FORBIDDEN, Json(json!({ "error": reason }))).into_response(),
        );
    }

    let rego_input = RegoInput {
        user: user.clone(),
        request: RequestInput {
            interface: "http".to_string(),
            method: method.clone(),
            path: path.clone(),
            action: format!("http:{method}:{path}"),
            client_ip: client_ip_str.clone(),
        },
        context: ContextInput {
            auth_mode: ctx.config.auth.mode.as_str().to_string(),
            station_callsign: ctx.config.callsign.clone(),
        },
    };

    let allowed = ctx.policy_engine.allow(&rego_input);
    if !allowed {
        if !user.authenticated {
            return apply_security_headers(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Authentication required" })),
                )
                    .into_response(),
            );
        }
        return apply_security_headers(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "Forbidden by security policy" })),
            )
                .into_response(),
        );
    }

    req.extensions_mut().insert(AuthenticatedUser(user));
    req.extensions_mut().insert(ClientIpAddress(client_ip_str));

    apply_security_headers(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::{LoginRateLimiter, MAX_RATE_LIMIT_ENTRIES, validate_cookie_csrf};
    use axum::http::{HeaderMap, HeaderValue, Method, header};

    #[test]
    fn test_validate_cookie_csrf_blocks_cross_site_and_mismatched_origin() {
        let mut cross_site = HeaderMap::new();
        cross_site.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(validate_cookie_csrf(&cross_site, &Method::POST).is_err());
        assert!(validate_cookie_csrf(&cross_site, &Method::GET).is_ok());

        let mut mismatch = HeaderMap::new();
        mismatch.insert(header::HOST, HeaderValue::from_static("localhost:3000"));
        mismatch.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(validate_cookie_csrf(&mismatch, &Method::DELETE).is_err());

        let mut same_origin = HeaderMap::new();
        same_origin.insert(header::HOST, HeaderValue::from_static("localhost:3000"));
        same_origin.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://localhost:3000"),
        );
        assert!(validate_cookie_csrf(&same_origin, &Method::POST).is_ok());
    }

    #[test]
    fn test_login_rate_limiter_respects_max_capacity() {
        let limiter = LoginRateLimiter::default();
        for i in 0..(MAX_RATE_LIMIT_ENTRIES + 50) {
            limiter.record_failure(&format!("ip-{i}"));
        }
        let guard = limiter.attempts.lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.len() <= MAX_RATE_LIMIT_ENTRIES);
    }
}
