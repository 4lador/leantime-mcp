#!/bin/bash
set -euo pipefail

REPO="4lador/leantime-mcp"
BINARY_NAME="leantmcp"
INSTALL_DIR="${HOME}/.local/bin"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}→${NC} $*"; }
warn()  { echo -e "${YELLOW}!${NC} $*"; }
error() { echo -e "${RED}✗${NC} $*" >&2; exit 1; }

detect_os() {
  case "$(uname -s)" in
    Linux*)  echo "linux" ;;
    Darwin*) echo "darwin" ;;
    *)       error "Unsupported OS: $(uname -s)" ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "x86_64" ;;
    aarch64|arm64) echo "aarch64" ;;
    *)             error "Unsupported architecture: $(uname -m)" ;;
  esac
}

get_latest_version() {
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/'
}

main() {
  local os arch version url dest

  os=$(detect_os)
  arch=$(detect_arch)

  info "Detecting latest version..."
  version=$(get_latest_version)
  if [ -z "$version" ]; then
    error "Could not determine latest version"
  fi
  info "Latest version: ${version}"

  local archive_name="${BINARY_NAME}-${os}-${arch}"
  if [ "$os" = "windows" ]; then
    archive_name="${archive_name}.exe"
  fi

  url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"
  dest="${INSTALL_DIR}/${BINARY_NAME}"

  info "Downloading ${url}..."
  mkdir -p "$INSTALL_DIR"
  # Download to a temp file first: writing directly over a running binary
  # fails with ETXTBSY (and on Windows the file is locked).
  local tmp_dest="${INSTALL_DIR}/.leantmcp.tmp"
  if ! curl -fsSL "$url" -o "$tmp_dest"; then
    rm -f "$tmp_dest"
    error "Download failed"
  fi
  mv -f "$tmp_dest" "$dest"
  chmod +x "$dest"

  info "Installed ${BINARY_NAME} v${version} to ${dest}"

  if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
    warn "Add ${INSTALL_DIR} to your PATH:"
    echo ""
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
  fi

  info "Run '${BINARY_NAME} setup global' to configure for opencode"
}

main "$@"
