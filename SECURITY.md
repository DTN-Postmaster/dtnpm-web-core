# Security Policy — `dtnpm-web-core`

## Supported Versions

| Version | Supported | Security Updates |
|---------|-----------|------------------|
| 0.2.x   | Yes       | Active           |
| < 0.2   | No        | End of Life      |

## Security Architecture & EU Cyber Resilience Act (CRA) Alignment

`dtnpm-web-core` provides the shared Axum REST/SSE web gateway, authentication middleware, and Preact UI shell for `dtn-postmaster`:

- **CORS & CSRF Isolation**: Strict `axum::http::Uri` host validation limits CORS origins to `localhost`, `127.0.0.1`, and `[::1]`. Cookie-authenticated state-changing requests (`POST`, `PUT`, `DELETE`, `PATCH`) enforce `Sec-Fetch-Site` and `Origin` vs `Host` matching (`src/auth.rs`).
- **Response Security Headers**: All HTTP responses receive `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, and `Referrer-Policy: no-referrer` by default, while untrusted downloads use `Content-Disposition: attachment` and sandboxed CSP (`default-src 'none'; sandbox`), blocking inline SVG execution (`src/routes/mod.rs`).
- **Trusted Proxy Isolation**: Proxy identity headers (`Remote-User`) and `X-Forwarded-Proto` are honored only when the TCP peer matches `[auth.proxy].trusted-proxies`.
- **Rate Limiting & Subprocess Bounds**: `LoginRateLimiter` enforces exponential backoff capped at 4,096 tracked IPs (`src/auth.rs`). Optional WebM-to-Ogg `ffmpeg` remuxing is bounded by a 2-permit semaphore, 16 MiB input cap, and 10-second timeout (`src/routes/bundles.rs`).
- **Frontend Markdown & Media Privacy**: `ui/src/utils/markdown.ts` uses per-call random nonces, extracts autolinks prior to HTML escaping, avoids backtracking-prone regexes, and blocks external `http(s)` images unless `allowExternalImages: true` is opted into.
- **VEX & Supply Chain**: Switching `dtnpm-core` from `serde_yaml` to `yaml_serde` removes `unsafe-libyaml` (`RUSTSEC-2024-0449`). `RUSTSEC-2026-0459` (`libsqlite3-sys` SQLite `session` extension) is VEX Not Affected (`rusqlite` `session` feature disabled).

## Reporting a Vulnerability

Report security issues privately via GitHub Security Advisories for `dtn-postmaster/dtnpm-web-core` rather than public issues.
