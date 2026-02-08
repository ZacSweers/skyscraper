use crate::{is_protected, Config};
use anyhow::{Context, Result};
use log::{info, warn};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub(crate) struct Account {
    id: String,
}

#[derive(Deserialize, Clone)]
pub(crate) struct Status {
    id: String,
    created_at: String,
    #[serde(default)]
    pinned: bool,
    reblog: Option<serde_json::Value>,
}

pub(crate) trait MastodonClient {
    async fn verify_credentials(&self) -> Result<Account>;
    async fn list_statuses(&self, account_id: &str, max_id: Option<&str>) -> Result<Vec<Status>>;
    async fn delete_status(&self, id: &str) -> Result<()>;
    async fn list_favourites(&self, max_id: Option<&str>) -> Result<(Vec<Status>, Option<String>)>;
    async fn unfavourite(&self, id: &str) -> Result<()>;
}

pub(crate) struct HttpMastodonClient {
    client: reqwest::Client,
    instance: String,
    auth: String,
}

impl HttpMastodonClient {
    pub fn new(instance: &str, token: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("skyscraper/0.1.0")
                .build()
                .expect("Failed to build HTTP client"),
            instance: instance.to_string(),
            auth: format!("Bearer {token}"),
        }
    }
}

impl MastodonClient for HttpMastodonClient {
    async fn verify_credentials(&self) -> Result<Account> {
        self.client
            .get(format!(
                "{}/api/v1/accounts/verify_credentials",
                self.instance
            ))
            .header("Authorization", &self.auth)
            .send()
            .await?
            .error_for_status()
            .context("Failed to verify Mastodon credentials")?
            .json()
            .await
            .context("Failed to parse Mastodon credentials response")
    }

    async fn list_statuses(&self, account_id: &str, max_id: Option<&str>) -> Result<Vec<Status>> {
        let mut url = format!(
            "{}/api/v1/accounts/{}/statuses?limit=40",
            self.instance, account_id
        );
        if let Some(id) = max_id {
            url.push_str(&format!("&max_id={id}"));
        }

        self.client
            .get(&url)
            .header("Authorization", &self.auth)
            .send()
            .await?
            .error_for_status()
            .context("Failed to fetch Mastodon statuses")?
            .json()
            .await
            .context("Failed to parse Mastodon statuses response")
    }

