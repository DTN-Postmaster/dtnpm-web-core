//! Bundle creation and injection endpoint handler.

use axum::Json;
use axum::extract::{FromRequest, Multipart, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use bytes::Bytes;
use dtnpm_core::normalize_eid;
use hardy_bpv7::builder::Builder;
use hardy_bpv7::creation_timestamp::CreationTimestamp;
use hardy_bpv7::eid::Eid;
use serde::Deserialize;
use serde_json::json;

use crate::ModuleContext as AppState;

/// JSON payload structure for sending a bundle.
#[derive(Debug, Deserialize)]
pub struct SendBundleJsonRequest {
    pub destination: String,
    pub source: Option<String>,
    pub lifetime: Option<u64>,
    pub payload: String,
    #[serde(default)]
    pub hop_limit: Option<u8>,
    #[serde(default)]
    pub aprs_path: Option<String>,
    #[serde(default)]
    pub spray_quota: Option<u32>,
    #[serde(default)]
    pub status_reports: bool,
}

const MAX_FFMPEG_INPUT_BYTES: usize = 16 * 1024 * 1024;
const FFMPEG_TIMEOUT_SECS: u64 = 10;
static FFMPEG_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// If the payload is a WebM audio stream, attempts to remux it to Ogg Opus using ffmpeg if available.
/// Bounded by concurrency (2 permits), maximum input size (16 MiB), and a 10-second execution timeout.
pub async fn maybe_remux_webm_to_ogg(payload: Vec<u8>) -> Vec<u8> {
    // Matroska / WebM signature: 0x1A 0x45 0xDF 0xA3
    if payload.len() >= 4
        && &payload[0..4] == b"\x1A\x45\xDF\xA3"
        && payload.len() <= MAX_FFMPEG_INPUT_BYTES
    {
        let Ok(_permit) = FFMPEG_SEMAPHORE.acquire().await else {
            return payload;
        };

        let remux_fut = async {
            use std::process::Stdio;
            use tokio::io::AsyncWriteExt;

            let mut child = match tokio::process::Command::new("ffmpeg")
                .args([
                    "-y", "-v", "error", "-i", "pipe:0", "-c:a", "copy", "-f", "ogg", "pipe:1",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
            {
                Ok(c) => c,
                Err(_) => return None,
            };

            if let Some(mut stdin) = child.stdin.take() {
                let bytes = payload.clone();
                tokio::spawn(async move {
                    let _ = stdin.write_all(&bytes).await;
                    let _ = stdin.shutdown().await;
                });
            }

            match child.wait_with_output().await {
                Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                    Some(output.stdout)
                }
                _ => None,
            }
        };

        match tokio::time::timeout(
            std::time::Duration::from_secs(FFMPEG_TIMEOUT_SECS),
            remux_fut,
        )
        .await
        {
            Ok(Some(remuxed)) => remuxed,
            _ => payload,
        }
    } else {
        payload
    }
}

struct BundleInjectionParams {
    destination_str: String,
    source_str: Option<String>,
    lifetime_secs: Option<u64>,
    hop_limit: Option<u8>,
    aprs_path: Option<String>,
    spray_quota: Option<u32>,
    status_reports: bool,
    payload_bytes: Vec<u8>,
}

async fn build_and_inject_bundle(
    state: &AppState,
    params: BundleInjectionParams,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let BundleInjectionParams {
        destination_str,
        source_str,
        lifetime_secs,
        hop_limit,
        aprs_path,
        spray_quota,
        status_reports,
        payload_bytes,
    } = params;
    let payload_bytes = maybe_remux_webm_to_ogg(payload_bytes).await;
    if destination_str.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Destination EID cannot be empty".to_string(),
        ));
    }

    let norm_dest = normalize_eid(&destination_str);
    let destination_eid: Eid = norm_dest.parse().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid destination EID '{norm_dest}': {e}"),
        )
    })?;

    let source_eid: Eid = if let Some(src) = source_str.as_deref().filter(|s| !s.trim().is_empty())
    {
        let norm_src = normalize_eid(src);
        norm_src.parse().map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid source EID '{norm_src}': {e}"),
            )
        })?
    } else {
        let default_source = if let Some(cs) = &state.config.callsign {
            format!("dtn://{}/files", cs.to_lowercase())
        } else if state.config.node_id.trim().starts_with("ipn:") {
            state.config.node_id.trim().to_string()
        } else {
            let base = state.config.node_id.trim_end_matches('/');
            format!("{base}/files")
        };
        let norm_src = normalize_eid(&default_source);
        norm_src.parse().map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Invalid station files endpoint '{norm_src}': {e}"),
            )
        })?
    };

    let lifetime = std::time::Duration::from_secs(lifetime_secs.unwrap_or(3600));
    let payload_len = payload_bytes.len();
    let is_chat_sender = matches!(
        &source_eid,
        Eid::Dtn { service_name, .. } if service_name.as_ref() == "chat"
    );
    let chat_content = is_chat_sender.then(|| String::from_utf8_lossy(&payload_bytes).to_string());

    let mut builder = Builder::new(source_eid.clone(), destination_eid.clone())
        .with_payload(payload_bytes.into())
        .with_lifetime(lifetime);
    if status_reports {
        builder = builder.with_flags(dtnpm_core::mesh_blocks::status_report_flags());
    }
    let builder = dtnpm_core::mesh_blocks::apply_routing_extensions(
        builder,
        hop_limit,
        aprs_path.as_deref(),
        spray_quota,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let (bundle, binbundle) = builder.build(CreationTimestamp::now()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to build bundle: {e}"),
        )
    })?;

    let dest_str = destination_eid.to_string();
    let (binbundle_vec, auth_status) = if let Some(key) =
        state.keystore.find_outbound_key(&dest_str)
    {
        tracing::info!(
            "Signing bundle with BPSec BIB key pattern '{}'",
            key.pattern
        );
        let signed = dtnpm_core::security::sign_bundle_with_key(&bundle, &binbundle, &key, None)
            .map_err(|e| {
                tracing::error!("BPSec BIB signing failed for destination '{dest_str}': {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to sign bundle with BPSec key: {e}"),
                )
            })?;
        (signed, "valid".to_string())
    } else {
        (binbundle.into_vec(), "unsigned".to_string())
    };

    let bundle_id = bundle.primary.id.to_key();
    let bundle_bytes = Bytes::from(binbundle_vec);

    tracing::info!(
        "Dispatching bundle {} -> {} (id: {}, size: {} bytes, lifetime: {}s)",
        source_eid,
        destination_eid,
        bundle_id,
        payload_len,
        lifetime.as_secs()
    );

    state.injector.inject(bundle_bytes).await.map_err(|e| {
        tracing::error!("Failed to inject bundle {}: {e}", bundle_id);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to dispatch bundle to BPA: {e}"),
        )
    })?;

    tracing::info!("Bundle {} injected successfully into BPA", bundle_id);

    if let Some(content) = chat_content {
        let dt = bundle
            .primary
            .id
            .timestamp
            .as_datetime()
            .unwrap_or_else(time::OffsetDateTime::now_utc);
        let created_at = dt
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| dt.to_string());
        let timestamp = i64::try_from(dt.unix_timestamp_nanos() / 1_000_000).unwrap_or(0);
        let clamped_secs = bundle.primary.lifetime.as_secs().min(365 * 86_400);
        let expiry_timestamp = timestamp.saturating_add(
            i64::try_from(clamped_secs)
                .unwrap_or(i64::MAX)
                .saturating_mul(1000),
        );
        let conversation_peer = dtnpm_core::internal_storage::normalize_chat_target(&dest_str);

        let message = dtnpm_core::internal_storage::ChatMessage {
            id: bundle_id.clone(),
            conversation_peer,
            sender_eid: source_eid.to_string(),
            recipient_eid: dest_str,
            content,
            direction: "outbound".to_string(),
            status: "sent".to_string(),
            is_read: true,
            is_sticky: false,
            timestamp,
            expiry_timestamp,
            created_at,
            auth_status,
            delivery_status: None,
            delivery_reason: None,
            delivery_updated_at: None,
        };
        if let Err(err) = state.internal_storage.insert_message(&message) {
            tracing::warn!("Failed to store generic-sender outbound chat message: {err}");
        } else {
            state.service_tracker.broadcast_chat_message(message);
        }
    }

    // Allow BPA async dispatch loop to write the bundle to storage before notifying clients
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    state.service_tracker.broadcast_bundles_update();

    Ok(json!({
        "status": "sent",
        "bundle_id": bundle_id,
        "source": source_eid.to_string(),
        "destination": destination_eid.to_string(),
        "payload_size": payload_len,
        "lifetime_secs": lifetime.as_secs(),
    }))
}

