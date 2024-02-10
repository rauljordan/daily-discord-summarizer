use chrono::NaiveDateTime;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::{Error, SqlitePool};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Summary {
    pub id: i64,
    pub daily_digest_id: Option<i64>,
    pub text: String,
    pub timestamp: NaiveDateTime,
}

#[derive(Serialize, Deserialize)]
pub struct DailyDigestData {
    pub id: i64,
    pub text: String,
    pub timestamp: NaiveDateTime,
}

#[derive(Serialize, Deserialize)]
pub struct DailyDigest {
    pub id: i64,
    pub text: String,
    pub timestamp: NaiveDateTime,
    pub summaries: Vec<Summary>,
}

pub async fn fetch_summaries(pool: Arc<SqlitePool>) -> Vec<Summary> {
    sqlx::query_as!(Summary, "SELECT * FROM summaries")
        .fetch_all(&*pool)
        .await
        .unwrap_or_else(|_| vec![])
}

pub async fn insert_summary(pool: &SqlitePool, text: &str) -> Result<i64, Error> {
    let result = sqlx::query!(
        "INSERT INTO summaries (daily_digest_id, text) VALUES (?, ?)",
        None::<i64>,
        text
    )
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn fetch_daily_digests(pool: Arc<SqlitePool>) -> Vec<DailyDigest> {
    let digests = sqlx::query_as!(
        DailyDigestData,
        "SELECT id, text, timestamp FROM daily_digests"
    )
    .fetch_all(&*pool)
    .await
    .unwrap_or_else(|_| vec![]);

    stream::iter(digests)
        .then(|digest| {
            let pool_clone = pool.clone();
            async move {
                let summaries = sqlx::query_as!(
                    Summary,
                    "SELECT * FROM summaries WHERE daily_digest_id = ?",
                    digest.id
                )
                .fetch_all(&*pool_clone)
                .await
                .unwrap_or_else(|_| vec![]);

                DailyDigest {
                    id: digest.id,
                    text: digest.text,
                    timestamp: digest.timestamp,
                    summaries,
                }
            }
        })
        .collect::<Vec<DailyDigest>>()
        .await
}

pub async fn insert_daily_digest(
    pool: &SqlitePool,
    digest_text: String,
    summary_ids: Vec<i64>,
) -> Result<(), Error> {
    let mut transaction = pool.begin().await?;

    // Insert the new digest and get its ID
    let digest_id: i64 = sqlx::query!("INSERT INTO daily_digests (text) VALUES (?)", digest_text)
        .execute(&mut *transaction)
        .await?
        .last_insert_rowid();

    // Update each summary to link it to the new digest
    for summary_id in summary_ids {
        sqlx::query!(
            "UPDATE summaries SET daily_digest_id = ? WHERE id = ?",
            digest_id,
            summary_id
        )
        .execute(&mut *transaction)
        .await?;
    }

    // Commit the transaction
    transaction.commit().await?;
    Ok(())
}

pub async fn fetch_latest_summaries(
    pool: Arc<SqlitePool>,
    count: usize,
    page: usize,
) -> Vec<Summary> {
    let offset = count * (page - 1);
    sqlx::query_as!(
        Summary,
        "SELECT * FROM summaries ORDER BY timestamp DESC LIMIT ? OFFSET ?",
        count as i64,
        offset as i64
    )
    .fetch_all(&*pool)
    .await
    .unwrap_or_else(|_| vec![])
}
