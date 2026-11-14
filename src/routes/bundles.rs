//! Bundle creation and injection endpoint handler.

use axum::Json;
use axum::extract::{FromRequest, Multipart, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use bytes::Bytes;
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
}

fn normalize_eid(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s
        .strip_prefix("dtn://")
        .filter(|r| !r.is_empty() && !r.contains('/'))
    {
        return format!("dtn://{}/", rest);
    }
    s.to_string()
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

async fn build_and_inject_bundle(
    state: &AppState,
    destination_str: &str,
    source_str: Option<&str>,
    lifetime_secs: Option<u64>,
    payload_bytes: Vec<u8>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let payload_bytes = maybe_remux_webm_to_ogg(payload_bytes).await;
    if destination_str.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Destination EID cannot be empty".to_string(),
        ));
    }

    let norm_dest = normalize_eid(destination_str);
    let destination_eid: Eid = norm_dest.parse().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid destination EID '{norm_dest}': {e}"),
        )
    })?;

    let source_eid: Eid = if let Some(src) = source_str.filter(|s| !s.trim().is_empty()) {
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

    let (bundle, binbundle) = Builder::new(source_eid.clone(), destination_eid.clone())
        .with_payload(payload_bytes.into())
        .with_lifetime(lifetime)
        .build(CreationTimestamp::now())
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build bundle: {e}"),
            )
        })?;

    let dest_str = destination_eid.to_string();
    let binbundle_vec = if let Some(key) = state.keystore.find_outbound_key(&dest_str) {
        tracing::info!(
            "Signing bundle with BPSec BIB key pattern '{}'",
            key.pattern
        );
        dtnpm_core::security::sign_bundle_with_key(&bundle, &binbundle, &key, None).map_err(
            |e| {
                tracing::error!("BPSec BIB signing failed for destination '{dest_str}': {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to sign bundle with BPSec key: {e}"),
                )
            },
        )?
    } else {
        binbundle.into_vec()
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

    if content_type.starts_with("multipart/form-data") {
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
        let mut payload_bytes: Option<Vec<u8>> = None;

        while let Ok(Some(field)) = multipart.next_field().await {
            let name = field.name().unwrap_or("").to_string();
            match name.as_str() {
                "destination" => {
                    if let Ok(text) = field.text().await {
                        destination = Some(text);
                    }
                }
                "source" => {
                    if let Ok(text) = field.text().await {
                        source = Some(text);
                    }
                }
                "lifetime" => {
                    if let Ok(text) = field.text().await {
                        lifetime = text.trim().parse::<u64>().ok();
                    }
                }
                "payload" | "file" => {
                    if let Ok(bytes) = field.bytes().await {
                        payload_bytes = Some(bytes.to_vec());
                    }
                }
                _ => {}
            }
        }

        let dest = destination.unwrap_or_default();
        let payload = payload_bytes.unwrap_or_default();

        match build_and_inject_bundle(&state, &dest, source.as_deref(), lifetime, payload).await {
            Ok(res) => (StatusCode::CREATED, Json(res)).into_response(),
            Err((code, err)) => (code, Json(json!({ "error": err }))).into_response(),
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

        match build_and_inject_bundle(
            &state,
            &req.destination,
            req.source.as_deref(),
            req.lifetime,
            req.payload.into_bytes(),
        )
        .await
        {
            Ok(res) => (StatusCode::CREATED, Json(res)).into_response(),
            Err((code, err)) => (code, Json(json!({ "error": err }))).into_response(),
        }
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
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response()
            }
        }
    } else {
        match dtnpm_core::bundle_inspector::list_bundles_from_db(
            &db_path,
            Some(&payload_dir),
            Some(&state.keystore),
        ) {
            Ok(bundles) => Json(json!(bundles)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response(),
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
