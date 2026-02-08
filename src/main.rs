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
    pub delete_reposts: bool,
    pub delete_likes: bool,
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

    let delete_reposts = env::var("DELETE_REPOSTS")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    let delete_likes = env::var("DELETE_LIKES")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    let cutoff = Utc::now() - TimeDelta::days(retention_days);
    let keep_file = env::var("KEEP_FILE").unwrap_or_else(|_| "keep.txt".into());
    let keep_list = load_keep_list(Path::new(&keep_file));

    info!("Skyscraper - Social Media Post Cleanup");
    info!("Cutoff date: {cutoff}");
    info!("Dry run: {dry_run}");
    info!("Delete reposts: {delete_reposts}");
    info!("Delete likes: {delete_likes}");
    info!("Delete pinned: {delete_pinned}");
    info!("Protected posts: {}", keep_list.len());

    let config = Config {
        cutoff,
        dry_run,
        delete_pinned,
        delete_reposts,
        delete_likes,
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
            let client = bluesky::HttpBlueskyClient::new(&pds);
            if let Err(e) =
                bluesky::delete_old_posts(&client, &identifier, &password, &config, &keep_list)
                    .await
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
            let client = mastodon::HttpMastodonClient::new(&instance, &token);
            if let Err(e) = mastodon::delete_old_posts(&client, &config, &keep_list).await {
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

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::Config;
    use chrono::{TimeDelta, Utc};

    pub fn config_with_cutoff_days_ago(days: i64) -> Config {
        Config {
            cutoff: Utc::now() - TimeDelta::days(days),
            dry_run: false,
            delete_pinned: false,
            delete_reposts: true,
            delete_likes: true,
        }
    }

    pub fn old_timestamp() -> String {
        "2020-01-01T00:00:00Z".to_string()
    }

    pub fn recent_timestamp() -> String {
        (Utc::now() + TimeDelta::days(1))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // --- parse_timestamp ---

    #[test]
    fn parse_timestamp_rfc3339_utc() {
        let dt = parse_timestamp("2024-06-15T12:30:00Z").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-06-15T12:30:00+00:00");
    }

    #[test]
    fn parse_timestamp_rfc3339_with_offset() {
        let dt = parse_timestamp("2024-06-15T12:30:00+05:30").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-06-15T12:30:00+05:30");
    }

    #[test]
    fn parse_timestamp_plus0000_fallback() {
        let dt = parse_timestamp("2024-06-15T12:30:00+0000").unwrap();
        assert_eq!(dt.to_rfc3339(), "2024-06-15T12:30:00+00:00");
    }

    #[test]
    fn parse_timestamp_invalid_string() {
        assert!(parse_timestamp("not-a-date").is_err());
    }

    #[test]
    fn parse_timestamp_empty_string() {
        assert!(parse_timestamp("").is_err());
    }

    // --- is_protected ---

    #[test]
    fn is_protected_platform_prefixed_match() {
        let keep = HashSet::from(["bluesky:abc123".to_string()]);
        assert!(is_protected(&keep, "bluesky", "abc123"));
    }

    #[test]
    fn is_protected_bare_id_match() {
        let keep = HashSet::from(["abc123".to_string()]);
        assert!(is_protected(&keep, "bluesky", "abc123"));
    }

    #[test]
    fn is_protected_no_match() {
        let keep = HashSet::from(["bluesky:other".to_string()]);
        assert!(!is_protected(&keep, "bluesky", "abc123"));
    }

    #[test]
    fn is_protected_wrong_platform_prefix() {
        let keep = HashSet::from(["mastodon:abc123".to_string()]);
        assert!(!is_protected(&keep, "bluesky", "abc123"));
    }

    // --- load_keep_list ---

    #[test]
    fn load_keep_list_parses_entries_skipping_comments_and_blanks() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "# comment").unwrap();
        writeln!(f).unwrap();
        writeln!(f, "bluesky:abc123").unwrap();
        writeln!(f, "mastodon:456").unwrap();
        let result = load_keep_list(f.path());
        assert_eq!(result.len(), 2);
        assert!(result.contains("bluesky:abc123"));
        assert!(result.contains("mastodon:456"));
    }

    #[test]
    fn load_keep_list_trims_whitespace() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "  bluesky:abc123  ").unwrap();
        let result = load_keep_list(f.path());
        assert!(result.contains("bluesky:abc123"));
    }

    #[test]
    fn load_keep_list_nonexistent_file_returns_empty() {
        let result = load_keep_list(Path::new("/nonexistent/keep.txt"));
        assert!(result.is_empty());
    }

    #[test]
    fn load_keep_list_empty_file_returns_empty() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let result = load_keep_list(f.path());
        assert!(result.is_empty());
    }
}
