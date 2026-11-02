# Contributing to Tide

Tide is a Rust workspace. The executable is in `crates/tide`; UI coordination is in `crates/tide-ui`. See the [architecture overview](docs/architecture/overview.md).

Report bugs and request features through [GitHub issues](https://github.com/ultradotdev/tide/issues), and open pull requests against `main`. Report vulnerabilities privately as described in the [security policy](SECURITY.md).

## Setup

Use the pinned `rust-toolchain.toml`, Git and a native C compiler/linker. Everything else, including maintainer tooling, is Rust: `cargo xtask <task>` runs the `xtask` crate, and `cargo xtask --help` lists every task.

```sh
cargo build --locked --release -p tide
./target/release/tide .
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Enable the tracked pre-commit hook once per clone. It runs `cargo fmt --all -- --check`:

```sh
git config core.hooksPath .githooks
```

Use isolated test homes and fixture workspaces. Never run destructive Git or database acceptance against your real project or database. Tests that need live PostgreSQL/TLS, real language servers or vim are `#[ignore]` with a reason and fail loudly when opted in. CI runs them with `-- --ignored`; `cargo xtask verify-rust-test-readiness` runs everything with `-- --include-ignored` against disposable fixtures. The TypeScript language server for those tests installs with `npm i -g typescript-language-server@5.1.3 typescript@5.9.3` (the versions CI pins).

## Generated references

The compiled exporter uses the same command routes, aliases, default settings, constraints and alternate bindings as the workbench. It does not open a workspace, profile, provider store or PTY.

```sh
cargo xtask generate-rust-reference --write
cargo xtask generate-rust-reference --check
cargo xtask generate-tide-site-package --write
cargo xtask generate-rust-notices --check
cargo xtask verify-tide-docs
cargo xtask validate-doc-links
```

Commit generated JSON and Markdown alongside their source changes. The runtime reference is authoritative for supported preferences and effective bindings. Embedded compatibility entries can exist without being supported user controls. The site package records the last commit that touched its inputs, so regenerate it with `--write` after committing an input change.

## Boundaries

- Core text operations are independent of terminal rendering.
- Service and provider crates expose typed operations. They do not import UI code.
- UI rendering and mouse targets use the same geometry.
- Keep parsing, discovery and potentially slow service work off the input/render path.
- Secret contents, provider histories and real connection strings do not belong in fixtures or logs.
- Protect unsaved buffers and require confirmation for destructive actions.
- Use semantic theme roles and preserve child terminal colors.

## Tests and release tooling

Rust unit, PTY, database and real-language-server gates are distinct. A missing prerequisite must be reported explicitly; an optional skip is not evidence of coverage. Dependency advisories, duplicate versions, licenses and sources are enforced by `cargo deny check` with the policy in `deny.toml`.

Performance measurements (`cargo xtask measure-rust-performance` and the `acceptance_performance` example) run on shared CI runners as reports with generous ceilings. The strict budgets apply on dedicated hardware through `cargo xtask verify-rust-test-readiness`.

Release archives contain `tide`, `uninstall.sh`, `LICENSE`, `ATTRIBUTIONS.md` and `VERSION.json`. macOS archives require Developer ID signing, hardened runtime and accepted Apple notarization using an App Store Connect API key. `cargo xtask package-release-artifact`, `sign-notarize-macos` and `generate-release-checksums` build and verify the resulting payload; their command construction is unit-tested without running signing tools. Manual rehearsal workflows do not publish. Publication happens only when the owner pushes the configured `v*` tag; `cargo xtask release-workflow-contract preflight` checks the tag, event and actor, and version tags are immutable.

Each release also publishes the source's `install.sh` and `uninstall.sh`. `generate-release-checksums` refuses copies that differ from the tagged source and lists them in `SHA256SUMS`, so the checksum-driven attestation covers them and `https://github.com/ultradotdev/tide/releases/latest/download/install.sh` is the one-line installer. `verify-rust-release-install` exercises the installer's latest-release lookup against a loopback server that imitates GitHub's `/releases/latest` redirect (`TIDE_LATEST_VERSION_URL` and `TIDE_RELEASE_BASE_URL` override the GitHub URLs).

After publication, the release workflow's `Release / Homebrew tap` job updates the `ultradotdev/homebrew-tap` tap (named by `releasePolicy.homebrewTap` in `config/release.json`). It runs only when the same owner tag run published, checks that the published `SHA256SUMS` matches the assembled one, regenerates `Formula/tide.rb` with `cargo xtask homebrew-formula --version <version> --checksums <SHA256SUMS> --output <path>`, and commits and pushes only a changed formula. Never edit the formula in the tap by hand. The job needs:

- the `ultradotdev/homebrew-tap` repository with an initial commit on its default branch;
- a `HOMEBREW_TAP_TOKEN` secret on the `release` environment: a fine-grained personal access token whose resource owner is `ultradotdev`, with repository access limited to `ultradotdev/homebrew-tap` and the **Contents: Read and write** permission (Metadata: read is implied). The job's own `GITHUB_TOKEN` stays read-only.

Because the job uses the `release` environment, the owner approves it after approving publication. Check a generated formula locally with `cargo xtask homebrew-formula --version <version> --checksums SHA256SUMS --output tide.rb` and `brew style tide.rb` (a Homebrew developer command: it turns on Homebrew's developer mode); `--download-base-url file:///…` points it at locally packaged archives.

Changes should include a concrete trigger, resulting behavior, relevant checks and any remaining limitation. Keep screenshots synthetic and free of personal files, session names and credentials. CI scans the history for secrets with Gitleaks and rejects tracked files that contain machine-specific absolute paths (`cargo xtask verify-portable-paths`). Read the [security policy](SECURITY.md) and [Code of Conduct](CODE_OF_CONDUCT.md).

## Cutting a release

The workspace `Cargo.toml` version is the release version. To prepare release `X.Y.Z`:

1. Set `X.Y.Z` in `Cargo.toml` and `config/product.json`, and `vX.Y.Z` in `config/release.json` (`releasePolicy.tag`, `title`, `notesPath` and the artifact filenames). Run `cargo update -w` to refresh `Cargo.lock`.
2. Write `docs/vX.Y.Z.md`, starting with `# Tide vX.Y.Z Release Notes`, and link it from `README.md` and `docs/README.md`.
3. Regenerate with `cargo xtask generate-rust-reference --write`, `generate-rust-notices --write` and `generate-tide-site-package --write`. `verify-release-artifacts` fails if any of these disagree with `Cargo.toml`.
4. Merge the release pull request, then push the `vX.Y.Z` tag from `main` and approve the `release-signing` and `release` environments.

## License

Tide is licensed under the [Apache License, Version 2.0](LICENSE). Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion is licensed under the same terms, as set out in section 5 of the license, without additional terms or conditions.
