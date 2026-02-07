# Releasing

## Regular release

```sh
./release.sh 1.2.0
```

The script handles the full process:

1. Bumps the version in `Cargo.toml`
2. Builds and regenerates `Cargo.lock`
3. Commits and pushes to main
4. Tags and pushes to trigger the release workflow
5. Waits for the release workflow to complete
6. Prompts to publish to crates.io
7. Updates the major version action tag (e.g. `v1`)

### Prerequisites

- Clean working tree (no uncommitted changes)
- `gh` CLI installed and authenticated
- `cargo login` already run (for crates.io publishing)

## Upgrading cargo-dist

When a new cargo-dist version is available:

1. Update `cargo-dist-version` in `dist-workspace.toml`
2. Regenerate the release workflow:
   ```sh
   dist generate
   ```
3. Verify:
   ```sh
   dist generate --check
   dist plan
   ```
4. Commit the updated `dist-workspace.toml` and `release.yml` together.

**Never hand-edit `release.yml`** — it will be overwritten by `dist generate`.

## Secrets

These GitHub Actions secrets must be configured on `ZacSweers/skyscraper`:

| Secret                  | Purpose                                                  | Rotation                                          |
|-------------------------|----------------------------------------------------------|---------------------------------------------------|
| `HOMEBREW_TAP_TOKEN`    | PAT with Contents read/write on `ZacSweers/homebrew-tap` | Check expiry, rotate before it lapses             |
