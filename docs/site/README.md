# Tide Site-Agent Package

This directory is generated from `config/product.json` and `config/release.json`. It is the machine-readable handoff for the Ultra.dev site agent.

- `tide-release.json` contains authoritative release facts and claim boundaries.
- `WEBSITE_CONTENT_GUIDE.md` contains approved copy and prohibited claims.
- Release artifacts and the common installer are under private validation. Download buttons remain disabled; signing credentials and private acceptance material are excluded from this handoff.

## Installer redirects

The website does not host installer scripts or version files. Each GitHub release publishes `install.sh` and `uninstall.sh`, covered by its `SHA256SUMS` and build attestation. Configure these HTTPS redirects as temporary redirects (302 or 307), never as HTML pages or permanent (301/308) redirects, so `curl -fsSL` always follows them to the current release:

| Website URL | Redirect to |
| --- | --- |
| `https://ultra.dev/tide/install.sh` | `https://github.com/ultradotdev/tide/releases/latest/download/install.sh` |
| `https://ultra.dev/tide/uninstall.sh` | `https://github.com/ultradotdev/tide/releases/latest/download/uninstall.sh` |

`https://ultra.dev/tide/releases/latest-version` is no longer read by the installer and can be retired. GitHub's `latest` URLs skip prereleases: until a release that is not marked as a prerelease exists, they return 404 and the redirects cannot serve an installer. Show the canonical GitHub command from `commands.binaryInstall`; the short website form is `curl -fsSL https://ultra.dev/tide/install.sh | sh`.

Regenerate with `cargo xtask generate-tide-site-package --write`. Verify with `--check`. Do not edit generated files directly.
