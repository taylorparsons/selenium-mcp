# selenium-mcp

An MCP server that drives a real browser like a human tester: navigate, click, type,
verify, and capture PNG/MP4/GIF evidence. Semantic memory (Chroma + in-process
embeddings) lets it find elements by description and self-heal selectors that break
after a UI change. It installs once and works from any project directory.

## Requirements

- Rust toolchain (to build) — https://rustup.rs
- Google Chrome or Chromium installed. `chromedriver` is downloaded automatically on
  first use if it isn't already on `PATH` (via thirtyfour's managed driver support).
- Optional, for semantic memory (`find_semantic`, self-healing, `knowledge_search`,
  `run_history`, `memory_stats`): a `chroma` CLI on `PATH`, or `uvx` (from
  [uv](https://docs.astral.sh/uv/)) so one can be launched on demand
  (`uvx --from chromadb chroma`). Without either, semantic tools degrade with an
  explicit error; every selector-based tool (`click`, `type_text`, `assert_that`,
  flows without `semantic` targets, `selenium-mcp test`) keeps working.
- Optional, for `record_start`/`record_stop`: `ffmpeg` on `PATH` or a trusted binary
  provisioned in `~/.selenium-mcp/bin/`. Archive downloads require an explicit
  HTTPS URL and SHA-256; see the security section below.

## Install

### Quick setup for macOS, Debian/Ubuntu, and WSL

From the repository root, run:

```sh
./scripts/install.sh
```

The script installs the Rust toolchain and required build packages, ensures
Google Chrome or Chromium is available, then builds and installs `selenium-mcp`
to `~/.cargo/bin`. It supports macOS (the project is tested on macOS) and
Debian/Ubuntu Linux, including Ubuntu in WSL. Native Windows PowerShell setup is
not provided. In WSL, install and run the Linux browser inside WSL; interactive
browser windows require WSLg, while headless flows do not.

On macOS, install Apple Command Line Tools first with `xcode-select --install`.
Homebrew is needed only if the script must install Chrome. On Debian/Ubuntu, the
script uses `apt` and `sudo` (or root); on amd64 it installs Chrome from Google's
signed apt repository, and on arm64 it uses a Chromium package when the distro
provides one. Unsupported Linux distributions are rejected before package
changes.

Preview actions without changing the system with `./scripts/install.sh --dry-run`.
If Claude Code CLI is available, setup asks before adding a user-level
MCP entry. Decline the prompt or pass `--no-claude` to skip registration. The
repository-local `.claude/skills/selenium-mcp/SKILL.md` is available to Claude
Code when this repository is open.

Semantic memory and recording remain optional. Install `chroma` or `uvx` for
semantic tools and `ffmpeg` for MP4/GIF recording; their absence does not block
selector-based browser automation.

### Manual build

To build without the setup script:

```
cargo install --path .
```

This builds the `selenium-mcp` binary and installs it to `~/.cargo/bin` (make sure
that's on your `PATH`). Alternatively, `cargo build --release` and copy
`target/release/selenium-mcp` wherever you like.

## Register with an MCP client

The setup script can register Claude Code after you confirm its prompt. You can
also register it manually (adjust the binary path if needed):

```sh
claude mcp add --transport stdio --scope user selenium-mcp -- "$HOME/.cargo/bin/selenium-mcp"
```

For another MCP client, add it as a stdio server. For example, in a client's
`mcpServers` config:

```json
{
  "mcpServers": {
    "selenium-mcp": {
      "command": "selenium-mcp"
    }
  }
}
```

No arguments, no per-project setup. Every tool call resolves the current project root
(the nearest `.git` toplevel, or the client's working directory if none) and creates
`.selenium-mcp/{flows,artifacts,baselines,profiles}/` there on first use. The entire
directory is private and gitignored by default: flows and baseline images may
contain personal information just as recordings and profiles do. Put explicitly
sanitized examples under `fixtures/` when you want to share them.
Two different projects on the same machine never share Chroma collections or
artifacts; each project gets its own namespace derived from its resolved root path.

## Tools

**Session & interaction:** `session_start`, `session_end`, `navigate`, `click`,
`type_text`, `assert_that`, `screenshot`, `profile_clear`.

**Flows & regression suite:** `flow_save`, `flow_list`, `flow_delete`, `flow_run`,
`flow_run_all`.

**Media:** `record_start`, `record_stop` (MP4 and/or animated GIF).

**Semantic memory & self-healing:** `find_semantic`, `run_history`,
`knowledge_search`, `memory_stats`.

**Visual regression:** `baseline_save`, `assert_visual`.

Every tool returns structured content — passes and failures are both regular results
with expected/actual detail, not exceptions; only truly malformed requests are
protocol-level errors.

## CI without an AI client

```
selenium-mcp test [--tag <tag>] [--flow <name>] [--json <path>]
                   [--heal-write-back] [--update-baselines]
```

Runs every flow in `<project>/.selenium-mcp/flows/` (optionally filtered) in one
headless browser session. A failing flow never stops the suite. Output is a console
PASS/FAIL summary plus `report.json` — written to `<project>/.selenium-mcp/artifacts/
report.json` by default, or to `--json <path>` if given. This happens even when
Chroma is unreachable or down; only `find_semantic`/self-healing/the knowledge base
degrade, never the report itself.

| Exit code | Meaning |
|---|---|
| 0 | Every flow passed |
| 1 | At least one flow failed (assertion, visual diff, unhealed selector) |
| 2 | Infrastructure error — browser/driver unavailable, malformed flow file |

`--heal-write-back` writes a step's healed selector back into the flow file when a
concrete selector breaks but semantic lookup finds it; default is report-only.
`--update-baselines` re-captures every `assert_visual` step's baseline instead of
diffing against it — the normal workflow right after an intentional UI change.

## Example flow

```yaml
name: login-happy-path
description: Log in and verify the dashboard loads
tags: [smoke]
base_url: http://localhost:3000
env: [TEST_EMAIL, TEST_PASSWORD]
steps:
  - action: navigate
    url: "{base_url}/login"
  - action: type_text
    target: { css: "#email", semantic: "email input field" }
    text: "{env.TEST_EMAIL}"
  - action: type_text
    target: { css: "#password" }
    text: "{env.TEST_PASSWORD}"
    secret: true
  - action: click
    target: { css: "button[type=submit]", semantic: "the sign in button" }
  - action: assert_that
    that: { text_visible: "Welcome" }
  - action: assert_visual
    baseline: dashboard-after-login
    threshold: 0.001
    masks:
      - { x: 900, y: 12, w: 120, h: 24 } # clock in the header
```

`{env.NAME}` interpolation keeps secrets out of flow files; secret steps must use
a single declared environment reference. Typing reports omit entered values, and
page indexing never reads form-control values. Captured images/video can still
show private content. `target.semantic` is what
self-healing re-resolves against if the concrete `css`/`xpath` breaks — provide it when recording a target or edit it into the saved YAML. `flow_save`
preserves the supplied targets; it does not generate semantic descriptions. `assert_visual` steps
always run at a fixed 1280x800 viewport unless the flow sets its own
`viewport_width`/`viewport_height`, so a baseline captured once stays comparable on
every future run.

## Persistent profiles

`session_start` takes an optional `profile: <name>`; a saved `Flow` accepts the same
field (set by hand-editing the flow's YAML, same as `viewport_width`/`viewport_height`
— it's not a `flow_save` parameter). When set, Chrome launches with
`--user-data-dir` pointed at a directory named after the profile, so cookies,
localStorage, and IndexedDB persist across separate `session_start` calls instead of
starting from a blank browser every time. Omitting `profile` keeps today's ephemeral
behavior exactly.

This means a login only has to happen once:

```
session_start({ profile: "indeed", headless: false })
```

Drive the real login form with the existing flow steps — `type_text` into the
email/password fields, then `click` the submit button. For a replayable login, use
`type_text({ target: { css: "#password" }, env_var: "TEST_PASSWORD", secret: true })`;
the variable must be present in the MCP server process's environment. Only its
reference is recorded, and `flow_save` declares it automatically. For a one-time
login, `secret: true` with literal `text` types the value without recording it and
blocks saving that session as an incomplete flow. Password fields are detected
even when `secret` is omitted. Use `headless: false` for
this first run: some sites present a CAPTCHA, 2FA, or headless-detection challenge
that this tool cannot automate, and a human needs to be able to see and complete it.
Once that session is authenticated, every later `session_start({ profile: "indeed" })`
— including headless runs in CI — starts already logged in, because the same Chrome
profile directory is reused.

If a persisted session goes stale (logged out remotely, expired cookies, forced
re-auth), call `profile_clear("indeed")` to delete the profile directory and start
clean. It succeeds whether or not the profile existed, with a distinct message for
each case.

Profiles live at `<project>/.selenium-mcp/profiles/<name>/` and that directory is
unconditionally gitignored — a profile can contain live session cookies, so it's
never committed. Flows and baselines are private by default too. Managed names are restricted to
`[a-zA-Z0-9_-]+` (letters, digits, underscore, hyphen); anything else is rejected
before it touches the filesystem.

## Security and personal use

The MCP client controls a real browser, including any authenticated profile you
select. Use trusted clients and synthetic accounts for shared demonstrations.
Keep personal credentials in your local environment or ignored `.env` variants;
this program does not automatically load `.env` files. `.gitignore` prevents
accidental additions but does not remove files already tracked by Git.

Set `SELENIUM_MCP_DISABLE_MEMORY=1` to disable semantic indexing, search and run
storage. Otherwise safe visible labels, selectors and run summaries are stored
locally in Chroma, or sent to your configured `CHROMA_URL`. This setting does not
delete existing memory. See [SECURITY.md](SECURITY.md) for data locations, deletion,
capture privacy, external downloads, and vulnerability reporting.

FFmpeg already on PATH or in the local cache is used first. To permit an archive
download, set both `SELENIUM_MCP_FFMPEG_URL` to a trusted HTTPS archive and
`SELENIUM_MCP_FFMPEG_SHA256` to its independently verified SHA-256. Unverified
FFmpeg archives are never unpacked.

## Open-source credits

This project uses and integrates code and tools maintained by other open-source
developers. See [Third-party credits](THIRD_PARTY_CREDITS.md) for the project and
maintainer acknowledgments, with upstream links and licenses. The
[resolved Rust dependency credits](THIRD_PARTY_RUST.md) list direct and
transitive crates, their published authors, licenses, and source repositories.

Read the [license compliance review](LICENSE_COMPLIANCE.md) before distributing
compiled executables, containers, bundled models or third-party binaries.

To update the Rust inventory after a dependency change:

```sh
python3 scripts/generate_rust_credits.py --output THIRD_PARTY_RUST.md
```

## License and attribution

Copyright 2026 D. Taylor Parsons III. Created by **D. Taylor Parsons III**.
Free to use, modify and distribute under the
[Apache License, Version 2.0](LICENSE), including commercial use. When
redistributing, preserve the required license and applicable notices, including
the author attribution in [NOTICE](NOTICE), as specified by Apache-2.0.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the verification commands.

## Memory footprint

Idle RSS stays under 30 MB — the embedding model, Chroma sidecar, and browser are
all lazy: nothing heavy is resident until a tool actually needs it, and Chroma shuts
itself down after 10 minutes of no calls (never when `CHROMA_URL` points at an
externally-managed instance).

## Known v1 gaps

- Re-indexing after `click`/`type_text` isn't debounced — only `navigate` re-indexes
  a page's elements. Fine for the common case; a UI that mutates itself heavily after
  an in-page interaction (no navigation) won't be re-indexed until the next
  `navigate`.
- Visual regression masks are raw pixel rectangles, not tied to the semantic element
  index — masking "the clock" means naming its coordinates, not its selector.
- No `wait_for` flow step and no per-project `config.toml` defaults
  (base_url/browser/viewport) yet — set `base_url` per flow and pass
  `viewport_width`/`viewport_height` per flow if you need something other than the
  1280x800 default.
- No automatic pruning of persistent profile directories — cache and service worker
  storage grow unbounded over time. `profile_clear` is the only reset, and it's
  manual; there's no size- or age-based cleanup.
