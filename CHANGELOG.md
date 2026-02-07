# Changelog

## [Unreleased]

### Changed

- Renamed "do-not-delete" to "keep" everywhere: file is now `keep.txt`, env var is `KEEP_FILE`, action inputs are `keep` and `keep-path`.

### Added

- `KEEP_FILE` env var to specify a custom path for the keep list. Defaults to `keep.txt` in the current directory.
- Logging when the keep file is loaded or not found.
- Pinned posts are now skipped by default. Set `DELETE_PINNED=true` to override. When a pinned post is skipped, a warning is logged with the suggested keep list entry.

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