/// Handler for `POST /api/v1/bundles` supporting both JSON and multipart/form-data.
pub async fn send_bundle(State(state): State<AppState>, request: Request) -> impl IntoResponse {
    let content_type = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let params = if content_type.starts_with("multipart/form-data") {
        let mut multipart = match Multipart::from_request(request, &state).await {
            Ok(m) => m,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Invalid multipart form data: {e}") })),
                )
                    .into_response();
            }
        };

        let mut destination: Option<String> = None;
        let mut source: Option<String> = None;
        let mut lifetime: Option<u64> = None;
        let mut hop_limit: Option<u8> = None;
        let mut aprs_path: Option<String> = None;
        let mut spray_quota: Option<u32> = None;
        let mut status_reports = false;
        let mut payload_bytes: Option<Vec<u8>> = None;

        loop {
            let field = match multipart.next_field().await {
                Ok(Some(f)) => f,
                Ok(None) => break,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("Multipart read error: {e}") })),
                    )
                        .into_response();
                }
            };
            let name = field.name().unwrap_or("").to_string();
            if matches!(name.as_str(), "payload" | "file") {
                match field.bytes().await {
                    Ok(bytes) => payload_bytes = Some(bytes.to_vec()),
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Failed to read payload bytes: {e}") })),
                        )
                            .into_response();
                    }
                }
                continue;
            }

            let text = match field.text().await {
                Ok(t) => t,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("Invalid field '{name}': {e}") })),
                    )
                        .into_response();
                }
            };
            let trimmed = text.trim();
            match name.as_str() {
                "destination" => destination = Some(text),
                "source" => source = Some(text),
                "lifetime" if !trimmed.is_empty() => match trimmed.parse::<u64>() {
                    Ok(v) => lifetime = Some(v),
                    Err(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Invalid lifetime '{trimmed}'") })),
                        )
                            .into_response();
                    }
                },
                "hop_limit" if !trimmed.is_empty() => match trimmed.parse::<u8>() {
                    Ok(v) => hop_limit = Some(v),
                    Err(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Invalid hop_limit '{trimmed}' (expected 1..=255)") })),
                        )
                            .into_response();
                    }
                },
                "aprs_path" if !trimmed.is_empty() => aprs_path = Some(trimmed.to_string()),
                "spray_quota" if !trimmed.is_empty() => match trimmed.parse::<u32>() {
                    Ok(v) => spray_quota = Some(v),
                    Err(_) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Invalid spray_quota '{trimmed}'") })),
                        )
                            .into_response();
                    }
                },
                "status_reports" => {
                    status_reports = matches!(
                        trimmed.to_ascii_lowercase().as_str(),
                        "true" | "1" | "yes" | "on"
                    );
                }
                _ => {}
            }
        }

        BundleInjectionParams {
            destination_str: destination.unwrap_or_default(),
            source_str: source,
            lifetime_secs: lifetime,
            hop_limit,
            aprs_path,
            spray_quota,
            status_reports,
            payload_bytes: payload_bytes.unwrap_or_default(),
        }
    } else {
        let Json(req) = match Json::<SendBundleJsonRequest>::from_request(request, &state).await {
            Ok(j) => j,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Invalid JSON request: {e}") })),
                )
                    .into_response();
            }
        };

        BundleInjectionParams {
            destination_str: req.destination,
            source_str: req.source,
            lifetime_secs: req.lifetime,
            hop_limit: req.hop_limit,
            aprs_path: req.aprs_path,
            spray_quota: req.spray_quota,
            status_reports: req.status_reports,
            payload_bytes: req.payload.into_bytes(),
        }
    };

    match build_and_inject_bundle(&state, params).await {
        Ok(res) => (StatusCode::CREATED, Json(res)).into_response(),
        Err((code, err)) => (code, Json(json!({ "error": err }))).into_response(),
    }
}

