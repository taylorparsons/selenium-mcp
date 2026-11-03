#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRY_RUN=0
SKIP_CLAUDE=0
GOOGLE_LINUX_KEY_FINGERPRINT="EB4C1BFD4F042F6DDDCCEC917721F63BD38B4796"
TEMP_DIR=""

cleanup() {
  if [[ -n "$TEMP_DIR" ]]; then
    rm -rf -- "$TEMP_DIR"
  fi
}
trap cleanup EXIT

usage() {
  cat <<'EOF'
Usage: scripts/install.sh [--dry-run] [--no-claude] [--help]

Install Rust and Chrome/Chromium prerequisites, then build/install selenium-mcp.
Supported systems: macOS and Debian/Ubuntu Linux, including WSL.

--dry-run  Show planned commands without installing or changing files.
--no-claude  Do not offer Claude Code MCP registration.
--help     Show this message.
EOF
}

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

plan() {
  printf 'PLAN: '
  printf '%q ' "$@"
  printf '\n'
}

run() {
  if (( DRY_RUN )); then
    plan "$@"
  else
    "$@"
  fi
}

as_root() {
  if (( EUID == 0 )); then
    run "$@"
  elif command -v sudo >/dev/null 2>&1; then
    run sudo "$@"
  else
    die "sudo is required to install Linux system packages. Run as root or install sudo."
  fi
}

has_browser() {
  command -v google-chrome >/dev/null 2>&1 ||
    command -v google-chrome-stable >/dev/null 2>&1 ||
    command -v chromium >/dev/null 2>&1 ||
    command -v chromium-browser >/dev/null 2>&1 ||
    [[ -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]]
}

install_chrome_linux() {
  local architecture key_ascii key_binary fingerprint
  architecture="$(dpkg --print-architecture)"

  if [[ "$architecture" == "amd64" ]]; then
    if (( DRY_RUN )); then
      plan "download and verify Google's Linux signing key from https://dl.google.com/linux/linux_signing_key.pub"
      plan "install the signed-by Google Chrome apt source"
      as_root apt-get update
      as_root apt-get install -y google-chrome-stable
      return
    fi

    TEMP_DIR="$(mktemp -d)"
    key_ascii="$TEMP_DIR/google-linux-signing-key.asc"
    key_binary="$TEMP_DIR/google-linux-signing-key.gpg"
    curl --proto '=https' --tlsv1.2 -fsSL \
      https://dl.google.com/linux/linux_signing_key.pub -o "$key_ascii"
    fingerprint="$(gpg --show-keys --with-colons "$key_ascii" |
      awk -F: '$1 == "fpr" { print $10; exit }')"
    [[ "$fingerprint" == "$GOOGLE_LINUX_KEY_FINGERPRINT" ]] ||
      die "Google Linux signing-key fingerprint did not match the expected value."
    gpg --dearmor --output "$key_binary" "$key_ascii"
    as_root install -d -m 0755 /etc/apt/keyrings
    as_root install -m 0644 "$key_binary" /etc/apt/keyrings/google-chrome.gpg
    printf '%s\n' \
      'deb [arch=amd64 signed-by=/etc/apt/keyrings/google-chrome.gpg] https://dl.google.com/linux/chrome/deb/ stable main' |
      as_root tee /etc/apt/sources.list.d/google-chrome.list >/dev/null
    as_root apt-get update
    as_root apt-get install -y google-chrome-stable
    return
  fi

  if [[ "$architecture" == "arm64" ]]; then
    local chromium_package="" candidate
    candidate="$(apt-cache policy chromium 2>/dev/null | awk '/Candidate:/ { print $2 }')"
    if [[ -n "$candidate" && "$candidate" != "(none)" ]]; then
      chromium_package=chromium
    else
      candidate="$(apt-cache policy chromium-browser 2>/dev/null | awk '/Candidate:/ { print $2 }')"
      if [[ -n "$candidate" && "$candidate" != "(none)" ]]; then
        chromium_package=chromium-browser
      fi
    fi
    if [[ -n "$chromium_package" ]]; then
      if (( DRY_RUN )); then
        plan "install an ARM64 Chromium package from apt: $chromium_package"
      else
        as_root apt-get install -y "$chromium_package"
      fi
      return
    fi
  fi

  die "No supported Chrome/Chromium package is available for Linux architecture '$architecture'. Install Chrome or Chromium manually, then rerun this script."
}