    async fn delete_status(&self, id: &str) -> Result<()> {
        self.client
            .delete(format!("{}/api/v1/statuses/{}", self.instance, id))
            .header("Authorization", &self.auth)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn list_favourites(&self, max_id: Option<&str>) -> Result<(Vec<Status>, Option<String>)> {
        let mut url = format!("{}/api/v1/favourites?limit=40", self.instance);
        if let Some(id) = max_id {
            url.push_str(&format!("&max_id={id}"));
        }

        let resp = self
            .client
            .get(&url)
            .header("Authorization", &self.auth)
            .send()
            .await?
            .error_for_status()
            .context("Failed to fetch Mastodon favourites")?;

        let link_header = resp
            .headers()
            .get("link")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let favourites: Vec<Status> = resp.json().await?;
        let next_max_id = link_header.as_deref().and_then(parse_max_id_from_link);

        Ok((favourites, next_max_id))
    }

    async fn unfavourite(&self, id: &str) -> Result<()> {
        self.client
            .post(format!(
                "{}/api/v1/statuses/{}/unfavourite",
                self.instance, id
            ))
            .header("Authorization", &self.auth)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

pub async fn delete_old_posts(
    client: &(impl MastodonClient + Sync),
    config: &Config,
    keep_list: &HashSet<String>,
) -> Result<()> {
    // Verify credentials and get account ID
    let account = client.verify_credentials().await?;
    info!("Authenticated as account {}", account.id);

    let mut max_id: Option<String> = None;
    let mut deleted = 0u64;
    let mut skipped_pinned = 0u64;
    let mut skipped_kept = 0u64;
    let mut skipped_reposts = 0u64;

    loop {
        let statuses = client.list_statuses(&account.id, max_id.as_deref()).await?;

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

            // Skip reblogs if delete_reposts is disabled
            if status.reblog.is_some() && !config.delete_reposts {
                skipped_reposts += 1;
                continue;
            }

            if status.pinned && !config.delete_pinned {
                skipped_pinned += 1;
                warn!(
                    "Skipping pinned post: {}. To keep it permanently, add to your keep file: mastodon:{}",
                    status.id, status.id
                );
                continue;
            }

            if is_protected(keep_list, "mastodon", &status.id) {
                skipped_kept += 1;
                info!("Protected, skipping: {}", status.id);
                continue;
            }

            let label = if status.reblog.is_some() {
                "reblog"
            } else {
                "post"
            };

            if config.dry_run {
                info!(
                    "[DRY RUN] Would delete {label}: {} ({})",
                    status.id, status.created_at
                );
                deleted += 1;
                continue;
            }

            match client.delete_status(&status.id).await {
                Ok(()) => {
                    deleted += 1;
                    info!("Deleted {label}: {} ({})", status.id, status.created_at);
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("429") {
                        warn!("Rate limited — Mastodon allows 30 deletions per 30 minutes. Remaining posts will be cleaned up on the next run.");
                        break;
                    }
                    warn!("Failed to delete {}: {e}", status.id);
                }
            }

            // Mastodon rate-limits deletions to 30 per 30 minutes
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    info!(
        "Mastodon statuses: deleted {deleted}, skipped {skipped_pinned} pinned, skipped {skipped_kept} kept, skipped {skipped_reposts} reposts"
    );

    // Delete old favourites
    if config.delete_likes {
        let mut fav_max_id: Option<String> = None;
        let mut fav_deleted = 0u64;
        let mut fav_skipped_kept = 0u64;

        'favourites: loop {
            let (favourites, next_max_id) = match client
                .list_favourites(fav_max_id.as_deref())
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    warn!("Could not fetch favourites (token may lack read:favourites scope): {e}");
                    break;
                }
            };

            if favourites.is_empty() {
                break;
            }

            for status in &favourites {
                let post_time = match crate::parse_timestamp(&status.created_at) {
                    Ok(t) => t.with_timezone(&chrono::Utc),
                    Err(e) => {
                        warn!("Skipping favourite {}: {e}", status.id);
                        continue;
                    }
                };

                if post_time >= config.cutoff {
                    continue;
                }

                if is_protected(keep_list, "mastodon", &status.id) {
                    fav_skipped_kept += 1;
                    info!("Protected favourite, skipping: {}", status.id);
                    continue;
                }

                if config.dry_run {
                    info!(
                        "[DRY RUN] Would unfavourite: {} ({})",
                        status.id, status.created_at
                    );
                    fav_deleted += 1;
                    continue;
                }

                match client.unfavourite(&status.id).await {
                    Ok(()) => {
                        fav_deleted += 1;
                        info!("Unfavourited: {} ({})", status.id, status.created_at);
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("429") {
                            warn!("Rate limited — Mastodon allows 30 deletions per 30 minutes. Remaining favourites will be cleaned up on the next run.");
                            break 'favourites;
                        }
                        warn!("Failed to unfavourite {}: {e}", status.id);
                    }
                }

                // Mastodon rate-limits deletions to 30 per 30 minutes
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }

            fav_max_id = next_max_id;
            if fav_max_id.is_none() {
                break 'favourites;
            }
        }

        info!("Mastodon favourites: deleted {fav_deleted}, skipped {fav_skipped_kept} kept");
    }

    Ok(())
}

/// Parse `max_id` from a Mastodon Link header.
/// Example: `<https://instance/api/v1/favourites?max_id=123>; rel="next"`
fn parse_max_id_from_link(link: &str) -> Option<String> {
    for part in link.split(',') {
        if part.contains("rel=\"next\"") {
            // Extract URL between < and >
            let url = part.trim().strip_prefix('<')?.split('>').next()?;
            // Extract max_id param
            for param in url.split('?').nth(1)?.split('&') {
                if let Some(val) = param.strip_prefix("max_id=") {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use std::sync::Mutex;

    // --- parse_max_id_from_link unit tests ---

    #[test]
    fn parse_max_id_simple_next_link() {
        let link = r#"<https://instance/api/v1/favourites?max_id=123>; rel="next""#;
        assert_eq!(parse_max_id_from_link(link), Some("123".to_string()));
    }

    #[test]
    fn parse_max_id_prev_and_next() {
        let link = r#"<https://instance/api/v1/favourites?min_id=999>; rel="prev", <https://instance/api/v1/favourites?max_id=456>; rel="next""#;
        assert_eq!(parse_max_id_from_link(link), Some("456".to_string()));
    }

    #[test]
    fn parse_max_id_no_next_rel() {
        let link = r#"<https://instance/api/v1/favourites?min_id=999>; rel="prev""#;
        assert_eq!(parse_max_id_from_link(link), None);
    }

    #[test]
    fn parse_max_id_empty_string() {
        assert_eq!(parse_max_id_from_link(""), None);
    }

    #[test]
    fn parse_max_id_multiple_query_params() {
        let link = r#"<https://instance/api/v1/favourites?limit=40&max_id=789>; rel="next""#;
        assert_eq!(parse_max_id_from_link(link), Some("789".to_string()));
    }

    // --- Fake client ---

    struct FakeMastodonClient {
        account_id: String,
        statuses: Mutex<Vec<Status>>,
        favourites: Mutex<Vec<Status>>,
        deleted_statuses: Mutex<Vec<String>>,
        unfavourited: Mutex<Vec<String>>,
        page_size: usize,
    }

    impl FakeMastodonClient {
        fn new(account_id: &str) -> Self {
            Self {
                account_id: account_id.to_string(),
                statuses: Mutex::new(Vec::new()),
                favourites: Mutex::new(Vec::new()),
                deleted_statuses: Mutex::new(Vec::new()),
                unfavourited: Mutex::new(Vec::new()),
                page_size: 100,
            }
        }

        fn with_statuses(self, statuses: Vec<Status>) -> Self {
            *self.statuses.lock().unwrap() = statuses;
            self
        }

        fn with_favourites(self, favourites: Vec<Status>) -> Self {
            *self.favourites.lock().unwrap() = favourites;
            self
        }

        fn with_page_size(mut self, size: usize) -> Self {
            self.page_size = size;
            self
        }

        fn deleted_statuses(&self) -> Vec<String> {
            self.deleted_statuses.lock().unwrap().clone()
        }

        fn unfavourited(&self) -> Vec<String> {
            self.unfavourited.lock().unwrap().clone()
        }
    }

    impl MastodonClient for FakeMastodonClient {
        async fn verify_credentials(&self) -> Result<Account> {
            Ok(Account {
                id: self.account_id.clone(),
            })
        }

        async fn list_statuses(
            &self,
            _account_id: &str,
            max_id: Option<&str>,
        ) -> Result<Vec<Status>> {
            let all = self.statuses.lock().unwrap();
            // Statuses are stored in descending ID order
            // max_id means "return statuses with id < max_id"
            let filtered: Vec<Status> = match max_id {
                Some(mid) => all
                    .iter()
                    .filter(|s| s.id.as_str() < mid)
                    .cloned()
                    .collect(),
                None => all.to_vec(),
            };
            let page: Vec<Status> = filtered.into_iter().take(self.page_size).collect();
            Ok(page)
        }

        async fn delete_status(&self, id: &str) -> Result<()> {
            self.statuses.lock().unwrap().retain(|s| s.id != id);
            self.deleted_statuses.lock().unwrap().push(id.to_string());
            Ok(())
        }

        async fn list_favourites(
            &self,
            max_id: Option<&str>,
        ) -> Result<(Vec<Status>, Option<String>)> {
            let all = self.favourites.lock().unwrap();
            let filtered: Vec<Status> = match max_id {
                Some(mid) => all
                    .iter()
                    .filter(|s| s.id.as_str() < mid)
                    .cloned()
                    .collect(),
                None => all.to_vec(),
            };
            let page: Vec<Status> = filtered.iter().take(self.page_size).cloned().collect();
            let next_max_id = if filtered.len() > self.page_size {
                page.last().map(|s| s.id.clone())
            } else {
                None
            };
            Ok((page, next_max_id))
        }

        async fn unfavourite(&self, id: &str) -> Result<()> {
            self.favourites.lock().unwrap().retain(|s| s.id != id);
            self.unfavourited.lock().unwrap().push(id.to_string());
            Ok(())
        }
    }

    fn make_status(id: &str, created_at: &str, pinned: bool, reblog: bool) -> Status {
        Status {
            id: id.to_string(),
            created_at: created_at.to_string(),
            pinned,
            reblog: if reblog {
                Some(serde_json::json!({"id": "reblog_original"}))
            } else {
                None
            },
        }
    }

    // --- statuses tests ---

    #[tokio::test]
    async fn deletes_posts_older_than_cutoff() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &old_timestamp(),
            false,
            false,
        )]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert_eq!(fake.deleted_statuses(), vec!["1001"]);
    }

    #[tokio::test]
    async fn skips_posts_newer_than_cutoff() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &recent_timestamp(),
            false,
            false,
        )]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.deleted_statuses().is_empty());
    }

    #[tokio::test]
    async fn skips_pinned_post() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &old_timestamp(),
            true,
            false,
        )]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.deleted_statuses().is_empty());
    }

    #[tokio::test]
    async fn skips_reblog_when_delete_reposts_false() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &old_timestamp(),
            false,
            true,
        )]);

        let mut config = config_with_cutoff_days_ago(30);
        config.delete_reposts = false;
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.deleted_statuses().is_empty());
    }

    #[tokio::test]
    async fn deletes_reblog_when_delete_reposts_true() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &old_timestamp(),
            false,
            true,
        )]);

        let config = config_with_cutoff_days_ago(30); // delete_reposts defaults to true
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert_eq!(fake.deleted_statuses(), vec!["1001"]);
    }

    #[tokio::test]
    async fn skips_keep_list_post() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &old_timestamp(),
            false,
            false,
        )]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::from(["mastodon:1001".to_string()]);
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.deleted_statuses().is_empty());
    }

    #[tokio::test]
    async fn dry_run_does_not_call_delete() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_statuses(vec![make_status(
            "1001",
            &old_timestamp(),
            false,
            false,
        )]);

        let mut config = config_with_cutoff_days_ago(30);
        config.dry_run = true;
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.deleted_statuses().is_empty());
    }

    #[tokio::test]
    async fn pagination_follows_max_id() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345")
            .with_page_size(1)
            .with_statuses(vec![
                make_status("1001", &old_timestamp(), false, false),
                make_status("1000", &old_timestamp(), false, false),
            ]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        let deleted = fake.deleted_statuses();
        assert_eq!(deleted.len(), 2);
        assert!(deleted.contains(&"1001".to_string()));
        assert!(deleted.contains(&"1000".to_string()));
    }

    // --- favourites tests ---

    #[tokio::test]
    async fn unfavourites_old_likes() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_favourites(vec![make_status(
            "2001",
            &old_timestamp(),
            false,
            false,
        )]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert_eq!(fake.unfavourited(), vec!["2001"]);
    }

    #[tokio::test]
    async fn skips_likes_when_delete_likes_false() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_favourites(vec![make_status(
            "2001",
            &old_timestamp(),
            false,
            false,
        )]);

        let mut config = config_with_cutoff_days_ago(30);
        config.delete_likes = false;
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.unfavourited().is_empty());
    }

    #[tokio::test]
    async fn favourites_pagination_via_link_header() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345")
            .with_page_size(1)
            .with_favourites(vec![
                make_status("2001", &old_timestamp(), false, false),
                make_status("2000", &old_timestamp(), false, false),
            ]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::new();
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        let unfavd = fake.unfavourited();
        assert_eq!(unfavd.len(), 2);
        assert!(unfavd.contains(&"2001".to_string()));
        assert!(unfavd.contains(&"2000".to_string()));
    }

    #[tokio::test]
    async fn favourites_respects_keep_list() {
        tokio::time::pause();
        let fake = FakeMastodonClient::new("12345").with_favourites(vec![make_status(
            "2001",
            &old_timestamp(),
            false,
            false,
        )]);

        let config = config_with_cutoff_days_ago(30);
        let keep_list = HashSet::from(["mastodon:2001".to_string()]);
        delete_old_posts(&fake, &config, &keep_list).await.unwrap();

        assert!(fake.unfavourited().is_empty());
    }
}