use axum::extract::Query;
use axum::http::HeaderMap;
use axum::response::Response;

/// Query parameters for bundle operations.
#[derive(Debug, Deserialize)]
pub struct BundleQuery {
    pub id: Option<String>,
    pub download: Option<bool>,
}

/// Handler for `GET /api/v1/bundles` - Lists stored bundles, or a single bundle detail if ?id=... is given.
pub async fn list_bundles(
    State(state): State<AppState>,
    Query(query): Query<BundleQuery>,
) -> Response {
    let db_path = state.config.storage.effective_db_path();
    let payload_dir = state.config.storage.effective_payload_dir();

    if let Some(id) = query.id {
        match dtnpm_core::bundle_inspector::get_bundle_detail(
            &db_path,
            &payload_dir,
            &id,
            Some(&state.keystore),
        )
        .await
        {
            Ok(Some(detail)) => Json(json!(detail)).into_response(),
            Ok(None) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Bundle not found" })),
            )
                .into_response(),
            Err(e) => {
                tracing::error!("get_bundle_detail error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Failed to inspect bundle" })),
                )
                    .into_response()
            }
        }
    } else {
        let keystore = state.keystore.clone();
        match tokio::task::spawn_blocking(move || {
            dtnpm_core::bundle_inspector::list_bundles_from_db(
                &db_path,
                Some(&payload_dir),
                Some(&keystore),
            )
        })
        .await
        {
            Ok(Ok(bundles)) => Json(json!(bundles)).into_response(),
            Ok(Err(e)) => {
                tracing::error!("list_bundles_from_db error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Failed to list bundles" })),
                )
                    .into_response()
            }
            Err(e) => {
                tracing::error!("list_bundles spawn_blocking join error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Internal server error" })),
                )
                    .into_response()
            }
        }
    }
}

