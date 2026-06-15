#!/usr/bin/env bash
#
# Bump the version everywhere, commit, and tag — the single source of truth for
# a release. Pushing the tag triggers .github/workflows/release.yml, which
# publishes to crates.io, PyPI, and the VS Code / Open VSX marketplaces and
# attaches binaries + wheels to a GitHub release.
#
# Version locations kept in sync (see also the `validate` job in release.yml):
#   crate + CLI      Cargo.toml, Cargo.lock
#   Python bindings  bindings/python/{Cargo.toml,pyproject.toml,Cargo.lock}
#   Zed extension    editors/zed/{Cargo.toml,extension.toml,Cargo.lock}
#   VS Code ext.     editors/vscode/{package.json,package-lock.json}
#
# The Zed extension's `[lib] version` in extension.toml is intentionally left
# alone (it is the extension-API version, not the release version).
set -euo pipefail

usage() {
    echo "Usage: $0 <version>"
    echo "Example: $0 0.9.0"
    exit 1
}

[[ $# -ne 1 ]] && usage

# Strip a leading 'v' if provided.
VERSION="${1#v}"

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: '$VERSION' is not a valid version (expected X.Y.Z)"
    exit 1
fi

TAG="v$VERSION"
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "Error: working tree is dirty. Commit or stash changes first."
    exit 1
fi

# All files we touch (also the set we stage / revert on failure).
FILES=(
    Cargo.toml
    Cargo.lock
    bindings/python/Cargo.toml
    bindings/python/pyproject.toml
    bindings/python/Cargo.lock
    editors/zed/Cargo.toml
    editors/zed/extension.toml
    editors/zed/Cargo.lock
    editors/vscode/package.json
    editors/vscode/package-lock.json
)

echo "Bumping version to $VERSION ..."

# Bump the first `version = "..."` line (the [package]/[project] version).
bump_toml() {
    VERSION="$VERSION" perl -0777 -pi -e 's/^version = "[^"]*"/version = "$ENV{VERSION}"/m' "$1"
}

# Bump the version of a specific [[package]] entry inside a Cargo.lock.
bump_lock_pkg() {
    PKG="$2" VERSION="$VERSION" perl -0777 -pi -e \
        's/(\[\[package\]\]\nname = "\Q$ENV{PKG}\E"\nversion = ")[^"]*"/$1 . $ENV{VERSION} . q{"}/e' "$1"
}

# crate + CLI
bump_toml Cargo.toml
bump_lock_pkg Cargo.lock webspec-index

# Python bindings
bump_toml bindings/python/Cargo.toml
bump_toml bindings/python/pyproject.toml
bump_lock_pkg bindings/python/Cargo.lock webspec-index
bump_lock_pkg bindings/python/Cargo.lock webspec-index-python

# Zed extension (top-level version only; [lib] version is left as-is)
bump_toml editors/zed/Cargo.toml
bump_toml editors/zed/extension.toml
bump_lock_pkg editors/zed/Cargo.lock webspec-lens-zed

# VS Code extension: package.json + the two root version fields in the lockfile.
VERSION="$VERSION" perl -0777 -pi -e \
    's/"version": "[^"]*"/"version": "$ENV{VERSION}"/' editors/vscode/package.json
VERSION="$VERSION" perl -0777 -pi -e \
    'my $n=0; s/"version": "[^"]*"/$n++ < 2 ? qq{"version": "$ENV{VERSION}"} : $&/ge' \
    editors/vscode/package-lock.json

# Verify every published version location now matches before committing.
fail=0
check() {
    if ! grep -Eq "$2" "$1"; then
        echo "  MISSING: $1 was not updated to $VERSION"
        fail=1
    fi
}
check Cargo.toml                     "^version = \"$VERSION\""
check bindings/python/Cargo.toml     "^version = \"$VERSION\""
check bindings/python/pyproject.toml "^version = \"$VERSION\""
check editors/vscode/package.json    "\"version\": \"$VERSION\""
check editors/zed/Cargo.toml         "^version = \"$VERSION\""
check editors/zed/extension.toml     "^version = \"$VERSION\""

if [[ "$fail" -ne 0 ]]; then
    echo "Version bump verification failed; reverting (no commit/tag created)."
    git checkout -- "${FILES[@]}"
    exit 1
fi

echo "Updated:"
for f in "${FILES[@]}"; do echo "  $f"; done

git add "${FILES[@]}"
git commit -m "release v$VERSION"
git tag "$TAG"

echo ""
echo "Created commit and tag $TAG."
echo "Review with: git show $TAG"
echo "Then run:    git push && git push --tags   (triggers the release workflow)"
