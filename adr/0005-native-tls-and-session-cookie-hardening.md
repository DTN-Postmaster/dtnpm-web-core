# ADR 0005: Optional Native TLS Termination and Bounded Rate-Limited Auth Cookies

- **Status**: Accepted
- **Scope**: `dtnpm-web-core` (`src/server.rs`, `src/auth.rs`, `src/routes/auth.rs`)

## Context
Local authentication protects management endpoints when exposed on LAN or reverse-proxied WAN links, while development/offline deployments may still explicitly configure `[auth].mode = "none"`. Unbounded failed-login tracking maps risk memory exhaustion from distributed password spraying (`WEB-04`).

## Decision
Support `[tls].enabled = true` via `tokio-rustls` (`ring`), emit `HttpOnly; SameSite=Lax` cookies (upgrading to `Secure` under TLS or trusted TLS proxies), and cap `LoginRateLimiter` at 4,096 tracked client keys with expired/oldest-entry eviction. Retain `auth.mode = "none"` exclusively as an explicit opt-in for isolated development or loopback-only deployments.

## Consequences
- Predictable memory ceiling under brute-force traffic without breaking standalone test/development workflows.
