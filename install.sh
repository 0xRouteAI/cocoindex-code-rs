#!/usr/bin/env bash

set -euo pipefail

REPO_SLUG="${REPO_SLUG:-0xRouteAI/cocoindex-code-rs}"
BIN_NAME="cocoindex-code-rs"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
VERSION="${VERSION:-latest}"
TEMP_DIR=""

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

detect_os() {
  case "$(uname -s)" in
    Linux) echo "linux" ;;
    Darwin) echo "macos" ;;
    *)
      echo "Unsupported OS: $(uname -s)" >&2
      exit 1
      ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "x86_64" ;;
    arm64|aarch64) echo "aarch64" ;;
    *)
      echo "Unsupported architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
}

resolve_version() {
  if [[ "$VERSION" != "latest" ]]; then
    echo "$VERSION"
    return
  fi

  curl -fsSL "https://api.github.com/repos/${REPO_SLUG}/releases/latest" \
    | grep '"tag_name":' \
    | head -n1 \
    | sed -E 's/.*"([^"]+)".*/\1/'
}

cleanup() {
  if [[ -n "${TEMP_DIR:-}" && -d "${TEMP_DIR}" ]]; then
    rm -rf -- "${TEMP_DIR}"
  fi
}

main() {
  need_cmd curl
  need_cmd tar
  need_cmd mktemp

  local os
  local arch
  local resolved_version
  local version_path
  local archive_name
  local download_url

  os="$(detect_os)"
  arch="$(detect_arch)"
  resolved_version="$(resolve_version)"

  if [[ -z "$resolved_version" ]]; then
    echo "Failed to resolve release version from GitHub." >&2
    exit 1
  fi

  version_path="$resolved_version"
  version_path="${version_path#refs/tags/}"
  archive_name="${BIN_NAME}-${os}-${arch}.tar.gz"
  download_url="https://github.com/${REPO_SLUG}/releases/download/${version_path}/${archive_name}"
  TEMP_DIR="$(mktemp -d)"

  trap cleanup EXIT

  echo "Installing ${BIN_NAME} ${resolved_version} for ${os}/${arch}"
  echo "Download URL: ${download_url}"

  curl -fL "$download_url" -o "${TEMP_DIR}/${archive_name}"
  tar -xzf "${TEMP_DIR}/${archive_name}" -C "$TEMP_DIR"

  mkdir -p "$INSTALL_DIR"
  install "${TEMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"

  echo
  echo "Installed to: ${INSTALL_DIR}/${BIN_NAME}"
  echo
  echo "If ${INSTALL_DIR} is not in PATH, add this to your shell profile:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
  echo
  echo "Verify:"
  echo "  ${BIN_NAME} --help"
}

main "$@"
