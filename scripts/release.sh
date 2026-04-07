#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <version>"
    echo "Example: $0 0.7.0"
    exit 1
}

[[ $# -ne 1 ]] && usage

VERSION="$1"

# Strip leading 'v' if provided
VERSION="${VERSION#v}"

# Validate semver-ish format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: '$VERSION' is not a valid version (expected X.Y.Z)"
    exit 1
fi

TAG="v$VERSION"
ROOT="$(git rev-parse --show-toplevel)"

# Check for uncommitted changes
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Error: working tree is dirty. Commit or stash changes first."
    exit 1
fi

echo "Bumping version to $VERSION ..."

# 1. Cargo.toml (main crate) — first version = line only
sed -i "0,/^version = /s/^version = .*/version = \"$VERSION\"/" "$ROOT/Cargo.toml"

# 2. editors/vscode/package.json
sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" "$ROOT/editors/vscode/package.json"

# 3. editors/zed/Cargo.toml — first version = line only
sed -i "0,/^version = /s/^version = .*/version = \"$VERSION\"/" "$ROOT/editors/zed/Cargo.toml"

# 4. editors/zed/extension.toml — first version = line only (skip [lib] version)
sed -i "0,/^version = /s/^version = .*/version = \"$VERSION\"/" "$ROOT/editors/zed/extension.toml"

echo "Updated:"
echo "  Cargo.toml              -> $VERSION"
echo "  editors/vscode/package.json -> $VERSION"
echo "  editors/zed/Cargo.toml  -> $VERSION"
echo "  editors/zed/extension.toml  -> $VERSION"

# 5. Regenerate Cargo.lock to reflect the new version
cargo generate-lockfile --quiet
echo "  Cargo.lock              -> regenerated"

git add \
    "$ROOT/Cargo.toml" \
    "$ROOT/Cargo.lock" \
    "$ROOT/editors/vscode/package.json" \
    "$ROOT/editors/zed/Cargo.toml" \
    "$ROOT/editors/zed/extension.toml"

git commit -m "release v$VERSION"
git tag "$TAG"

echo ""
echo "Created commit and tag $TAG."
echo "Run 'git push && git push --tags' to trigger the release workflow."
