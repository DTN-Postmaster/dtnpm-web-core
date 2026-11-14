# Guide de Contribution — `dtnpm-web-core`

Ce guide présente les règles de développement et les exigences de sécurité pour le serveur Web et l'interface utilisateur de `dtnpm-web-core`.

---

## 1. Principes Fondamentaux

- **Zéro Panic / Zéro Unwrap** : Aucun appel `.unwrap()` ou `.expect()` en code de production.
- **Sécurité Web OWASP** :
  - Toujours valider les origines des requêtes CORS et CSRF.
  - Ne jamais exposer de contenu utilisateur non assaini en mode `inline` (forcer `attachment` pour les formats à risque comme SVG).
  - Borner strictement les processus enfants (`ffmpeg`) et les limites de taille de payload.
- **Traçabilité Architecturale (ADR)** :
  - Tout changement de routage, d'authentification ou de rendu UI doit être consigné dans `adr/` et indexé dans `adr/README.md`.

---

## 2. Vérification Locale

```bash
cargo fmt --all -- --check
cargo clippy --offline --workspace --lib --bins -- -D warnings -D clippy::unwrap_used -D clippy::panic
cargo test --offline --all-features
./scripts/check-security.sh --strict
```
