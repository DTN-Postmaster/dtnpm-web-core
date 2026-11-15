//! Web API routes.

pub mod auth;
pub mod bundles;
pub mod clas;
pub mod config;
pub mod security;
pub mod services;
pub mod static_routes;

use axum::{
    Json,
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;

/// Prepares common headers for serving payload data (Content-Type, Content-Disposition, CSP, etc.).
fn prepare_payload_headers(
    total_len: u64,
    mime_type: &str,
    filename: &str,
    is_download: bool,
) -> HeaderMap {
    // Determine safe disposition type.
    // Dangerous web types like HTML and SVG must NEVER be rendered inline; force attachment.
    let is_safe_inline_mime = mime_type.starts_with("image/png")
        || mime_type.starts_with("image/jpeg")
        || mime_type.starts_with("image/gif")
        || mime_type.starts_with("image/webp")
        || mime_type.starts_with("audio/")
        || mime_type.starts_with("video/")
        || mime_type.starts_with("text/plain")
        || mime_type.starts_with("text/markdown");

    let disposition_type = if is_download || !is_safe_inline_mime {
        "attachment"
    } else {
        "inline"
    };

    // Sanitize filename to safe printable ASCII graphic characters for Content-Disposition header
    let safe_filename: String = filename
        .chars()
        .filter(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\\' | ';'))
        .take(128)
        .collect();
    let safe_filename = if safe_filename.is_empty() {
        "download.bin".to_string()
    } else {
        safe_filename
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disposition_type}; filename=\"{safe_filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    let _ = total_len;
    headers
}

/// Parses standard HTTP Range header (e.g. bytes=0-1023, bytes=1024-, bytes=-500).
fn parse_http_range(req_headers: &HeaderMap, total_len: u64) -> Option<Result<(u64, u64), ()>> {
    let range_header = req_headers.get(header::RANGE)?.to_str().ok()?;
    let range_spec = range_header.strip_prefix("bytes=")?;
    let first_range = range_spec.split(',').next()?.trim();
    let (start_str, end_str) = first_range.split_once('-')?;
    if total_len == 0 {
        return Some(Err(()));
    }

    if start_str.is_empty() {
        // Suffix range: -N (last N bytes)
        let suffix_len = end_str.parse::<u64>().ok()?;
        if suffix_len == 0 {
            return Some(Err(()));
        }
        let start = total_len.saturating_sub(suffix_len);
        let end = total_len - 1;
        Some(Ok((start, end)))
    } else if let Ok(start) = start_str.parse::<u64>() {
        if start < total_len {
            let end = if end_str.is_empty() {
                total_len - 1
            } else {
                match end_str.parse::<u64>() {
                    Ok(v) => v.min(total_len - 1),
                    Err(_) => return Some(Err(())),
                }
            };
            if start <= end {
                Some(Ok((start, end)))
            } else {
                Some(Err(()))
            }
        } else {
            Some(Err(()))
        }
    } else {
        Some(Err(()))
    }
}

fn refine_ogg_opus_mime(raw_mime: &str, head: &[u8], total_len: u64) -> String {
    if (raw_mime == "audio/ogg" || raw_mime == "audio/opus" || raw_mime.is_empty())
        && total_len >= 36
        && head.len() >= 36
        && &head[0..4] == b"OggS"
    {
        let page_segments = head[26] as usize;
        let packet_offset = 27 + page_segments;
        if head.len() >= packet_offset + 8 && &head[packet_offset..packet_offset + 8] == b"OpusHead"
        {
            return "audio/ogg; codecs=opus".to_string();
        }
    }
    raw_mime.to_string()
}

/// Serves a byte payload with support for Content-Type, Content-Disposition, and HTTP Range requests (RFC 9110).
pub fn serve_payload_with_range(
    bytes: Bytes,
    raw_mime: &str,
    filename: &str,
    is_download: bool,
    req_headers: &HeaderMap,
) -> Response {
    let total_len = bytes.len() as u64;
    let mime_type = refine_ogg_opus_mime(raw_mime, &bytes, total_len);

    let mut headers = prepare_payload_headers(total_len, &mime_type, filename, is_download);

    // Handle HTTP Range header
    if let Some(range_res) = parse_http_range(req_headers, total_len) {
        match range_res {
            Ok((start, end)) => {
                let range_len = end - start + 1;
                if let Ok(hv) = HeaderValue::from_str(&format!("bytes {start}-{end}/{total_len}")) {
                    headers.insert(header::CONTENT_RANGE, hv);
                }
                headers.insert(header::CONTENT_LENGTH, HeaderValue::from(range_len));

                let sliced = bytes.slice((start as usize)..=(end as usize));
                return (StatusCode::PARTIAL_CONTENT, headers, Body::from(sliced)).into_response();
            }
            Err(()) => {
                if let Ok(hv) = HeaderValue::from_str(&format!("bytes */{total_len}")) {
                    headers.insert(header::CONTENT_RANGE, hv);
                }
                return (StatusCode::RANGE_NOT_SATISFIABLE, headers, Body::empty()).into_response();
            }
        }
    }

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(total_len));
    (StatusCode::OK, headers, Body::from(bytes)).into_response()
}

/// Streams an arbitrary byte slice of a file from disk with O(1) memory footprint and full HTTP Range support.
pub async fn serve_payload_file_slice_with_range(
    path: &std::path::Path,
    slice_offset: u64,
    slice_len: u64,
    raw_mime: &str,
    filename: &str,
    is_download: bool,
    req_headers: &HeaderMap,
) -> Response {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    use tokio_util::io::ReaderStream;

    let total_len = slice_len;
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // Check Ogg Opus header from first 64 bytes of the slice using the same handle
    let mut mime_type = raw_mime.to_string();
    if (mime_type == "audio/ogg" || mime_type == "audio/opus" || mime_type.is_empty())
        && total_len >= 36
        && file
            .seek(std::io::SeekFrom::Start(slice_offset))
            .await
            .is_ok()
    {
        let mut head = [0u8; 64];
        if let Ok(n) = file.read(&mut head).await {
            mime_type = refine_ogg_opus_mime(raw_mime, &head[..n], total_len);
        }
    }

    let mut headers = prepare_payload_headers(total_len, &mime_type, filename, is_download);

    if let Some(range_res) = parse_http_range(req_headers, total_len) {
        match range_res {
            Ok((start, end)) => {
                let range_len = end - start + 1;
                if let Ok(hv) = HeaderValue::from_str(&format!("bytes {start}-{end}/{total_len}")) {
                    headers.insert(header::CONTENT_RANGE, hv);
                }
                headers.insert(header::CONTENT_LENGTH, HeaderValue::from(range_len));

                let physical_start = slice_offset.saturating_add(start);
                if let Err(e) = file.seek(std::io::SeekFrom::Start(physical_start)).await {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": e.to_string() })),
                    )
                        .into_response();
                }
                let take = file.take(range_len);
                let stream = ReaderStream::new(take);
                return (
                    StatusCode::PARTIAL_CONTENT,
                    headers,
                    Body::from_stream(stream),
                )
                    .into_response();
            }
            Err(()) => {
                if let Ok(hv) = HeaderValue::from_str(&format!("bytes */{total_len}")) {
                    headers.insert(header::CONTENT_RANGE, hv);
                }
                return (StatusCode::RANGE_NOT_SATISFIABLE, headers, Body::empty()).into_response();
            }
        }
    }

    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(total_len));
    if let Err(e) = file.seek(std::io::SeekFrom::Start(slice_offset)).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    let take = file.take(total_len);
    let stream = ReaderStream::new(take);
    (StatusCode::OK, headers, Body::from_stream(stream)).into_response()
}

/// Streams an entire file from disk with O(1) memory footprint and full HTTP Range support.
pub async fn serve_payload_file_with_range(
    path: &std::path::Path,
    raw_mime: &str,
    filename: &str,
    is_download: bool,
    req_headers: &HeaderMap,
) -> Response {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    serve_payload_file_slice_with_range(
        path,
        0,
        metadata.len(),
        raw_mime,
        filename,
        is_download,
        req_headers,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::prepare_payload_headers;
    use axum::http::header;

    #[test]
    fn test_prepare_payload_headers_forces_attachment_for_svg_and_sanitizes_filename() {
        let headers = prepare_payload_headers(128, "image/svg+xml", "evil\";\r\n.svg", false);
        let disp = headers
            .get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(disp.starts_with("attachment; filename=\""));
        assert!(!disp.contains('\r'));
        assert!(!disp.contains('\n'));
        assert_eq!(disp, "attachment; filename=\"evil.svg\"");

        let utf8_headers = prepare_payload_headers(64, "image/png", "éé", false);
        assert_eq!(
            utf8_headers
                .get(header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok()),
            Some("inline; filename=\"download.bin\"")
        );
    }
}
