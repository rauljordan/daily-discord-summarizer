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

use axum::extract::Query;
use serde::Deserialize;

#[derive(Deserialize)]
struct SummariesQueryParams {
    count: usize, // Number of summaries to fetch
    page: usize,  // Page number for pagination
}

pub async fn fetch_latest_summaries_handler(
    Query(params): Query<SummariesQueryParams>,
    Extension(db): Extension<Arc<SqlitePool>>,
) -> Json<Vec<db::Summary>> {
    let summaries = db::fetch_latest_summaries(db.clone(), params.count, params.page).await;
    Json(summaries)
}