install_rust() {
  if command -v rustup >/dev/null 2>&1; then
    printf 'Rustup found.\n'
  else
    if (( DRY_RUN )); then
      plan "install rustup from https://sh.rustup.rs (minimal profile)"
    else
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
        sh -s -- -y --profile minimal
    fi
  fi

  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
  fi

  if command -v rustup >/dev/null 2>&1; then
    run rustup toolchain install stable
  elif (( ! DRY_RUN )); then
    die "Rustup installation finished, but rustup is not on PATH. Open a new terminal and rerun this script."
  fi
}

install_macos_prerequisites() {
  if ! command -v xcode-select >/dev/null 2>&1 || ! xcode-select -p >/dev/null 2>&1; then
    die "Apple Command Line Tools are required. Install them with 'xcode-select --install', then rerun this script."
  fi

  if ! has_browser; then
    command -v brew >/dev/null 2>&1 ||
      die "Chrome/Chromium is missing and Homebrew is unavailable. Install Google Chrome or install Homebrew, then rerun this script."
    run brew install --cask google-chrome
  fi
}

install_linux_prerequisites() {
  local distro
  # shellcheck disable=SC1091
  source /etc/os-release
  distro="${ID:-unknown}"
  case "$distro" in
    debian|ubuntu) ;;
    *) die "Unsupported Linux distribution '$distro'. This installer supports Debian/Ubuntu, including WSL." ;;
  esac

  command -v apt-get >/dev/null 2>&1 || die "apt-get is required on Debian/Ubuntu."
  as_root apt-get update
  as_root apt-get install -y ca-certificates curl gnupg build-essential

  if ! has_browser; then
    install_chrome_linux
  fi
}

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --no-claude) SKIP_CLAUDE=1 ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; die "Unknown option: $arg" ;;
  esac
done

OS="$(uname -s)"
case "$OS" in
  Darwin) install_macos_prerequisites ;;
  Linux) install_linux_prerequisites ;;
  *) die "Unsupported operating system '$OS'. This installer supports macOS and Debian/Ubuntu Linux, including WSL." ;;
esac

if ! has_browser && (( ! DRY_RUN )); then
  die "Chrome/Chromium was not found after prerequisite setup. Install a supported browser and rerun this script."
fi

install_rust

if (( DRY_RUN )); then
  plan cargo +stable install --locked --path "$REPO_ROOT" --force
  printf 'Dry run complete; no files or system packages were changed.\n'
else
  cargo +stable install --locked --path "$REPO_ROOT" --force
  printf '\nInstalled selenium-mcp. Confirm %s/.cargo/bin is on PATH for your MCP client.\n' "$HOME"
fi

if (( ! SKIP_CLAUDE )); then
  MCP_COMMAND="$HOME/.cargo/bin/selenium-mcp"
  if command -v claude >/dev/null 2>&1; then
    if (( DRY_RUN )); then
      plan claude mcp add --transport stdio --scope user selenium-mcp -- "$MCP_COMMAND"
    elif [[ -t 0 ]]; then
      printf 'Register selenium-mcp in your user-level Claude Code settings? [y/N] '
      read -r answer || answer=""
      case "$answer" in
        [Yy]|[Yy][Ee][Ss])
          claude mcp add --transport stdio --scope user selenium-mcp -- "$MCP_COMMAND"
          ;;
        *) printf 'Skipped Claude Code registration.\n' ;;
      esac
    else
      printf 'To register later, run: claude mcp add --transport stdio --scope user selenium-mcp -- %q\n' "$MCP_COMMAND"
    fi
  else
    printf 'Claude Code CLI not found. When installed, register with:\n'
    printf '  claude mcp add --transport stdio --scope user selenium-mcp -- %q\n' "$MCP_COMMAND"
  fi
fi
