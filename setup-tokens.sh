#!/usr/bin/env bash
set -euo pipefail

# --------------------------------------------------------------------------- #
# Skyscraper — interactive token setup
# Walks through credential creation for each platform and optionally stores
# them as GitHub Actions secrets.
# --------------------------------------------------------------------------- #

SECRETS=()  # accumulates NAME=VALUE pairs

prompt_secret() {
  local varname="$1" prompt="$2" value
  read -rp "$prompt: " value
  if [[ -z "$value" ]]; then
    return 1
  fi
  SECRETS+=("$varname=$value")
  printf -v "$varname" '%s' "$value"
  return 0
}

prompt_secret_hidden() {
  local varname="$1" prompt="$2" value
  read -rsp "$prompt: " value
  echo
  if [[ -z "$value" ]]; then
    return 1
  fi
  SECRETS+=("$varname=$value")
  printf -v "$varname" '%s' "$value"
  return 0
}

confirm() {
  local prompt="${1:-Continue?}" reply
  read -rp "$prompt [Y/n] " reply
  [[ -z "$reply" || "$reply" =~ ^[Yy] ]]
}

separator() {
  echo
  echo "───────────────────────────────────────────────────"
  echo
}

# --------------------------------------------------------------------------- #
echo "Skyscraper — Token Setup"
echo "========================"
echo
echo "This script walks you through configuring credentials for each platform."
echo "Skip any platform by pressing Enter at the first prompt."

# --------------------------------------------------------------------------- #
# Bluesky
# --------------------------------------------------------------------------- #
separator
echo "▸ Bluesky"
echo
echo "You need a handle (or DID) and an app password."
echo "Create an app password at: https://bsky.app → Settings → Privacy and Security → App Passwords"
echo

BLUESKY_IDENTIFIER=""
BLUESKY_APP_PASSWORD=""

if prompt_secret BLUESKY_IDENTIFIER "Handle or DID (e.g. you.bsky.social, blank to skip)"; then
  prompt_secret_hidden BLUESKY_APP_PASSWORD "App password"

  echo
  echo "Validating Bluesky credentials..."
  HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST "https://bsky.social/xrpc/com.atproto.server.createSession" \
    -H "Content-Type: application/json" \
    -d "{\"identifier\":\"${BLUESKY_IDENTIFIER}\",\"password\":\"${BLUESKY_APP_PASSWORD}\"}")

  if [[ "$HTTP_CODE" == "200" ]]; then
    echo "✓ Bluesky credentials are valid."
  else
    echo "✗ Authentication failed (HTTP $HTTP_CODE). Check your handle and app password."
    if ! confirm "Keep these values anyway?"; then
      # Remove the last two entries
      unset 'SECRETS[-1]'
      unset 'SECRETS[-1]'
    fi
  fi
else
  echo "Skipping Bluesky."
fi

# --------------------------------------------------------------------------- #
# Mastodon
# --------------------------------------------------------------------------- #
separator
echo "▸ Mastodon"
echo
echo "You need your instance URL and an access token."
echo "Create a token at: <your-instance>/settings/applications/new"
echo "Required scopes: read:accounts, read:statuses, write:statuses"
echo "Optional scopes: read:favourites, write:favourites (for deleting likes)"
echo

MASTODON_INSTANCE_URL=""
MASTODON_ACCESS_TOKEN=""

if prompt_secret MASTODON_INSTANCE_URL "Instance URL (e.g. https://mastodon.social, blank to skip)"; then
  # Strip trailing slash
  MASTODON_INSTANCE_URL="${MASTODON_INSTANCE_URL%/}"
  # Update the stored value without the trailing slash
  for i in "${!SECRETS[@]}"; do
    if [[ "${SECRETS[$i]}" == MASTODON_INSTANCE_URL=* ]]; then
      SECRETS[$i]="MASTODON_INSTANCE_URL=$MASTODON_INSTANCE_URL"
    fi
  done

  prompt_secret_hidden MASTODON_ACCESS_TOKEN "Access token"

  echo
  echo "Validating Mastodon credentials..."
  HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" \
    -H "Authorization: Bearer ${MASTODON_ACCESS_TOKEN}" \
    "${MASTODON_INSTANCE_URL}/api/v1/accounts/verify_credentials")

  if [[ "$HTTP_CODE" == "200" ]]; then
    echo "✓ Mastodon credentials are valid."
  else
    echo "✗ Authentication failed (HTTP $HTTP_CODE). Check your instance URL and token."
    if ! confirm "Keep these values anyway?"; then
      unset 'SECRETS[-1]'
      unset 'SECRETS[-1]'
    fi
  fi
else
  echo "Skipping Mastodon."
fi

# --------------------------------------------------------------------------- #
# Summary and GitHub secrets
# --------------------------------------------------------------------------- #
separator

if [[ ${#SECRETS[@]} -eq 0 ]]; then
  echo "No credentials were configured. Nothing to do."
  exit 0
fi

echo "Configured secrets:"
for entry in "${SECRETS[@]}"; do
  name="${entry%%=*}"
  echo "  • $name"
done
echo

# --- Write to GitHub Actions secrets ---
if command -v gh &>/dev/null; then
  if confirm "Store these as GitHub Actions secrets using 'gh secret set'?"; then
    echo
    for entry in "${SECRETS[@]}"; do
      name="${entry%%=*}"
      value="${entry#*=}"
      echo -n "$value" | gh secret set "$name" --body -
      echo "  ✓ Set $name"
    done
    echo
    echo "Done! Secrets are stored in GitHub Actions."
  fi
else
  echo "The 'gh' CLI is not installed — skipping GitHub secrets setup."
  echo "Install it from https://cli.github.com/ and re-run, or set secrets manually:"
  echo "  gh secret set SECRET_NAME"
fi

separator
echo "Setup complete. Push this repo and the nightly workflow will run automatically."
echo "Trigger a manual run from the Actions tab to verify."