/// Handler for `GET /api/v1/bundles/payload` - Downloads or streams the raw bundle payload.
pub async fn get_bundle_payload_handler(
    State(state): State<AppState>,
    Query(query): Query<BundleQuery>,
    req_headers: HeaderMap,
) -> Response {
    let Some(id) = query.id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Missing 'id' query parameter" })),
        )
            .into_response();
    };

    let db_path = state.config.storage.effective_db_path();
    let payload_dir = state.config.storage.effective_payload_dir();

    match dtnpm_core::bundle_inspector::get_bundle_payload_location(&db_path, &payload_dir, &id)
        .await
    {
        Ok(Some(loc)) => {
            super::serve_payload_file_slice_with_range(
                &loc.path,
                loc.offset,
                loc.length,
                loc.mime_type,
                "bundle_payload.bin",
                query.download.unwrap_or(false),
                &req_headers,
            )
            .await
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Bundle or payload not found" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("get_bundle_payload error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to retrieve bundle payload" })),
            )
                .into_response()
        }
    }
}

/// Handler for `DELETE /api/v1/bundles` - Deletes a bundle from storage.
pub async fn delete_bundle_handler(
    State(state): State<AppState>,
    Query(query): Query<BundleQuery>,
) -> Response {
    let Some(id) = query.id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Missing 'id' query parameter" })),
        )
            .into_response();
    };

    let db_path = state.config.storage.effective_db_path();
    let payload_dir = state.config.storage.effective_payload_dir();

    match dtnpm_core::bundle_inspector::delete_bundle_from_storage(&db_path, &payload_dir, &id)
        .await
    {
        Ok(true) => {
            state.service_tracker.broadcast_bundles_update();
            (
                StatusCode::OK,
                Json(json!({ "status": "deleted", "bundle_id": id })),
            )
                .into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Bundle not found" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("get_bundle_payload error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to retrieve bundle payload" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use dtnpm_core::service_tracker::ServiceEvent;
    use serde_json::json;

    use crate::TestServerBuilder;

    #[tokio::test]
    async fn generic_sender_with_chat_source_records_and_broadcasts_chat_message() {
        let srv = TestServerBuilder::new()
            .node_id("dtn://stata/")
            .callsign("STATA")
            .build()
            .await
            .expect("test server");

        let mut rx = srv.ctx.service_tracker.subscribe();

        // 1. Generic sender with sender EID ending in /chat -> recorded as outbound ChatMessage + SSE
        let res = srv
            .post_json(
                "/api/v1/bundles",
                json!({
                    "source": "dtn://stata/chat",
                    "destination": "dtn://statb/chat",
                    "payload": "hello via generic sender",
                    "status_reports": true
                }),
            )
            .await;
        assert_eq!(res.status(), axum::http::StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json_res: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let bundle_id = json_res["bundle_id"].as_str().unwrap().to_string();

        let msgs = srv
            .ctx
            .internal_storage
            .get_conversation_messages("dtn://statb/chat")
            .unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, bundle_id);
        assert_eq!(msgs[0].sender_eid, "dtn://stata/chat");
        assert_eq!(msgs[0].recipient_eid, "dtn://statb/chat");
        assert_eq!(msgs[0].content, "hello via generic sender");
        assert_eq!(msgs[0].direction, "outbound");
        assert_eq!(msgs[0].status, "sent");

        let mut saw_chat_event = false;
        while let Ok(ev) = rx.try_recv() {
            if let ServiceEvent::ChatMessage { message } = ev
                && message["id"] == bundle_id
            {
                saw_chat_event = true;
            }
        }
        assert!(saw_chat_event, "expected ServiceEvent::ChatMessage on SSE");

        // 2. Non-/chat sender EID (e.g. /files) -> must NOT record in chat_messages
        let res2 = srv
            .post_json(
                "/api/v1/bundles",
                json!({
                    "source": "dtn://stata/files",
                    "destination": "dtn://statb/chat",
                    "payload": "file payload"
                }),
            )
            .await;
        assert_eq!(res2.status(), axum::http::StatusCode::CREATED);

        let msgs_after = srv
            .ctx
            .internal_storage
            .get_conversation_messages("dtn://statb/chat")
            .unwrap();
        assert_eq!(msgs_after.len(), 1);
    }
}
