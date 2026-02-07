use crate::{is_protected, Config};
use anyhow::{Context, Result};
use log::{info, warn};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
struct Session {
    did: String,
    #[serde(rename = "accessJwt")]
    access_jwt: String,
}

#[derive(Deserialize)]
struct ListRecordsResponse {
    records: Vec<Record>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
struct Record {
    uri: String,
    value: PostRecord,
}

#[derive(Deserialize)]
struct PostRecord {
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
}

pub async fn delete_old_posts(
    pds: &str,
    identifier: &str,
    password: &str,
    config: &Config,
    do_not_delete: &HashSet<String>,
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent("skyscraper/0.1.0")
        .build()?;

    // Authenticate
    let session: Session = client
        .post(format!("{pds}/xrpc/com.atproto.server.createSession"))
        .json(&serde_json::json!({
            "identifier": identifier,
            "password": password,
        }))
        .send()
        .await?
        .error_for_status()
        .context("Failed to authenticate with Bluesky")?
        .json()
        .await?;

    info!("Authenticated as {}", session.did);

    let mut cursor: Option<String> = None;
    let mut deleted = 0u64;
    let mut skipped = 0u64;

    loop {
        let mut url = format!(
            "{pds}/xrpc/com.atproto.repo.listRecords?repo={}&collection=app.bsky.feed.post&limit=100",
            session.did
        );
        if let Some(ref c) = cursor {
            url.push_str(&format!("&cursor={c}"));
        }

        let resp: ListRecordsResponse = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", session.access_jwt))
            .send()
            .await?
            .error_for_status()
            .context("Failed to list Bluesky records")?
            .json()
            .await?;

        if resp.records.is_empty() {
            break;
        }

        for record in &resp.records {
            let Some(ref created_at) = record.value.created_at else {
                warn!("Record missing createdAt, skipping: {}", record.uri);
                continue;
            };

            let post_time = match crate::parse_timestamp(created_at) {
                Ok(t) => t.with_timezone(&chrono::Utc),
                Err(e) => {
                    warn!("Skipping record {}: {e}", record.uri);
                    continue;
                }
            };

            if post_time >= config.cutoff {
                continue;
            }

            // rkey is the last segment of the AT URI
            let rkey = record.uri.rsplit('/').next().context("Invalid AT URI")?;

            if is_protected(do_not_delete, "bluesky", rkey)
                || is_protected(do_not_delete, "bluesky", &record.uri)
            {
                skipped += 1;
                info!("Protected, skipping: {}", record.uri);
                continue;
            }

            if config.dry_run {
                info!("[DRY RUN] Would delete: {} ({created_at})", record.uri);
                deleted += 1;
                continue;
            }

            let del_resp = client
                .post(format!("{pds}/xrpc/com.atproto.repo.deleteRecord"))
                .header("Authorization", format!("Bearer {}", session.access_jwt))
                .json(&serde_json::json!({
                    "repo": session.did,
                    "collection": "app.bsky.feed.post",
                    "rkey": rkey,
                }))
                .send()
                .await?;

            if del_resp.status().is_success() {
                deleted += 1;
                info!("Deleted: {} ({created_at})", record.uri);
            } else {
                warn!("Failed to delete {}: {}", record.uri, del_resp.status());
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        cursor = resp.cursor;
        if cursor.is_none() {
            break;
        }
    }

    info!("Bluesky: deleted {deleted}, skipped {skipped} protected");
    Ok(())
}
