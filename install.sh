#!/usr/bin/env bash
set -euo pipefail

REPO="https://github.com/iamnotagentleman/elasticsearch-hub-mcp.git"
INSTALL_DIR="${ES_HUB_DIR:-$HOME/.elasticsearch-hub-mcp}"
echo "==> Elasticsearch Hub MCP — Installer"
echo ""

# 1. Install uv if missing
if ! command -v uv &>/dev/null; then
  echo "==> Installing uv..."
  curl -LsSf https://astral.sh/uv/install.sh | sh

  # Source uv into current shell
  export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

  if ! command -v uv &>/dev/null; then
    echo "ERROR: uv installed but not found in PATH."
    echo "Restart your terminal and run this script again."
    exit 1
  fi
fi

echo "==> uv $(uv --version)"

# 2. Install Python 3.13 if missing
if ! uv python find 3.13 &>/dev/null; then
  echo "==> Installing Python 3.13 via uv..."
  uv python install 3.13 || uv python install --native-tls 3.13
fi

echo "==> Python $(uv python find 3.13)"

# 3. Clone or update repo
if [ -d "$INSTALL_DIR" ]; then
  echo "==> Updating existing install at $INSTALL_DIR..."
  git -C "$INSTALL_DIR" pull --ff-only
else
  echo "==> Cloning to $INSTALL_DIR..."
  git clone "$REPO" "$INSTALL_DIR"
fi

# 4. Install dependencies
echo "==> Installing dependencies..."
cd "$INSTALL_DIR"
uv sync || uv sync --native-tls

# 5. Create config if missing
if [ ! -f "$INSTALL_DIR/config.json" ]; then
  cp "$INSTALL_DIR/config.example.json" "$INSTALL_DIR/config.json"
  echo ""
  echo "==> Created config.json from example. Edit it with your instances:"
  echo "    $INSTALL_DIR/config.json"
fi

echo ""
echo "==> Done! Add to your MCP client:"
echo ""
echo "    Claude Code:"
echo "      claude mcp add elasticsearch -- uv run --directory $INSTALL_DIR elasticsearch-hub-mcp"
echo ""
echo "    Claude Desktop / Cursor (JSON config):"
echo "      \"command\": \"uv\","
echo "      \"args\": [\"run\", \"--directory\", \"$INSTALL_DIR\", \"elasticsearch-hub-mcp\"]"
echo ""
