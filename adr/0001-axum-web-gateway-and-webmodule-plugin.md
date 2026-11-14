# ADR 0001: Axum Web Gateway and `WebModule` Plugin Trait

- **Status**: Accepted
- **Scope**: `dtnpm-web-core` (`src/module.rs`, `src/server.rs`, `src/context.rs`)

## Context
Service modules (`chat`, `files`, `senml`) need to run both embedded inside the standalone `dtnpm` binary and independently against a remote gRPC node.

## Decision
Define the `WebModule` trait and `ModuleContext` abstraction in `dtnpm-web-core` backed by Axum 0.8 and `BpaClient`, exposing `/api/capabilities` discovery for dynamic Preact SPA tab mounting and a 64 MiB default body limit.

## Consequences
- Clean separation between system routes (`/api/v1/*`) and modular endpoints (`/api/{id}/v1/*`).
- Shared PBAC authentication, security headers, and error envelopes apply uniformly across all mounted modules.
