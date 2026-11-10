# Changelog

All notable changes to this project are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versions follow [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-10

### Security
- Upgrade the `rmcp` MCP SDK from 3.8.0 to 3.9.0 to clear RUSTSEC-2026-0462 (unbounded response body buffering in Streamable HTTP clients). selenium-mcp does not enable the affected HTTP client features, so this was not exploitable here, but the weekly security audit failed on it.

### Changed
- Raise the minimum `rmcp` version in `Cargo.toml` from 3.5 to 3.9 so older, affected versions can't be resolved.

## [0.1.0]

- Versions before this changelog was started.
