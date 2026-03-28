#!/usr/bin/env bash
set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}==> Building twitch-tui in release mode...${NC}"
cargo build --release

BINARY="target/release/twt"
INSTALL_DIR="${HOME}/.cargo/bin"

if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    exit 1
fi

echo -e "${BLUE}==> Installing to ${INSTALL_DIR}...${NC}"
mkdir -p "$INSTALL_DIR"
cp "$BINARY" "$INSTALL_DIR/twt"

# Make sure it's executable
chmod +x "$INSTALL_DIR/twt"

# Get version
VERSION=$("$INSTALL_DIR/twt" --version 2>/dev/null || echo "version unknown")

echo -e "${GREEN}==> Successfully installed twt${NC}"
echo -e "${GREEN}    ${VERSION}${NC}"
echo -e "${GREEN}    Location: ${INSTALL_DIR}/twt${NC}"

# Check if ~/.cargo/bin is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo -e "${YELLOW}Warning: ${INSTALL_DIR} is not in your PATH${NC}"
    echo -e "${YELLOW}Add the following to your ~/.bashrc or ~/.zshrc:${NC}"
    echo -e "    export PATH=\"\$HOME/.cargo/bin:\$PATH\""
fi

echo ""
echo -e "${BLUE}Run 'twt' to start the application${NC}"
