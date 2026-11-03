# Contributing

Use synthetic accounts and sanitized fixtures. Keep personal flows, browser profiles, baselines, recordings, local configuration and credentials in ignored local directories. Do not force-add these assets. Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

Before submitting a change:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test tests/index_security.cjs
python3 -m unittest discover -s tests -p 'test_*.py'
python3 scripts/security_check.py
python3 scripts/audit_license_inventory.py
python3 scripts/generate_rust_credits.py --check
```

The scanner requires Python 3.11+ and Gitleaks 8.30.1. Dependency checks require network access. Model downloads are excluded from default Rust tests; explicitly run `cargo test --locked --features model-integration --lib embedding::tests` for real model coverage. Browser security checks run with `cargo test --locked --features e2e --test browser_security`. The broader browser/semantic suite is `cargo test --locked --features e2e --test e2e`; it needs Chrome and may retrieve a driver, Chroma and embedding assets.

Independent test binaries and the Node/Python checks can run in parallel. Build Rust targets once before running their binaries concurrently to avoid Cargo artifact-lock contention. Keep separate logs and inspect every exit code. For a browser-only check without retaining semantic data, set `SELENIUM_MCP_DISABLE_MEMORY=1`; do not set it for semantic-memory coverage.

Contributions are accepted under Apache-2.0. Preserve the license and applicable copyright/attribution notices, including the D. Taylor Parsons III attribution in [NOTICE](NOTICE). Do not add a separate attribution restriction or claim that third-party binaries/models are relicensed by this repository.

## Third-party credits

Keep [THIRD_PARTY_CREDITS.md](THIRD_PARTY_CREDITS.md) accurate when adding major
external tools, models or databases. After changing `Cargo.toml` or
`Cargo.lock`, regenerate the full crate attribution report:

```sh
python3 scripts/generate_rust_credits.py --output THIRD_PARTY_RUST.md
```

Review any missing license metadata and the license terms of external binaries
or downloaded models before distributing a release.

The license inventory audit fails on new or unreviewed license identifiers and
missing or outdated crate entries. See [LICENSE_COMPLIANCE.md](LICENSE_COMPLIANCE.md)
for the additional obligations that apply to binary, browser, model or FFmpeg
redistribution. The audit is not a substitute for collecting required notices.
