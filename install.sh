#!/usr/bin/env bash
set -euo pipefail

REPO="4lador/leantime-mcp"
BINARY_NAME="leantmcp"
INSTALL_DIR="${HOME}/.local/bin"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; NC='\033[0m'

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
  local os arch version url dest tmp_dest tmp_sha

  os=$(detect_os)
  arch=$(detect_arch)

  info "Detecting latest version..."
  version=$(get_latest_version)
  [ -z "$version" ] && error "Could not determine latest version"
  info "Latest version: ${version}"

  local artifact_name="${BINARY_NAME}-${os}-${arch}"
  url="https://github.com/${REPO}/releases/download/${version}/${artifact_name}"
  dest="${INSTALL_DIR}/${BINARY_NAME}"

  info "Downloading ${url}..."
  mkdir -p "$INSTALL_DIR"
  tmp_dest="${INSTALL_DIR}/.leantmcp.tmp"
  if ! curl -fsSL "$url" -o "$tmp_dest"; then
    rm -f "$tmp_dest"
    error "Download failed"
  fi

  # Integrity: verify the published SHA-256
  tmp_sha="${INSTALL_DIR}/.leantmcp.sha256.tmp"
  if ! curl -fsSL "${url}.sha256" -o "$tmp_sha"; then
    rm -f "$tmp_dest" "$tmp_sha"
    error "Could not download the checksum"
  fi
  local expected actual
  expected="$(awk '{print $1}' "$tmp_sha")"
  rm -f "$tmp_sha"
  [ -z "$expected" ] && { rm -f "$tmp_dest"; error "Checksum file is empty"; }
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp_dest" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp_dest" | awk '{print $1}')"
  else
    rm -f "$tmp_dest"; error "Neither sha256sum nor shasum available"
  fi
  [ "$actual" != "$expected" ] && {
    rm -f "$tmp_dest"
    error "Checksum mismatch (expected $expected, got $actual)"
  }

  mv -f "$tmp_dest" "$dest"
  chmod +x "$dest"

  info "Installed ${BINARY_NAME} ${version} to ${dest}"

  if ! echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
    warn "Add ${INSTALL_DIR} to your PATH:"
    echo ""
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
  fi

  info "Run '${BINARY_NAME} setup global' to configure for opencode"
}

main "$@"
