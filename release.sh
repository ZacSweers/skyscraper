#!/usr/bin/env bash
set -euo pipefail

# --------------------------------------------------------------------------- #
# release.sh — automate the full release process
# Usage: ./release.sh <version>   (e.g. ./release.sh 1.2.0)
# --------------------------------------------------------------------------- #

VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
  echo "Usage: ./release.sh <version>"
  echo "Example: ./release.sh 1.2.0"
  exit 1
fi

# Strip leading 'v' if provided
VERSION="${VERSION#v}"
TAG="v${VERSION}"
REPO="ZacSweers/skyscraper"
MAJOR_TAG="v$(echo "$VERSION" | cut -d. -f1)"

echo "Releasing ${TAG}"
echo "================"
echo

# --- Preflight checks ---
if ! command -v gh &>/dev/null; then
  echo "Error: 'gh' CLI is required. Install from https://cli.github.com/"
  exit 1
fi

if ! command -v cargo &>/dev/null; then
  echo "Error: 'cargo' is required."
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "Error: Working tree is dirty. Commit or stash changes first."
  exit 1
fi

if git rev-parse "$TAG" &>/dev/null; then
  echo "Error: Tag ${TAG} already exists."
  exit 1
fi

# --- Update CHANGELOG.md ---
echo "→ Updating CHANGELOG.md"
DATE=$(date +%Y-%m-%d)
if ! grep -q "## \[Unreleased\]" CHANGELOG.md; then
  echo "Error: CHANGELOG.md is missing an [Unreleased] section."
  exit 1
fi
# Insert new version heading with date on next line, matching existing format
sed -i '' "s/## \[Unreleased\]/## [Unreleased]\\
\\
## [${VERSION}]\\
\\
_${DATE}_/" CHANGELOG.md

# --- Bump version in Cargo.toml ---
echo "→ Bumping version to ${VERSION}"
sed -i '' "s/^version = \".*\"/version = \"${VERSION}\"/" Cargo.toml

# --- Build and regenerate lockfile ---
echo "→ Building and regenerating lockfile"
cargo build

# --- Commit and push ---
echo "→ Committing version bump"
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "Prepare release ${VERSION}"
git push origin main

# --- Tag and push ---
echo "→ Tagging ${TAG}"
git tag "$TAG"
git push origin "$TAG"

# --- Wait for release workflow ---
echo "→ Waiting for release workflow to start..."
sleep 5

RUN_ID=""
for i in {1..10}; do
  RUN_ID=$(gh run list --repo "$REPO" --workflow=release.yml --branch="$TAG" --json databaseId,headBranch --jq ".[] | select(.headBranch == \"${TAG}\") | .databaseId" | head -1)
  if [[ -n "$RUN_ID" ]]; then
    break
  fi
  sleep 3
done

if [[ -z "$RUN_ID" ]]; then
  echo "Error: Could not find release workflow run for ${TAG}."
  echo "Check https://github.com/${REPO}/actions manually."
  exit 1
fi

echo "→ Watching release workflow (run ${RUN_ID})..."
gh run watch "$RUN_ID" --repo "$REPO"

# Check if it succeeded
STATUS=$(gh run view "$RUN_ID" --repo "$REPO" --json conclusion -q .conclusion)
if [[ "$STATUS" != "success" ]]; then
  echo "Error: Release workflow failed with status: ${STATUS}"
  echo "Check https://github.com/${REPO}/actions/runs/${RUN_ID}"
  exit 1
fi

echo "→ Release workflow completed successfully"

# --- Publish to crates.io ---
read -rp "Publish to crates.io? [Y/n] " reply
if [[ -z "$reply" || "$reply" =~ ^[Yy] ]]; then
  echo "→ Publishing to crates.io"
  cargo publish
fi

# --- Update major version action tag ---
echo "→ Updating ${MAJOR_TAG} action tag"
git tag -f "$MAJOR_TAG" "$TAG"
git push -f origin "$MAJOR_TAG"

echo
echo "Done! Released ${TAG}"
echo "  GitHub Release: https://github.com/${REPO}/releases/tag/${TAG}"
echo "  crates.io:      https://crates.io/crates/skyscraper-cli/${VERSION}"
