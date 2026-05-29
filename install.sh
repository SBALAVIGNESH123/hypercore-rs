#!/bin/bash
set -e

# HYPERCORE v1 Installer
echo "Installing HYPERCORE..."

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        if [ "$ARCH" = "x86_64" ]; then
            ASSET="hypercore-linux-amd64"
        else
            echo "Unsupported architecture: $ARCH"
            exit 1
        fi
        ;;
    Darwin)
        if [ "$ARCH" = "x86_64" ]; then
            ASSET="hypercore-darwin-amd64"
        elif [ "$ARCH" = "arm64" ]; then
            ASSET="hypercore-darwin-arm64"
        else
            echo "Unsupported architecture: $ARCH"
            exit 1
        fi
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

echo "Fetching latest release for $ASSET..."

# In a real release, this would fetch from GitHub API
# LATEST_URL=$(curl -s https://api.github.com/repos/youruser/hypercore/releases/latest | grep "browser_download_url.*$ASSET" | cut -d '"' -f 4)
LATEST_URL="https://github.com/hypercore-ai/hypercore/releases/latest/download/$ASSET"

echo "Downloading from $LATEST_URL..."
curl -sSL -o /usr/local/bin/hypercore "$LATEST_URL"
chmod +x /usr/local/bin/hypercore

echo ""
echo "======================================"
echo "HYPERCORE installed successfully!"
echo "Run 'hypercore --help' to get started."
echo "======================================"
