# ADR 0004: Nonce-Protected Markdown Sanitizer and Opt-In External Image Loading

- **Status**: Accepted
- **Scope**: `dtnpm-web-core` (`ui/src/utils/markdown.ts`, `ui/tests/markdown.spec.ts`)

## Context
Static predictable placeholder tokens (`@@HDYTOKEN0@@`), post-escape bare autolink matching (which could swallow `&gt;` after `escapeHtml`), nested-quantifier link regexes, and automatic fetching of remote HTTP/HTTPS or SVG `data:` images expose DTN operators to IP-leak tracking over clearnet, SVG script vectors, and UI hangs (`UI-01`, `XC-06`).

## Decision
1. Generate a per-render random placeholder nonce delimited by `\x00` (after stripping NUL bytes from input and avoiding `_`/`*` inside tokens).
2. Extract `<https://...>` / `<dtn://...>` and bare autolinks as placeholders prior to `escapeHtml`, and replace the link regex with a linear-time expression preserving optional titles.
3. Restrict `isSafeImageUrl` by default to raster `data:image/(png|jpeg|jpg|gif|webp);base64,...` URIs and relative paths; loading external `http(s)://` images requires explicit opt-in via `{ allowExternalImages: true }`.

## Consequences
- Offline amateur radio nodes avoid inadvertent clearnet DNS/HTTP requests and SVG XSS while retaining full GFM checklist, code block, and inline raster preview support.
