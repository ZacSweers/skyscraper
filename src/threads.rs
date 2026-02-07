//! Threads integration via Meta's Threads API.
//!
//! Note: Threads long-lived access tokens expire after 60 days. You will need
//! to refresh the token periodically. See:
//! https://developers.facebook.com/docs/threads/get-started/long-lived-tokens
//! I've also not tested this at all because I'm not gonna make a facebook account just to hit an API

use crate::{is_protected, Config};
use anyhow::{Context, Result};
use log::{info, warn};
use serde::Deserialize;
use std::collections::HashSet;

const BASE_URL: &str = "https://graph.threads.net/v1.0";

#[derive(Deserialize)]
struct ThreadsResponse {
    data: Vec<ThreadsPost>,
    paging: Option<Paging>,
}

#[derive(Deserialize)]
struct ThreadsPost {
    id: String,
    timestamp: String,
}

#[derive(Deserialize)]
struct Paging {
    cursors: Option<Cursors>,
}

#[derive(Deserialize)]
struct Cursors {
    after: Option<String>,
}

pub async fn delete_old_posts(
    token: &str,
    config: &Config,
    do_not_delete: &HashSet<String>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("skyscraper/0.1.0")
        .build()?;

    let mut after: Option<String> = None;
    let mut deleted = 0u64;
    let mut skipped = 0u64;

    loop {
        let mut url =
            format!("{BASE_URL}/me/threads?fields=id,timestamp&access_token={token}&limit=50");
        if let Some(ref cursor) = after {
            url.push_str(&format!("&after={cursor}"));
        }

        let resp: ThreadsResponse = client
            .get(&url)
            .send()
            .await?
            .error_for_status()
            .context("Failed to fetch Threads posts")?
            .json()
            .await?;

        if resp.data.is_empty() {
            break;
        }

        for post in &resp.data {
            let post_time = match crate::parse_timestamp(&post.timestamp) {
                Ok(t) => t.with_timezone(&chrono::Utc),
                Err(e) => {
                    warn!("Skipping thread {}: {e}", post.id);
                    continue;
                }
            };

            if post_time >= config.cutoff {
                continue;
            }

            if is_protected(do_not_delete, "threads", &post.id) {
                skipped += 1;
                info!("Protected, skipping: {}", post.id);
                continue;
            }

            if config.dry_run {
                info!("[DRY RUN] Would delete: {} ({})", post.id, post.timestamp);
                deleted += 1;
                continue;
            }

            let resp = client
                .delete(format!("{BASE_URL}/{}?access_token={token}", post.id))
                .send()
                .await?;

            if resp.status().is_success() {
                deleted += 1;
                info!("Deleted: {} ({})", post.id, post.timestamp);
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                warn!("Failed to delete {}: {status} - {body}", post.id);
            }

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        after = resp.paging.and_then(|p| p.cursors).and_then(|c| c.after);
        if after.is_none() {
            break;
        }
    }

    info!("Threads: deleted {deleted}, skipped {skipped} protected");
    Ok(())
}
