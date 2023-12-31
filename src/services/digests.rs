use crate::{db, gpt};

use chrono::NaiveDateTime;
use sqlx::sqlite::SqlitePool;
use std::{sync::Arc, time::Duration};
use tokio::time::interval;
use tracing::{error, info};

pub struct DailyRecapService {
    db: Arc<SqlitePool>,
    interval: Duration,
}

impl DailyRecapService {
    pub fn new(db: Arc<SqlitePool>, interval_seconds: u64) -> Self {
        Self {
            db,
            interval: Duration::from_secs(interval_seconds),
        }
    }

    pub async fn run(&mut self) {
        let mut interval_timer = interval(self.interval);

        loop {
            interval_timer.tick().await;
            // Perform your task here
            info!("Running daily recap of summaries...");

            // Here, we should decide whether to fetch all summaries or only those after the last recap.
            let last_recap: Option<(i32, NaiveDateTime)> =
                sqlx::query_as::<_, (i32, NaiveDateTime)>(
                    "SELECT id, timestamp FROM daily_digests ORDER BY timestamp DESC LIMIT 1",
                )
                .fetch_optional(&*self.db)
                .await
                .unwrap(); // Handle this error properly in production code

            let summaries = match last_recap {
                Some((_, last_timestamp)) => sqlx::query_as!(
                    db::Summary,
                    "SELECT * FROM summaries WHERE timestamp >= ? ORDER BY timestamp ASC",
                    last_timestamp,
                )
                .fetch_all(&*self.db)
                .await
                .unwrap(),
                None => sqlx::query_as!(db::Summary, "SELECT * FROM summaries")
                    .fetch_all(&*self.db)
                    .await
                    .unwrap(),
            };

            if summaries.is_empty() {
                info!("No summaries to recap");
                continue;
            }
            let summary_ids: Vec<i64> = summaries.iter().map(|s| s.id).collect();

            let summaries_content: Vec<String> = summaries.into_iter().map(|s| s.text).collect();
            let summaries_content = summaries_content.join(" ");
            let digest = match gpt::summarize(&summaries_content).await {
                Ok(txt) => txt,
                Err(e) => {
                    error!("Could not summarize daily digest: {e}");
                    continue;
                }
            };
            info!("Obtained a summarized daily digest: {digest}");
            if let Err(e) = db::insert_daily_digest(&self.db, digest, summary_ids).await {
                error!("Could not insert summarized daily digest into DB: {e}");
                continue;
            }
            info!("Saved daily digest to DB");
        }
    }
}
