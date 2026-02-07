# Changelog

## [Unreleased]

## [1.1.0]

_2026-02-07_

### Removed

- Threads support. The Threads API just throws 500 errors and doesn't appear to be a serious product.

## [1.0.0]

_2026-02-07_

Initial release.

- Delete old posts from Bluesky (AT Protocol) and Mastodon.
- Configurable retention period (default: 180 days).
- Do-not-delete list for exempting specific posts.
- Dry run mode for previewing deletions.
- Reusable GitHub Action (`ZacSweers/skyscraper@v1`).
- Prebuilt binaries for Linux, macOS, and Windows via cargo-dist.
- Homebrew formula via `ZacSweers/tap/skyscraper`.
- Published to crates.io as `skyscraper-cli`.
