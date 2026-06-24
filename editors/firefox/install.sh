#!/usr/bin/env bash
# Build webspec-index and install the native messaging host manifest for Firefox.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "Building webspec-index..."
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

BINARY="$REPO_ROOT/target/release/webspec-index"

case "$(uname)" in
  Linux)
    HOST_DIR="$HOME/.mozilla/native-messaging-hosts"
    ;;
  Darwin)
    HOST_DIR="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
    ;;
  *)
    echo "ERROR: unsupported OS: $(uname)" >&2
    exit 1
    ;;
esac

mkdir -p "$HOST_DIR"

WRAPPER="$HOST_DIR/webspec-index-native-messaging"
cat > "$WRAPPER" <<EOF
#!/bin/sh
exec "$BINARY" native-messaging
EOF
chmod +x "$WRAPPER"

cat > "$HOST_DIR/webspec_index.json" <<EOF
{
  "name": "webspec_index",
  "description": "webspec-index native messaging host for webspec-lens",
  "path": "$WRAPPER",
  "type": "stdio",
  "allowed_extensions": ["webspec-lens@mozilla.org"]
}
EOF

echo "Binary:   $BINARY"
echo "Wrapper:  $WRAPPER"
echo "Manifest: $HOST_DIR/webspec_index.json"
