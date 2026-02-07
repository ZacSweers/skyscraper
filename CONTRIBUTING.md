# Contributing

## Development setup

```sh
git clone git@github.com:ZacSweers/skyscraper.git
cd skyscraper
cargo build
```

## Running locally

```sh
# Dry run with one platform
DRY_RUN=true \
BLUESKY_IDENTIFIER=you.bsky.social \
BLUESKY_APP_PASSWORD=your-app-password \
cargo run

# All env vars are documented in README.md
```

## Token setup helper

The `setup-tokens.sh` script walks through credential creation for each platform and optionally stores them as GitHub Actions secrets:

```sh
./setup-tokens.sh
```

## Code style

- Format with `cargo fmt`
- Lint with `cargo clippy -- -D warnings`
- CI enforces both on every push and PR

## Project structure

```
src/
  main.rs       — entry point, config loading, platform orchestration
  bluesky.rs    — Bluesky/AT Protocol integration
  mastodon.rs   — Mastodon API integration
  threads.rs    — Threads/Meta API integration
```

## Adding a new platform

1. Create `src/newplatform.rs` with a `pub async fn delete_old_posts(...)` following the same signature pattern as the existing modules.
2. Add `mod newplatform;` to `main.rs`.
3. Add a new env var block in `main()` that calls your module.
4. Add the new inputs to `action.yml`.
5. Update `setup-tokens.sh` with a section for the new platform.
6. Document the new env vars in `README.md`.

## cargo-dist

Release automation is managed by [cargo-dist](https://opensource.axo.dev/cargo-dist/). Config lives in `dist-workspace.toml`. The release workflow at `.github/workflows/release.yml` is **auto-generated** — never edit it by hand.

To regenerate after changing `dist-workspace.toml`:

```sh
dist generate
dist generate --check  # verify it's in sync
dist plan              # preview what a release would produce
```

See [RELEASING.md](RELEASING.md) for the full release process.
