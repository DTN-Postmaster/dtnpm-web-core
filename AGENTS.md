# Agent Guide — `dtnpm-web-core`

## 🎯 Mission
Fournir le serveur Web d'administration et de supervision Axum, le registre de plugins applicatifs (`WebModule`), l'API de découverte des capacités (`/api/capabilities`), les routes système (configuration, statut des pairs, journal en temps réel via SSE, inspecteur de bundles) et le shell WebUI Preact de **DTN Postmaster**.

---

## 🛡️ Référentiels et Exigences de Sécurité

### 1. Cyber Resilience Act (CRA)
- **Sécurité par défaut (Secure-by-Default)** :
  - Authentification locale activée par défaut avec sessions sécurisées (`HttpOnly`, `SameSite=Lax`, `Secure` sous TLS).
  - Images distantes désactivées par défaut dans le rendu Markdown pour prévenir toute fuite d'adresse IP vers des tiers.
- **Supply-Chain & VEX** :
  - Audit des dépendances via `cargo-audit` et `cargo-deny`. Déclaration VEX `not_affected` pour `RUSTSEC-2026-0459` dans `.cargo/audit.toml`.
  - Actions GitHub CI épinglées par SHA-1 complet à 40 caractères avec `permissions: contents: read`.
- **Politique de sécurité** : Consignée dans [SECURITY.md](SECURITY.md).

### 2. OWASP Web & API Security
- **CORS Strict** :
  - Correspondance exacte sur l'hôte et le port de l'URI d'origine (`origin.host()`), interdisant les correspondances partielles ou de préfixes (ex. blocage de `http://localhost.evil.com`).
- **Protection Anti-CSRF & Isolation Proxy** :
  - Validation systématique des requêtes mutatives (`POST`, `PUT`, `DELETE`) via les en-têtes `Sec-Fetch-Site: same-origin` ou correspondance stricte `Origin == Host`.
  - Prise en compte de `x-forwarded-proto` UNIQUEMENT en provenance des proxys explicitement approuvés dans `trusted_proxies`.
- **En-têtes de Sécurité HTTP** :
  - Injection systématique des en-têtes `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, et `Referrer-Policy: strict-origin-when-cross-origin`.
- **Prévention DoS & Limitation de Débit** :
  - `LoginRateLimiter` borné en mémoire à 4 096 entrées avec élagage automatique.
  - Transcodage `ffmpeg` borné par un sémaphore global à 2 permis simultanés, timeout dur de 10s, plafond de sortie à 16 Mio et `kill_on_drop(true)`.
- **Contenu Téléchargeable & Protection XSS** :
  - Interdiction de l'affichage inline des fichiers SVG non assainis (conversion forcée en téléchargement `Content-Disposition: attachment`).
  - Noms de fichiers conformes à la RFC 5987 avec caractères d'échappement sécurisés.
  - Rendu Markdown durci contre les collisions de jetons (`crypto.getRandomValues`) et sans regex linéaires vulnérables au ReDoS.

---

## 🦀 Standards d'Ingénierie Rust

1. **Règle Zéro Panic / Zéro Unwrap** :
   - Tout le code sous `src/` doit être 100% exempt de `.unwrap()`, `.expect()` et de macro `panic!`.
   - Utiliser `thiserror`, `anyhow`, et propager les erreurs via `?`.
   - Linter strict : `-D warnings -D clippy::unwrap_used -D clippy::panic`.
2. **ADRs Obligatoires** :
   - Toute décision architecturale Web/UI doit être répertoriée sous `adr/` et indexée dans [adr/README.md](adr/README.md).

---

## ⚡ Commandes Critiques de Vérification

- `cargo fmt --all -- --check`
- `cargo clippy --offline --workspace --lib --bins -- -D warnings -D clippy::unwrap_used -D clippy::panic`
- `cargo test --offline --all-features`
- `./scripts/check-security.sh --strict`
- `./scripts/check-coverage.sh --summary-only`
