# Rust Parity Fixtures

These files were generated from the TypeScript authorities and are consumed by
the Rust tests. They are frozen: the TypeScript application and its generator,
`scripts/generate-rust-parity-fixtures.ts`, were retired from this repository and
remain on the TypeScript branch. Edit a fixture only together with the Rust test
that reads it.

Files:

| File | Source | Rust test |
| --- | --- | --- |
| `settings-defaults.json` | `config/defaults.ts` | `tide-config` parses 90 settings |
| `keybindings-defaults.json` | `config/defaults.ts` | `tide-config` parses 198 bindings |
| `commands-defaults.json` | `config/defaults.ts` | `tide-config` parses 274 commands |
| `themes-defaults.json` | `config/defaults.ts` | `tide-config` parses 13 themes |
| `session-v7.json` | Session V7 contract | `tide-config` round-trips V7 state |

Ensemble-specific fixture data is intentionally excluded from the Rust target.
