# Contributing Guide — `dtnpm-web-core`

This guide outlines the development rules and security requirements for the `dtnpm-web-core` web server and user interface.

---

## 1. Core Principles

- **Zero Panic / Zero Unwrap**: No `.unwrap()` or `.expect()` calls in production code.
- **OWASP Web Security**:
  - Always validate request origins for CORS and CSRF.
  - Never expose unsanitized user content in `inline` mode (force `attachment` for risky formats such as SVG).
  - Strictly bound child processes (`ffmpeg`) and payload size limits.
- **Architectural Traceability (ADR)**:
  - Any change to routing, authentication, or UI rendering must be recorded in `adr/` and indexed in `adr/README.md`.

---

## 2. Local Verification

```bash
cargo fmt --all -- --check
cargo clippy --offline --workspace --lib --bins -- -D warnings -D clippy::unwrap_used -D clippy::panic
cargo test --offline --all-features
./scripts/check-security.sh --strict
```
