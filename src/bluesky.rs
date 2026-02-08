use crate::{is_protected, Config};
use anyhow::{Context, Result};
use log::{info, warn};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub(crate) struct Session {
    did: String,
    #[serde(rename = "accessJwt")]
    access_jwt: String,
}

#[derive(Deserialize)]
struct ProfileRecord {
    value: ProfileValue,
}

#[derive(Deserialize)]
struct ProfileValue {
    #[serde(rename = "pinnedPost")]
    pinned_post: Option<String>,
}

#[derive(Deserialize, Clone)]
pub(crate) struct ListRecordsResponse {
    records: Vec<Record>,
    cursor: Option<String>,
}

#[derive(Deserialize, Clone)]
struct Record {
    uri: String,
    value: RecordValue,
}

#[derive(Deserialize, Clone)]
struct RecordValue {
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
}

struct DeleteResult {
    deleted: u64,
    skipped_pinned: u64,
    skipped_kept: u64,
}

pub(crate) trait BlueskyClient {
    async fn create_session(&self, identifier: &str, password: &str) -> Result<Session>;
    async fn get_pinned_post_uri(&self, did: &str) -> Option<String>;
    async fn list_records(
        &self,
        did: &str,
        collection: &str,
        cursor: Option<&str>,
    ) -> Result<ListRecordsResponse>;
    async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()>;
}

pub(crate) struct HttpBlueskyClient {
    client: reqwest::Client,
    pds: String,
    session: std::sync::OnceLock<Session>,
}

impl HttpBlueskyClient {
    pub fn new(pds: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("skyscraper/0.1.0")
                .build()
                .expect("Failed to build HTTP client"),
            pds: pds.to_string(),
            session: std::sync::OnceLock::new(),
        }
    }

    fn session(&self) -> &Session {
        self.session.get().expect("Session not initialized")
    }
}

impl BlueskyClient for HttpBlueskyClient {
    async fn create_session(&self, identifier: &str, password: &str) -> Result<Session> {
        let session: Session = self
            .client
            .post(format!(
                "{}/xrpc/com.atproto.server.createSession",
                self.pds
            ))
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
        let _ = self.session.set(Session {
            did: session.did.clone(),
            access_jwt: session.access_jwt.clone(),
        });
        Ok(session)
    }

    async fn get_pinned_post_uri(&self, did: &str) -> Option<String> {
        let session = self.session();
        let profile_url = format!(
            "{}/xrpc/com.atproto.repo.getRecord?repo={}&collection=app.bsky.actor.profile&rkey=self",
            self.pds, did
        );
        match self
            .client
            .get(&profile_url)
            .header("Authorization", format!("Bearer {}", session.access_jwt))
            .send()
            .await
        {
            Ok(resp) => resp
                .json::<ProfileRecord>()
                .await
                .ok()
                .and_then(|p| p.value.pinned_post),
            Err(_) => None,
        }
    }

    async fn list_records(
        &self,
        did: &str,
        collection: &str,
        cursor: Option<&str>,
    ) -> Result<ListRecordsResponse> {
        let session = self.session();
        let mut url = format!(
            "{}/xrpc/com.atproto.repo.listRecords?repo={}&collection={}&limit=100",
            self.pds, did, collection
        );
        if let Some(c) = cursor {
            url.push_str(&format!("&cursor={c}"));
        }

        let resp: ListRecordsResponse = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", session.access_jwt))
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("Failed to list Bluesky records for {collection}"))?
            .json()
            .await?;

        Ok(resp)
    }

    async fn delete_record(&self, did: &str, collection: &str, rkey: &str) -> Result<()> {
        let session = self.session();
        self.client
            .post(format!("{}/xrpc/com.atproto.repo.deleteRecord", self.pds))
            .header("Authorization", format!("Bearer {}", session.access_jwt))
            .json(&serde_json::json!({
                "repo": did,
                "collection": collection,
                "rkey": rkey,
            }))
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("Failed to delete {collection}/{rkey}"))?;
        Ok(())
    }
}

