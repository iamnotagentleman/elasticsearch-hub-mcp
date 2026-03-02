#!/bin/sh
# Elasticsearch Hub MCP — Installer
# Works when piped: curl ... | sh

REPO="https://github.com/iamnotagentleman/elasticsearch-hub-mcp.git"
INSTALL_DIR="${ES_HUB_DIR:-$HOME/.elasticsearch-hub-mcp}"

echo "==> Elasticsearch Hub MCP — Installer"
echo ""

# 1. Install uv if missing
if ! command -v uv >/dev/null 2>&1; then
  echo "==> Installing uv..."
  curl -LsSf https://astral.sh/uv/install.sh | sh

  export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

  if ! command -v uv >/dev/null 2>&1; then
    echo "ERROR: uv installed but not found in PATH."
    echo "Restart your terminal and run this script again."
    exit 1
  fi
fi

echo "==> uv $(uv --version)"

# 2. Install Python 3.13 if missing
if ! uv python find 3.13 >/dev/null 2>&1; then
  echo "==> Installing Python 3.13 via uv..."
  if ! uv python install 3.13 2>&1; then
    echo "==> Retrying with --native-tls..."
    if ! uv python install --native-tls 3.13; then
      echo "ERROR: Python install failed."
      exit 1
    fi
  fi
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
cd "$INSTALL_DIR" || exit 1
if ! uv sync 2>&1; then
  echo "==> Retrying with --native-tls..."
  if ! uv sync --native-tls; then
    echo "ERROR: uv sync failed."
    exit 1
  fi
fi

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
