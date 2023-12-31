use std::collections::HashSet;
use std::env;
use std::sync::Arc;

use axum::routing::get;
use axum::{Extension, Router};
use dotenv::dotenv;
use futures::future::join_all;
use serenity::model::prelude::*;
use serenity::prelude::*;
use services::digests::DailyRecapService;
use services::discord_handler::Handler;
use services::message_listener::MessageLogService;
use services::summarizer::SummarizerService;
use tokio::task::{self, JoinError};
use tracing::{error, info};

mod config;
mod db;
mod gpt;
mod http_api;
mod services;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt::init();

    let token = env::var("DISCORD_BOT_SECRET").expect("No DISCORD_BOT_SECRET provided");
    let config = config::AppConfig::load_from_file("config.toml")?;
    _ = config;
    let messages_base = config.service.message_log_directory;

    // Initiate a connection to the database file, creating the file if required.
    let database = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(config.database.url)
                .create_if_missing(true),
        )
        .await
        .expect("Couldn't connect to database");

    // Run migrations, which updates the database's schema to the latest version.
    sqlx::migrate!("./migrations")
        .run(&database)
        .await
        .expect("Couldn't run database migrations");

    let shared_db = Arc::new(database);

    let mut tasks = vec![];

    let (summarize_tx, summarize_rx) = tokio::sync::mpsc::channel(100);
    let (discord_tx, discord_rx) = tokio::sync::mpsc::channel(100);

    let mut summary_srv =
        SummarizerService::new(messages_base.clone(), summarize_rx, shared_db.clone());
    tasks.push(task::spawn(async move {
        info!("Running summary service");
        summary_srv.run().await;
    }));

    let mut message_log_srv = MessageLogService::new(
        messages_base,
        summarize_tx,
        discord_rx,
        config.service.max_gpt_request_tokens,
    );
    tasks.push(task::spawn(async move {
        info!("Running message log service");
        message_log_srv.run().await;
    }));

    let mut daily_recap_srv = DailyRecapService::new(
        shared_db.clone(),
        config.service.produce_digest_interval_seconds,
    );
    tasks.push(task::spawn(async move {
        info!("Running daily digest service");
        daily_recap_srv.run().await;
    }));

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut discord_client = Client::builder(token, intents)
        .event_handler(Handler::new(discord_tx, HashSet::default()))
        .await
        .expect("Error creating client");

    tasks.push(task::spawn(async move {
        // The Serenity crate Will automatically attempt to reconnect, and will perform
        // exponential backoff until it reconnects.
        if let Err(why) = discord_client.start().await {
            error!("Client error: {why:?}");
        }
    }));

    let app = Router::new()
        .route("/summaries", get(http_api::summaries_handler))
        .route("/daily_digests", get(http_api::daily_digests_handler))
        .layer(Extension(shared_db));

    tasks.push(task::spawn(async move {
        info!("Serving http API on port {}", config.service.port);
        let listener = tokio::net::TcpListener::bind(format!(
            "{}:{}",
            config.service.host, config.service.port
        ))
        .await
        .unwrap();
        axum::serve(listener, app).await.unwrap();
    }));

    join_all(tasks)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, JoinError>>()?;
    Ok(())
}
