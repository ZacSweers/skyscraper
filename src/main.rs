mod bluesky;
mod mastodon;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, TimeDelta, Utc};
use log::{error, info, warn};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

pub struct Config {
    pub cutoff: DateTime<Utc>,
    pub dry_run: bool,
    pub delete_pinned: bool,
}

/// Parse an ISO 8601 / RFC 3339 timestamp, tolerating the `+0000` offset
/// format that some APIs return instead of `+00:00`.
pub fn parse_timestamp(s: &str) -> Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z"))
        .with_context(|| format!("Failed to parse timestamp: {s}"))
}

pub fn is_protected(keep_list: &HashSet<String>, platform: &str, id: &str) -> bool {
    keep_list.contains(&format!("{platform}:{id}")) || keep_list.contains(id)
}

fn load_keep_list(path: &Path) -> HashSet<String> {
    if !path.exists() {
        info!("No keep file at {}, skipping", path.display());
        return HashSet::new();
    }
    info!("Loading keep list from {}", path.display());
    let Ok(contents) = fs::read_to_string(path) else {
        warn!("Failed to read {}", path.display());
        return HashSet::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let retention_days: i64 = env::var("RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(180);

    let dry_run = env::var("DRY_RUN")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let delete_pinned = env::var("DELETE_PINNED")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let cutoff = Utc::now() - TimeDelta::days(retention_days);
    let keep_file = env::var("KEEP_FILE").unwrap_or_else(|_| "keep.txt".into());
    let keep_list = load_keep_list(Path::new(&keep_file));

    info!("Skyscraper - Social Media Post Cleanup");
    info!("Cutoff date: {cutoff}");
    info!("Dry run: {dry_run}");
    info!("Protected posts: {}", keep_list.len());

    let config = Config {
        cutoff,
        dry_run,
        delete_pinned,
    };
    let mut had_errors = false;

    // --- Bluesky ---
    match (
        env::var("BLUESKY_IDENTIFIER"),
        env::var("BLUESKY_APP_PASSWORD"),
    ) {
        (Ok(identifier), Ok(password)) => {
            let pds = env::var("BLUESKY_PDS_HOST").unwrap_or_else(|_| "https://bsky.social".into());
            info!("Processing Bluesky account: {identifier}");
            if let Err(e) =
                bluesky::delete_old_posts(&pds, &identifier, &password, &config, &keep_list).await
            {
                error!("Bluesky error: {e:#}");
                had_errors = true;
            }
        }
        _ => warn!("Bluesky credentials not set, skipping"),
    }

    // --- Mastodon ---
    match (
        env::var("MASTODON_INSTANCE_URL"),
        env::var("MASTODON_ACCESS_TOKEN"),
    ) {
        (Ok(instance), Ok(token)) => {
            info!("Processing Mastodon instance: {instance}");
            if let Err(e) = mastodon::delete_old_posts(&instance, &token, &config, &keep_list).await
            {
                error!("Mastodon error: {e:#}");
                had_errors = true;
            }
        }
        _ => warn!("Mastodon credentials not set, skipping"),
    }

    if had_errors {
        anyhow::bail!("One or more platforms encountered errors");
    }

    info!("Done!");
    Ok(())
}
