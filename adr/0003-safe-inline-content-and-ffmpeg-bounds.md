# ADR 0003: Safe Payload Disposition (Blocking Inline SVG) and Bounded `ffmpeg` Remuxing

- **Status**: Accepted
- **Scope**: `dtnpm-web-core` (`src/routes/mod.rs`, `src/routes/bundles.rs`)

## Context
Serving received bundle payloads (`image/svg+xml`) with `Content-Disposition: inline` poses stored-XSS and MIME risks if opened directly, and header construction using unchecked `"inline"` fallback or loose filename characters risks header injection (`WEB-02`). Furthermore, spawning unbounded `ffmpeg` subprocesses on WebM payloads could exhaust CPU/RAM on embedded hardware (`WEB-03`).

## Decision
1. Remove `image/svg+xml` from `is_safe_inline_mime`, filter `Content-Disposition` filenames strictly to printable ASCII graphic characters excluding `"`, `\`, and `;`, fall back to `"attachment"` on error, and keep `Content-Security-Policy: default-src 'none'; sandbox`.
2. Bound `maybe_remux_webm_to_ogg` with a 2-permit semaphore, 16 MiB payload limit, `tokio::process::Command` with `kill_on_drop(true)`, and a 10-second execution timeout.

## Consequences
- Untrusted DTN attachments cannot execute inline scripts or spawn runaway transcoding processes.
