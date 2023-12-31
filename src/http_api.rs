use crate::db;

use axum::{Extension, Json};
use sqlx::SqlitePool;
use std::sync::Arc;

pub async fn summaries_handler(
    Extension(db): Extension<Arc<SqlitePool>>,
) -> Json<Vec<db::Summary>> {
    let summaries = db::fetch_summaries(db.clone()).await;
    Json(summaries)
}

pub async fn daily_digests_handler(
    Extension(db): Extension<Arc<SqlitePool>>,
) -> Json<Vec<db::DailyDigest>> {
    let digests = db::fetch_daily_digests(db.clone()).await;
    Json(digests)
}
