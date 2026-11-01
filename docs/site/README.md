# Tide Site-Agent Package

This directory is generated from `config/product.json` and `config/release.json`. It is the machine-readable handoff for the Ultra.dev site agent.

- `tide-release.json` contains authoritative release facts and claim boundaries.
- `WEBSITE_CONTENT_GUIDE.md` contains approved copy and prohibited claims.
- Rust artifacts and the common installer are under private validation. Download buttons remain disabled; signing credentials and private acceptance material are excluded from this handoff.

Regenerate with `cargo xtask generate-tide-site-package --write`. Verify with `--check`. Do not edit generated files directly.
