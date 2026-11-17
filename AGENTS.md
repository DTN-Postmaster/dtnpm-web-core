# Agent Guide — `dtnpm-web-core`

## 🎯 Mission
Provide the Axum administration and monitoring web server, the application plugin registry (`WebModule`), the capabilities discovery API (`/api/capabilities`), system routes (configuration, peer status, real-time log streaming via SSE, bundle inspector), and the Preact WebUI shell for **DTN Postmaster**.

---

## 📝 Documentation Language

- **English by Default**: English must be used as the language for all documentation, unless the file is suffixed with `.fr.md` or another language is explicitly requested in the prompt.

---

## 🛡️ Security Frameworks and Requirements

### 1. Cyber Resilience Act (CRA)
- **Secure-by-Default**:
  - Local authentication enabled by default with secure sessions (`HttpOnly`, `SameSite=Lax`, `Secure` under TLS).
  - Remote images disabled by default in Markdown rendering to prevent IP address leakage to third parties.
- **Supply-Chain & VEX**:
  - Dependency auditing via `cargo-audit` and `cargo-deny`. `not_affected` VEX declaration for `RUSTSEC-2026-0459` in `.cargo/audit.toml`.
  - GitHub CI actions pinned by full 40-character SHA-1 with `permissions: contents: read`.
- **Security Policy**: Documented in [SECURITY.md](SECURITY.md).

### 2. OWASP Web & API Security
- **Strict CORS**:
  - Exact matching on the host and port of the origin URI (`origin.host()`), forbidding partial or prefix matches (e.g., blocking `http://localhost.evil.com`).
- **Anti-CSRF Protection & Proxy Isolation**:
  - Systematic validation of state-changing requests (`POST`, `PUT`, `DELETE`) via `Sec-Fetch-Site: same-origin` headers or strict `Origin == Host` matching.
  - Honor `x-forwarded-proto` ONLY from proxies explicitly approved in `trusted_proxies`.
- **HTTP Security Headers**:
  - Systematic injection of `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, and `Referrer-Policy: strict-origin-when-cross-origin`.
- **DoS Prevention & Rate Limiting**:
  - `LoginRateLimiter` bounded in memory to 4,096 entries with automatic pruning.
  - `ffmpeg` transcoding bounded by a global semaphore of 2 concurrent permits, 10s hard timeout, 16 MiB output cap, and `kill_on_drop(true)`.
- **Downloadable Content & XSS Protection**:
  - Forbid inline rendering of unsanitized SVG files (forced conversion to `Content-Disposition: attachment` download).
  - RFC 5987 compliant filenames with safe escaping characters.
  - Markdown rendering hardened against token collisions (`crypto.getRandomValues`) and free of linear regexes vulnerable to ReDoS.

---

## 🦀 Rust Engineering Standards

1. **Zero Panic / Zero Unwrap Rule**:
   - All code under `src/` must be 100% free of `.unwrap()`, `.expect()`, and the `panic!` macro.
   - Use `thiserror`, `anyhow`, and propagate errors via `?`.
   - Strict linter: `-D warnings -D clippy::unwrap_used -D clippy::panic`.
2. **Mandatory ADRs**:
   - Any Web/UI architectural decision must be listed under `adr/` and indexed in [adr/README.md](adr/README.md).

---

## ⚡ Critical Verification Commands

- `cargo fmt --all -- --check`
- `cargo clippy --offline --workspace --lib --bins -- -D warnings -D clippy::unwrap_used -D clippy::panic`
- `cargo test --offline --all-features`
- `./scripts/check-security.sh --strict`
- `./scripts/check-coverage.sh --summary-only`
