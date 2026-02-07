use crate::{is_protected, Config};
use anyhow::{Context, Result};
use log::{info, warn};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
struct Account {
    id: String,
}

#[derive(Deserialize)]
struct Status {
    id: String,
    created_at: String,
}

pub async fn delete_old_posts(
    instance: &str,
    token: &str,
    config: &Config,
    do_not_delete: &HashSet<String>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("skyscraper/0.1.0")
        .build()?;
    let auth = format!("Bearer {token}");

    // Verify credentials and get account ID
    let account: Account = client
        .get(format!("{instance}/api/v1/accounts/verify_credentials"))
        .header("Authorization", &auth)
        .send()
        .await?
        .error_for_status()
        .context("Failed to verify Mastodon credentials")?
        .json()
        .await?;

    info!("Authenticated as account {}", account.id);

    let mut max_id: Option<String> = None;
    let mut deleted = 0u64;
    let mut skipped = 0u64;

    loop {
        let mut url = format!(
            "{instance}/api/v1/accounts/{}/statuses?limit=40",
            account.id
        );
        if let Some(ref id) = max_id {
            url.push_str(&format!("&max_id={id}"));
        }

        let statuses: Vec<Status> = client
            .get(&url)
            .header("Authorization", &auth)
            .send()
            .await?
            .error_for_status()
            .context("Failed to fetch Mastodon statuses")?
            .json()
            .await?;

        if statuses.is_empty() {
            break;
        }

        max_id = statuses.last().map(|s| s.id.clone());

        for status in &statuses {
            let post_time = match crate::parse_timestamp(&status.created_at) {
                Ok(t) => t.with_timezone(&chrono::Utc),
                Err(e) => {
                    warn!("Skipping status {}: {e}", status.id);
                    continue;
                }
            };

            if post_time >= config.cutoff {
                continue;
            }

            if is_protected(do_not_delete, "mastodon", &status.id) {
                skipped += 1;
                info!("Protected, skipping: {}", status.id);
                continue;
            }

            if config.dry_run {
                info!(
                    "[DRY RUN] Would delete: {} ({})",
                    status.id, status.created_at
                );
                deleted += 1;
                continue;
            }

            let resp = client
                .delete(format!("{instance}/api/v1/statuses/{}", status.id))
                .header("Authorization", &auth)
                .send()
                .await?;

            if resp.status().is_success() {
                deleted += 1;
                info!("Deleted: {} ({})", status.id, status.created_at);
            } else {
                warn!("Failed to delete {}: {}", status.id, resp.status());
            }

            // Mastodon rate-limits deletions; be conservative
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    info!("Mastodon: deleted {deleted}, skipped {skipped} protected");
    Ok(())
}
