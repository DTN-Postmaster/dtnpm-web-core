# ADR 0002: Strict CORS Origin Parsing, Cookie CSRF Checks, and Trusted Proxy Isolation

- **Status**: Accepted
- **Scope**: `dtnpm-web-core` (`src/server.rs`, `src/auth.rs`, `src/routes/auth.rs`)

## Context
Prefix matching (`origin.starts_with("http://localhost")`) allows lookalike origins such as `http://localhost.attacker.example` (`WEB-01`). Additionally, `HttpOnly` cookie sessions (`dtnpm_session`) require CSRF protection on state-changing HTTP verbs, and proxy headers (`Remote-User`, `X-Forwarded-Proto`) can be spoofed when exposed directly without trusted-proxy validation (`WEB-04`).

## Decision
1. Parse `Origin` as `axum::http::Uri` in CORS predicate and accept only exact hosts `localhost`, `127.0.0.1`, or `[::1]` on `http`/`https`.
2. Enforce `Sec-Fetch-Site != cross-site` and `Origin` host matching `Host` on cookie-authenticated `POST`/`PUT`/`DELETE`/`PATCH` requests (exempting stateless `Authorization: Bearer` tokens and `auth.mode = "none"` opt-out).
3. Restrict `X-Forwarded-Proto` and proxy identity headers strictly to peers matching `[auth.proxy].trusted-proxies`, and attach `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, and `Referrer-Policy: no-referrer` on all responses.

## Consequences
- Eliminates lookalike CORS bypasses, cross-origin CSRF against operator sessions, and forged TLS/identity headers from untrusted clients.
