# Skyscraper

A tool for deleting old posts from Bluesky, Mastodon, and Threads.

Posts older than a configurable retention period (default: 180 days) are deleted automatically. A do-not-delete list lets you exempt specific posts.

## Installation

### Homebrew (macOS and Linux)

```sh
brew install ZacSweers/tap/skyscraper
```

### Shell installer (macOS and Linux)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ZacSweers/skyscraper/releases/latest/download/skyscraper-cli-installer.sh | sh
```

### PowerShell installer (Windows)

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/ZacSweers/skyscraper/releases/latest/download/skyscraper-cli-installer.ps1 | iex"
```

### Cargo

```sh
cargo install skyscraper-cli
```

### GitHub Releases

Download prebuilt binaries from the [latest release](https://github.com/ZacSweers/skyscraper/releases/latest).

### GitHub Action

Use skyscraper in your own GitHub Actions workflow without installing anything:

```yaml
name: Skyscraper

on:
  schedule:
    - cron: '0 6 * * *'
  workflow_dispatch:

jobs:
  cleanup:
    runs-on: ubuntu-latest
    steps:
      - uses: ZacSweers/skyscraper@v1
        with:
          bluesky-identifier: ${{ secrets.BLUESKY_IDENTIFIER }}
          bluesky-app-password: ${{ secrets.BLUESKY_APP_PASSWORD }}
          mastodon-instance-url: ${{ secrets.MASTODON_INSTANCE_URL }}
          mastodon-access-token: ${{ secrets.MASTODON_ACCESS_TOKEN }}
          threads-access-token: ${{ secrets.THREADS_ACCESS_TOKEN }}
          do-not-delete: |
            bluesky:3k2la5diqyc2x
            mastodon:111234567890123456
          do-not-delete-path: path/to/do-not-delete.txt
```

## Setup

### 1. Generate tokens

You can run the `setup-tokens.sh` script in this repo to generate and configure all tokens interactively:

```sh
./setup-tokens.sh
```

The script walks you through each platform, validates credentials, and optionally writes them as GitHub Actions secrets via `gh secret set`.

If you prefer to do it manually, see [Manual token setup](#manual-token-setup) below.

### 2. Configure the do-not-delete list

Edit `do-not-delete.txt` to protect posts from deletion. One entry per line:

```
# Bluesky — use the rkey (last segment of the AT URI) or the full URI
bluesky:3k2la5diqyc2x
bluesky:at://did:plc:xyz/app.bsky.feed.post/3k2la5diqyc2x

# Mastodon — use the status ID
mastodon:111234567890123456

# Threads — use the media ID
threads:1234567890
```

Lines starting with `#` and blank lines are ignored.

## Environment variables

### Required (per platform)

Any platform whose variables are missing is silently skipped.

| Variable                | Description                                                                            |
|-------------------------|----------------------------------------------------------------------------------------|
| `BLUESKY_IDENTIFIER`    | Your handle (e.g. `user.bsky.social`) or DID                                           |
| `BLUESKY_APP_PASSWORD`  | App password — generate one at [bsky.app](https://bsky.app) → Settings → App Passwords |
| `MASTODON_INSTANCE_URL` | Instance base URL, e.g. `https://mastodon.social`                                      |
| `MASTODON_ACCESS_TOKEN` | OAuth access token from your instance's developer settings                             |
| `THREADS_ACCESS_TOKEN`  | Long-lived token from the [Meta developer portal](https://developers.facebook.com/)    |

### Optional

| Variable           | Default               | Description                                                          |
|--------------------|-----------------------|----------------------------------------------------------------------|
| `RETENTION_DAYS`   | `180`                 | Posts older than this many days are deleted                          |
| `DRY_RUN`          | `false`               | Set to `true` to log what would be deleted without actually deleting |
| `BLUESKY_PDS_HOST` | `https://bsky.social` | Override if your account is on a different PDS                       |

## Manual token setup

### Bluesky

1. Go to [bsky.app](https://bsky.app) → Settings → App Passwords.
2. Create a new app password (name it something like "skyscraper").
3. Set `BLUESKY_IDENTIFIER` to your handle and `BLUESKY_APP_PASSWORD` to the generated password.

### Mastodon

1. Log into your instance's web UI.
2. Go to Preferences → Development → New Application.
3. Set the application name (e.g. "skyscraper").
4. Required scopes: `read:accounts`, `read:statuses`, `write:statuses`.
5. Save, then copy the access token.
6. Set `MASTODON_INSTANCE_URL` to your instance URL and `MASTODON_ACCESS_TOKEN` to the token.

### Threads

1. Go to the [Meta Developer Portal](https://developers.facebook.com/) and create an app with Threads API access.
2. Add the `threads_basic`, `threads_content_publish`, and `threads_manage_posts` permissions.
3. Generate a short-lived token, then exchange it for a long-lived token (valid 60 days).
4. Set `THREADS_ACCESS_TOKEN` to the long-lived token.

> **Note:** Threads long-lived tokens expire after 60 days. You will need to
> re-run `./setup-tokens.sh` (or refresh the token manually) before it expires.
> See [Meta's documentation on long-lived tokens](https://developers.facebook.com/docs/threads/get-started/long-lived-tokens).

## Running locally

```sh
# Dry run (no deletions)
DRY_RUN=true BLUESKY_IDENTIFIER=you.bsky.social BLUESKY_APP_PASSWORD=xxxx cargo run

# Real run
BLUESKY_IDENTIFIER=you.bsky.social BLUESKY_APP_PASSWORD=xxxx cargo run
```

License
-------

    Copyright (C) 2026 Zac Sweers

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

       https://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
