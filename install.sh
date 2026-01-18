#!/bin/sh
# quickdiff installer
# Usage: curl -fsSL https://raw.githubusercontent.com/Yeshwanthyk/quickdiff/main/install.sh | sh

set -e

REPO="Yeshwanthyk/quickdiff"
INSTALL_DIR="${QUICKDIFF_INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64) TARGET="aarch64-apple-darwin" ;;
      x86_64) echo "Intel Mac not supported. Use: cargo install quickdiff"; exit 1 ;;
      *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) echo "Linux ARM not supported. Use: cargo install quickdiff"; exit 1 ;;
      *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "Unsupported OS: $OS"
    exit 1
    ;;
esac

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST" ]; then
  echo "Failed to fetch latest release"
  exit 1
fi

echo "Installing quickdiff $LATEST for $TARGET..."

# Download and extract
URL="https://github.com/$REPO/releases/download/$LATEST/quickdiff-$TARGET.tar.gz"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

curl -fsSL "$URL" | tar xz -C "$TMPDIR"

# Install
if [ -w "$INSTALL_DIR" ]; then
  mv "$TMPDIR/quickdiff" "$INSTALL_DIR/quickdiff"
else
  echo "Installing to $INSTALL_DIR (requires sudo)..."
  sudo mv "$TMPDIR/quickdiff" "$INSTALL_DIR/quickdiff"
fi

chmod +x "$INSTALL_DIR/quickdiff"

echo "Installed quickdiff to $INSTALL_DIR/quickdiff"
echo "Run 'quickdiff --help' to get started"
