# Releasing

## Regular release

1. **Bump the version** in `Cargo.toml`:
   ```toml
   version = "1.1.0"
   ```

2. **Commit the version bump**:
   ```sh
   git add Cargo.toml Cargo.lock
   git commit -m "Prepare release 1.1.0"
   git push origin main
   ```

3. **Tag and push** to trigger the release workflow:
   ```sh
   git tag v1.1.0
   git push origin v1.1.0
   ```

   This triggers `.github/workflows/release.yml`, which:
   - Cross-compiles binaries for all platforms (Linux, macOS, Windows)
   - Generates shell and PowerShell installer scripts
   - Creates a GitHub Release with all artifacts
   - Pushes the updated Homebrew formula to `ZacSweers/homebrew-tap`

4. **Wait for the release workflow to complete** — check the Actions tab.

5. **Publish to crates.io** (manual, after verifying the GitHub Release looks good):
   ```sh
   cargo publish
   ```
   This is intentionally manual. Crate publishes are permanent (you can yank but not delete).

6. **Update the `v1` action tag** so GitHub Action consumers on `@v1` get the new version:
   ```sh
   git tag -f v1 v1.1.0
   git push -f origin v1
   ```
   Only bump to `v2` if you make breaking changes to `action.yml` inputs.

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

## Checklist

```
- [ ] Version bumped in Cargo.toml
- [ ] Committed and pushed to main
- [ ] Tag pushed (vX.Y.Z)
- [ ] Release workflow completed successfully
- [ ] GitHub Release looks correct
- [ ] Published to crates.io (`cargo publish`)
- [ ] Updated v1 action tag
- [ ] Verified `brew install ZacSweers/tap/skyscraper` works
```
