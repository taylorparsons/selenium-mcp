---
name: selenium-mcp
description: Use the selenium-mcp browser tools from Claude Code to inspect websites, run UI checks, capture evidence, and create or replay browser flows. Trigger when asked to test a web UI, automate browser interactions, investigate a browser-visible bug, or run selenium-mcp flows.
---

# Use selenium-mcp

Use the registered `selenium-mcp` MCP server to drive Google Chrome for browser-visible checks. Use the MCP tools exposed by Claude Code; do not assume another browser or remote Selenium service is configured.

## Choose a workflow

- For a one-off check, start a session, navigate to the requested URL, inspect the page, perform the requested interaction, assert the visible result, and end the session.
- For repeatable checks, save a YAML flow in the current project's `.selenium-mcp/flows/` and run it with `selenium-mcp test` or the MCP flow tools. Keep selectors and assertions focused on user-visible behavior.
- For screenshot comparisons, use `baseline_save` to establish a baseline and `assert_visual` to compare later runs. Update a baseline only when the user intends to accept a visual change.
- Use semantic lookup when Chroma is available. If semantic memory is unavailable, continue with CSS or XPath selectors and explain the limitation only if it affects the result.

## Handle accounts and captured data carefully

- Use `secret: true` and environment-variable references for credentials in replayable flows. Never save literal passwords or tokens in flow YAML.
- Screenshots, recordings, browser profiles, and page content can contain private information. Report their paths and avoid sharing them unless requested.
- Use a persistent `profile` only when the user wants a saved login. A profile stores browser session data under the current project's ignored `.selenium-mcp/profiles/` directory. `profile_clear` deletes that profile.
- Do not submit a consequential form, purchase, payment, or destructive action unless the user explicitly requested that action.

## Report results

State which URL or flow was checked, the observed outcome, and any relevant artifact path. For the CLI runner, interpret exit code `0` as all flows passing, `1` as one or more flow failures, and `2` as an infrastructure or malformed-flow error. Distinguish an unavailable browser/driver from an assertion failure.

## Set up the server

If the MCP tools are missing, do not claim a browser check ran. Point the user to `./scripts/install.sh` for macOS or Debian/Ubuntu Linux, including WSL. The installer may install system packages and build the binary; it asks before changing Claude Code's user-level MCP configuration. Run it only when setup is requested.
