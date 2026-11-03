# Security policy

Version 0.1 is under active development. Security fixes target the latest main branch; older commits are not maintained separately.

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/taylorparsons/selenium-mcp/security/advisories/new) when available. If GitHub does not offer private reporting, open an issue requesting a private contact channel without including exploit details, credentials, private URLs, screenshots or session data. Do not publish an exploit or secret in a normal issue or pull request.

Include the affected commit, operating system, reproduction using synthetic data, and expected versus actual behavior. Never send a real browser profile or credential file.

## Trust and storage

This server grants its MCP client the ability to control a browser and use its authenticated sessions. Run it only for trusted clients and trusted requests. A project namespace is an organizational boundary, not an operating-system sandbox. A compromised local account can replace files while the server runs; the managed-path checks do not defend against a malicious operating-system administrator or concurrent filesystem replacement by the same account.

All `.selenium-mcp/` data is private by default, including flows, baselines, profiles, recordings and reports. The application writes a runtime-level `.gitignore`, and this repository ignores the directory at its root. These protections do not encrypt data or untrack files that were already committed. Use sanitized examples under `fixtures/` for public sharing. Inspect `git ls-files .selenium-mcp` before publication and remove any tracked personal assets from the index deliberately.

Persistent Chrome profiles contain authentication cookies and other website state. Use distinct named profiles for testing. `profile_clear` deletes one named profile; otherwise profiles persist until you delete them. Screenshots, recordings and baselines can show account information. Secret typing does not redact pixels from captures. Review or delete these artifacts before sharing, and use synthetic accounts for public demonstrations.

Typing with `env_var` reads a process environment variable and records only its reference. Password fields and sensitive autocomplete fields are detected automatically. Typing a sensitive literal remains possible for a one-time login, but disables saving that session as a flow. Start a fresh session and use `env_var` to save a replayable flow. Secret YAML steps must use declared `{env.NAME}` references. Driver details from typing failures are omitted to avoid echoing entered values.

Page indexing excludes form values and hidden content, but visible labels, selectors, URLs and run summaries can still be private. Set `SELENIUM_MCP_DISABLE_MEMORY=1` to disable indexing, semantic queries and run-memory storage. Existing data is not deleted by that setting. Local Chroma data lives under `~/.selenium-mcp/chroma/`; stop the server and any managed Chroma process before deliberately deleting that directory. `CHROMA_URL` points at an externally managed service and may transfer documents and metadata off the machine; secure and manage retention for that service separately.

## Downloads and dependencies

ChromeDriver is managed by thirtyfour using vendor release metadata and HTTPS. Install a trusted compatible driver on PATH if your environment requires independently verified binaries. Chroma's `uvx` fallback is pinned to `chromadb==1.5.9`; a `chroma` already on PATH is managed by its installer. FastEmbed downloads model/runtime assets from its upstream sources on first real use. Pin and verify these dependencies according to your environment's policy; Cargo's lockfile does not pin every external model asset.

FFmpeg on PATH or an explicitly provisioned local cache is preferred. Automatic FFmpeg archive retrieval requires a trusted HTTPS `SELENIUM_MCP_FFMPEG_URL` and matching `SELENIUM_MCP_FFMPEG_SHA256`. The archive is verified before extraction. Third-party tools and models keep their own licenses; Apache-2.0 covers this project's code, not a blanket relicensing of downloaded assets.

The security workflow checks the complete Git history and current publication candidates for secrets, checks locked Rust dependencies against OSV, and runs tests. Maintenance-only advisories are warnings; vulnerability matches or incomplete scans fail. GitHub branch protection, private reporting and push protection are separate hosting settings and must be configured before opening public contributions.

The current dependency graph retains `paste` through the tokenizer dependency. Its advisory is an unmaintained-project notice, with no vulnerability identified in that record. Track replacement upstream; do not suppress future vulnerability records for the package.

## If a secret was committed

Revoke or rotate it first. Removing a file or adding `.gitignore` does not revoke a credential and does not remove historical copies. Coordinate any history cleanup separately; this project never automatically rewrites your Git history or deletes personal files.
