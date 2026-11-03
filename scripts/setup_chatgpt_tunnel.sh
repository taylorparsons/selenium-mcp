#!/usr/bin/env bash
set -euo pipefail

PROFILE="${TUNNEL_PROFILE:-selenium-mcp}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

if [[ -z "${CONTROL_PLANE_TUNNEL_ID:-}" ]]; then
  die "CONTROL_PLANE_TUNNEL_ID is not set. Export your OpenAI tunnel ID first."
fi

if [[ -z "${CONTROL_PLANE_API_KEY:-}" ]]; then
  die "CONTROL_PLANE_API_KEY is not set. Export your OpenAI runtime API key first."
fi

if [[ "$(uname -s)" == "Darwin" ]]; then
  [[ -d "/Applications/Google Chrome.app" ]] || die "Google Chrome is not installed at /Applications/Google Chrome.app."
elif ! command -v google-chrome >/dev/null 2>&1 && ! command -v chromium >/dev/null 2>&1; then
  die "Google Chrome or Chromium was not found on PATH."
fi

TUNNEL_CLIENT="${TUNNEL_CLIENT_BIN:-}"
if [[ -z "$TUNNEL_CLIENT" ]]; then
  TUNNEL_CLIENT="$(command -v tunnel-client 2>/dev/null || true)"
fi
[[ -n "$TUNNEL_CLIENT" ]] || die "tunnel-client is not installed. Download it from https://github.com/openai/tunnel-client/releases and set TUNNEL_CLIENT_BIN if needed."

MCP_COMMAND="${SELENIUM_MCP_BIN:-}"
if [[ -z "$MCP_COMMAND" ]]; then
  MCP_COMMAND="$(command -v selenium-mcp 2>/dev/null || true)"
fi
if [[ -z "$MCP_COMMAND" && -x "$HOME/.cargo/bin/selenium-mcp" ]]; then
  MCP_COMMAND="$HOME/.cargo/bin/selenium-mcp"
fi
if [[ -z "$MCP_COMMAND" ]]; then
  echo "selenium-mcp is not installed; building the release binary..."
  cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"
  MCP_COMMAND="$REPO_ROOT/target/release/selenium-mcp"
fi
[[ -x "$MCP_COMMAND" ]] || die "selenium-mcp binary is not executable: $MCP_COMMAND"

echo "Using Chrome on $(uname -s)"
echo "Using MCP server: $MCP_COMMAND"
echo "Using tunnel profile: $PROFILE"

cd "$REPO_ROOT"

"$TUNNEL_CLIENT" init \
  --sample sample_mcp_stdio_local \
  --profile "$PROFILE" \
  --tunnel-id "$CONTROL_PLANE_TUNNEL_ID" \
  --mcp-command "$MCP_COMMAND"

"$TUNNEL_CLIENT" doctor --profile "$PROFILE" --explain

echo "Starting Secure MCP Tunnel. Leave this terminal running while ChatGPT uses the app."
exec "$TUNNEL_CLIENT" run --profile "$PROFILE"