async fn delete_old_records(
    client: &(impl BlueskyClient + Sync),
    did: &str,
    collection: &str,
    label: &str,
    config: &Config,
    keep_list: &HashSet<String>,
    pinned_uri: Option<&str>,
) -> Result<DeleteResult> {
    let mut cursor: Option<String> = None;
    let mut deleted = 0u64;
    let mut skipped_pinned = 0u64;
    let mut skipped_kept = 0u64;

    loop {
        let resp = client
            .list_records(did, collection, cursor.as_deref())
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

            if pinned_uri == Some(record.uri.as_str()) {
                skipped_pinned += 1;
                warn!(
                    "Skipping pinned post: {}. To keep it permanently, add to your keep file: bluesky:{}",
                    record.uri, rkey
                );
                continue;
            }

            if is_protected(keep_list, "bluesky", rkey)
                || is_protected(keep_list, "bluesky", &record.uri)
            {
                skipped_kept += 1;
                info!("Protected, skipping: {}", record.uri);
                continue;
            }

            if config.dry_run {
                info!(
                    "[DRY RUN] Would delete {label}: {} ({created_at})",
                    record.uri
                );
                deleted += 1;
                continue;
            }

            match client.delete_record(did, collection, rkey).await {
                Ok(()) => {
                    deleted += 1;
                    info!("Deleted {label}: {} ({created_at})", record.uri);
                }
                Err(e) => {
                    warn!("Failed to delete {}: {e}", record.uri);
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        cursor = resp.cursor;
        if cursor.is_none() {
            break;
        }
    }

    Ok(DeleteResult {
        deleted,
        skipped_pinned,
        skipped_kept,
    })
}

pub async fn delete_old_posts(
    client: &(impl BlueskyClient + Sync),
    identifier: &str,
    password: &str,
    config: &Config,
    keep_list: &HashSet<String>,
) -> Result<()> {
    // Authenticate
    let session = client.create_session(identifier, password).await?;
    info!("Authenticated as {}", session.did);

    // Fetch pinned post URI from profile
    let pinned_uri: Option<String> = if !config.delete_pinned {
        client.get_pinned_post_uri(&session.did).await
    } else {
        None
    };

    // Delete old posts
    let posts = delete_old_records(
        client,
        &session.did,
        "app.bsky.feed.post",
        "post",
        config,
        keep_list,
        pinned_uri.as_deref(),
    )
    .await?;

    info!(
        "Bluesky posts: deleted {}, skipped {} pinned, skipped {} kept",
        posts.deleted, posts.skipped_pinned, posts.skipped_kept
    );

    // Delete old reposts
    if config.delete_reposts {
        let reposts = delete_old_records(
            client,
            &session.did,
            "app.bsky.feed.repost",
            "repost",
            config,
            keep_list,
            None,
        )
        .await?;

        info!(
            "Bluesky reposts: deleted {}, skipped {} kept",
            reposts.deleted, reposts.skipped_kept
        );
    }

    // Delete old likes
    if config.delete_likes {
        let likes = delete_old_records(
            client,
            &session.did,
            "app.bsky.feed.like",
            "like",
            config,
            keep_list,
            None,
        )
        .await?;

        info!(
            "Bluesky likes: deleted {}, skipped {} kept",
            likes.deleted, likes.skipped_kept
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use std::sync::Mutex;

    const DID: &str = "did:plc:testuser123";
    const PINNED_URI: &str = "at://did:plc:testuser123/app.bsky.feed.post/pinned1";

    struct FakeBlueskyClient {
        did: String,
        pinned_post: Option<String>,
        records: Mutex<std::collections::HashMap<String, Vec<Record>>>,
        deleted: Mutex<Vec<(String, String)>>,
        page_size: usize,
    }

    impl FakeBlueskyClient {
        fn new(did: &str) -> Self {
            Self {
                did: did.to_string(),
                pinned_post: None,
                records: Mutex::new(std::collections::HashMap::new()),
                deleted: Mutex::new(Vec::new()),
                page_size: 100,
            }
        }

        fn with_pinned_post(mut self, uri: &str) -> Self {
            self.pinned_post = Some(uri.to_string());
            self
        }

        fn with_records(self, collection: &str, records: Vec<Record>) -> Self {
            self.records
                .lock()
                .unwrap()
                .insert(collection.to_string(), records);
            self
        }

        fn with_page_size(mut self, size: usize) -> Self {
            self.page_size = size;
            self
        }

        fn deleted(&self) -> Vec<(String, String)> {
            self.deleted.lock().unwrap().clone()
        }
    }

    impl BlueskyClient for FakeBlueskyClient {
        async fn create_session(&self, _identifier: &str, _password: &str) -> Result<Session> {
            Ok(Session {
                did: self.did.clone(),
                access_jwt: "fake".to_string(),
            })
        }

        async fn get_pinned_post_uri(&self, _did: &str) -> Option<String> {
            self.pinned_post.clone()
        }

        async fn list_records(
            &self,
            _did: &str,
            collection: &str,
            cursor: Option<&str>,
        ) -> Result<ListRecordsResponse> {
            let records_map = self.records.lock().unwrap();
            let all_records = match records_map.get(collection) {
                Some(r) => r,
                None => {
                    return Ok(ListRecordsResponse {
                        records: vec![],
                        cursor: None,
                    })
                }
            };

            let start: usize = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
            let end = (start + self.page_size).min(all_records.len());
            let page = all_records[start..end].to_vec();
            let next_cursor = if end < all_records.len() {
                Some(end.to_string())
            } else {
                None
            };

            Ok(ListRecordsResponse {
                records: page,
                cursor: next_cursor,
            })
        }

        async fn delete_record(&self, _did: &str, collection: &str, rkey: &str) -> Result<()> {
            self.deleted
                .lock()
                .unwrap()
                .push((collection.to_string(), rkey.to_string()));
            Ok(())
        }
    }

    fn make_record(rkey: &str, created_at: &str) -> Record {
        Record {
            uri: format!("at://{DID}/app.bsky.feed.post/{rkey}"),
            value: RecordValue {
                created_at: Some(created_at.to_string()),
            },
        }
    }

    fn make_record_for_collection(collection: &str, rkey: &str, created_at: &str) -> Record {
        Record {
            uri: format!("at://{DID}/{collection}/{rkey}"),
            value: RecordValue {
                created_at: Some(created_at.to_string()),
            },
        }
    }

    #[tokio::test]
    async fn deletes_posts_older_than_cutoff() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID).with_records(
            "app.bsky.feed.post",
            vec![make_record("abc123", &old_timestamp())],
        );

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert_eq!(
            fake.deleted(),
            vec![("app.bsky.feed.post".to_string(), "abc123".to_string())]
        );
    }

    #[tokio::test]
    async fn skips_posts_newer_than_cutoff() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID).with_records(
            "app.bsky.feed.post",
            vec![make_record("abc123", &recent_timestamp())],
        );

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert!(fake.deleted().is_empty());
    }

    #[tokio::test]
    async fn skips_pinned_post() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID)
            .with_pinned_post(PINNED_URI)
            .with_records(
                "app.bsky.feed.post",
                vec![make_record("pinned1", &old_timestamp())],
            );

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert!(fake.deleted().is_empty());
    }

    #[tokio::test]
    async fn deletes_pinned_when_configured() {
        tokio::time::pause();
        // When delete_pinned=true, get_pinned_post_uri is not called,
        // so the pinned_post value on the fake doesn't matter
        let fake = FakeBlueskyClient::new(DID).with_records(
            "app.bsky.feed.post",
            vec![make_record("pinned1", &old_timestamp())],
        );

        let mut config = config_with_cutoff_days_ago(30);
        config.delete_pinned = true;
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert_eq!(
            fake.deleted(),
            vec![("app.bsky.feed.post".to_string(), "pinned1".to_string())]
        );
    }

    #[tokio::test]
    async fn skips_keep_list_by_rkey() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID).with_records(
            "app.bsky.feed.post",
            vec![make_record("abc123", &old_timestamp())],
        );

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::from(["bluesky:abc123".to_string()]);
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert!(fake.deleted().is_empty());
    }

    #[tokio::test]
    async fn skips_keep_list_by_full_uri() {
        tokio::time::pause();
        let uri = format!("at://{DID}/app.bsky.feed.post/abc123");
        let fake = FakeBlueskyClient::new(DID).with_records(
            "app.bsky.feed.post",
            vec![make_record("abc123", &old_timestamp())],
        );

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::from([format!("bluesky:{uri}")]);
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert!(fake.deleted().is_empty());
    }

    #[tokio::test]
    async fn dry_run_does_not_call_delete() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID).with_records(
            "app.bsky.feed.post",
            vec![make_record("abc123", &old_timestamp())],
        );

        let mut config = config_with_cutoff_days_ago(30);
        config.dry_run = true;
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert!(fake.deleted().is_empty());
    }

    #[tokio::test]
    async fn pagination_follows_cursor() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID).with_page_size(1).with_records(
            "app.bsky.feed.post",
            vec![
                make_record("abc123", &old_timestamp()),
                make_record("def456", &old_timestamp()),
            ],
        );

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        let deleted = fake.deleted();
        assert_eq!(deleted.len(), 2);
        assert!(deleted.contains(&("app.bsky.feed.post".to_string(), "abc123".to_string())));
        assert!(deleted.contains(&("app.bsky.feed.post".to_string(), "def456".to_string())));
    }

    #[tokio::test]
    async fn respects_delete_reposts_and_likes_flags() {
        tokio::time::pause();
        let fake = FakeBlueskyClient::new(DID)
            .with_records(
                "app.bsky.feed.repost",
                vec![make_record_for_collection(
                    "app.bsky.feed.repost",
                    "repost1",
                    &old_timestamp(),
                )],
            )
            .with_records(
                "app.bsky.feed.like",
                vec![make_record_for_collection(
                    "app.bsky.feed.like",
                    "like1",
                    &old_timestamp(),
                )],
            );

        let mut config = config_with_cutoff_days_ago(30);
        config.delete_reposts = false;
        config.delete_likes = false;
        let keep_list = HashSet::new();
        delete_old_posts(&fake, "user", "pass", &config, &keep_list)
            .await
            .unwrap();

        assert!(fake.deleted().is_empty());
    }
}
