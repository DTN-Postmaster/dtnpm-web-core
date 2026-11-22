# Agent Guide — `dtnpm-web-core`

## 🎯 Mission
Provide the Axum administration and monitoring web server, the application plugin registry (`WebModule`), the capabilities discovery API (`/api/capabilities`), system routes (configuration, peer status, real-time log streaming via SSE, bundle inspector), and the Preact WebUI shell for **DTN Postmaster**.

---

## 📝 Documentation & User Interface Language

- **English by Default (Docs & UI)**:
  - **Documentation**: English must be used for all documentation, unless the file is suffixed with `.fr.md` or another language is explicitly requested in the prompt.
  - **User Interface (100% English)**: Every user-facing string in `ui/src/` and `ui/tests/` MUST be written in English — including navbar tabs, Station sub-tabs, headings, labels, placeholders, buttons, `SecurityBadge` statuses, tooltips (`title`), `aria-label` attributes, status banners, `alert()`/`confirm()` dialogs, and error strings. Never introduce French or mixed-language copy.

## 🧭 WebUI Navigation & Sub-Tab Architecture

- **Global Navbar (`ui/src/components/Navbar.tsx`)**: Strictly limited to primary operational workflows (`NavTabId`: `'station' | 'chat' | 'files' | 'senml' | 'send'` — `Station`, `Chat`, `Files`, `SenML`, `Send Bundle`), gated by `enabledModules` from `/api/capabilities`. Do **not** add configuration, routing, filtering, security, or diagnostic screens to the global navbar.
- **Station Sub-Tabs (`ui/src/components/ConfigView.tsx`)**: All station administration, routing, policy, security, storage, and diagnostics live as sub-tabs inside `Station` (`StationSubTab`):
  - Order: `radio` (`Links & Radio`) → `routes` (`Routing (RIB)`) → `rego_routing` (`Rego Routing`, gated by `enabledModules.includes('rego_routing')`) → `security` (`BPSec Security`) → `bundles` (`Storage & Bundles`) → `logs` (`Logs & Diagnostics`) → `advanced` (`JSON Config`).
- **Component Synchronization**: When updating shared UI components (`SecurityBadge.tsx`, `markdown.ts`, shared module views) or API types, keep `dtnpm-web-core/ui`, `dtnpm-mod-*/ui`, and `template-dtnpm-mod/ui` synchronized and free of non-English copy.

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
- `cd ui && pnpm build && pnpm test:e2e` (verify WebUI bundle builds and all Playwright specs pass with English UI assertions)
- `./scripts/check-security.sh --strict`
- `./scripts/check-coverage.sh --summary-only`
