use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use axum::Extension;
use chrono::NaiveDateTime;
use serenity::async_trait;
use serenity::client::{Context, EventHandler};
use serenity::model::prelude::*;
use serenity::prelude::*;

use tracing::{error, info, warn};
use tracing_subscriber;

use dotenv::dotenv;
use futures::future::join_all;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::{self, JoinError};

use serde::{Deserialize, Serialize};
use serde_json::json;

use config::{Config, ConfigError};

#[derive(Deserialize)]
struct AppConfig {
    database: DatabaseConfig,
    service: ServiceConfig,
    discord: DiscordConfig,
}

#[derive(Deserialize)]
struct DatabaseConfig {
    url: String,
}

#[derive(Deserialize)]
struct ServiceConfig {
    interval_seconds: u64,
    message_log_directory: PathBuf,
}

#[derive(Deserialize)]
struct DiscordConfig {
    channel_ids: Vec<String>,
}

impl AppConfig {
    fn load_from_file(file_path: &str) -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::with_name(file_path))
            .build()?;

        config.try_deserialize::<Self>()
    }
}

struct Handler {
    tx: Sender<DiscordMessage>,
}

impl Handler {
    fn new(tx: Sender<DiscordMessage>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _: Context, msg: Message) {
        if let Err(e) = self.tx.send(DiscordMessage::Received(msg)).await {
            error!("Could not send received message tx over channel: {e}");
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
    }
}

#[derive(Deserialize, Debug)]
pub struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
pub struct Choice {
    message: GptMessage,
}

#[derive(Deserialize, Debug)]
pub struct GptMessage {
    content: String,
}

async fn summarize(text: &str) -> eyre::Result<String> {
    let client = reqwest::Client::new();
    let api_key = env::var("OPEN_AI_SECRET").expect("No OPEN_AI_SECRET provided");
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": "gpt-4",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a summarizer of large amount of content for a technical team. Summarize the following thoroughly:"
                },
                {
                    "role": "user",
                    "content": text,
                }
            ],
            "max_tokens": 4096,
        }))
        .send()
        .await?
        .json::<ChatCompletionResponse>()
        .await?;

    dbg!(&response);
    Ok(response.choices[0].message.content.clone())
}

enum DiscordMessage {
    Received(Message),
}

const MAX_REQUEST_TOKENS: usize = 2048;

struct MessageLogService {
    summarize_tx: Sender<SummarizeRequest>,
    discord_rx: Receiver<DiscordMessage>,
    message_log_path: PathBuf,
    log_file_index: usize,
    curr_file_token_count: usize,
    message_log: File,
}

impl MessageLogService {
    fn new(
        message_log_path: PathBuf,
        summarize_tx: Sender<SummarizeRequest>,
        discord_rx: Receiver<DiscordMessage>,
    ) -> Self {
        let log_file_index: usize = find_last_log_file_index(&message_log_path).unwrap_or(0);
        info!("{}", log_file_index);
        let fpath = message_log_path.join(format!("messages_{log_file_index}.txt"));
        let message_log = OpenOptions::new()
            .append(true) // Set to append mode
            .create(true) // Create file if it does not exist
            .open(&fpath) // Specify the file path
            .expect("Unable to open messages log");

        let curr_file_token_count =
            estimate_token_count(fpath).expect("Could not estimate token count of file on init");
        Self {
            summarize_tx,
            discord_rx,
            message_log_path,
            log_file_index,
            curr_file_token_count,
            message_log,
        }
    }

    async fn run(&mut self) {
        while let Some(data) = self.discord_rx.recv().await {
            match data {
                DiscordMessage::Received(msg) => {
                    // Check if the file has reached the critical mass, then figure out what we need to do:
                    // Have we reached the max tokens we want in our request? If so, then increase the log file index
                    // and emit a summarize request.
                    let incoming_token_count = msg.content.chars().count() / CHARS_PER_TOKEN;
                    if self.curr_file_token_count + incoming_token_count > MAX_REQUEST_TOKENS {
                        warn!("File has overflowed the allowed token count, creating new file");
                        let log_file_index = self.log_file_index + 1;
                        let fpath = self
                            .message_log_path
                            .join(format!("messages_{log_file_index}.txt"));
                        let message_log = OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(fpath)
                            .expect("Unable to open messages log"); // TODO: Handle panic.

                        // Send a request to summarize the previous, full file.
                        self.summarize_tx
                            .send(SummarizeRequest::FileWithIndex(self.log_file_index))
                            .await
                            .unwrap(); // TODO: Handle panic.

                        self.message_log = message_log;
                        self.log_file_index = log_file_index;
                        self.curr_file_token_count = 0;
                    }

                    let timestamp = msg.timestamp;
                    let content = msg.content;
                    let author = msg.author.name;
                    if let Err(e) = writeln!(
                        self.message_log,
                        "timestamp: {timestamp}, author: {author}, content: {content}"
                    ) {
                        error!("Could not write message with content: {content} to log file: {e}");
                        continue;
                    }
                    self.curr_file_token_count += incoming_token_count;
                    info!(
                        "Processed message, file has total token count of {}",
                        self.curr_file_token_count
                    );
                }
            }
        }
    }
}

fn find_last_log_file_index(dirpath: &PathBuf) -> Option<usize> {
    std::fs::read_dir(dirpath)
        .expect("Directory containing message logs not found")
        .filter_map(|entry| {
            entry.ok().and_then(|e| {
                e.path().file_name().and_then(|name| {
                    name.to_str().and_then(|s| {
                        if s.starts_with("messages_") && s.ends_with(".txt") {
                            s.trim_start_matches("messages_")
                                .trim_end_matches(".txt")
                                .parse::<usize>()
                                .ok()
                        } else {
                            None
                        }
                    })
                })
            })
        })
        .max()
}

const CHARS_PER_TOKEN: usize = 4;

fn estimate_token_count(fpath: PathBuf) -> io::Result<usize> {
    let contents = std::fs::read_to_string(fpath)?;
    let message_contents: Vec<String> = contents
        .lines()
        .filter_map(|line| line.split("content: ").nth(1))
        .map(|content| content.trim().to_string())
        .collect();

    let char_count = message_contents.join(" ").chars().count();
    Ok(char_count / CHARS_PER_TOKEN)
}

enum SummarizeRequest {
    FileWithIndex(usize),
}

struct SummarizerService {
    summarize_rx: Receiver<SummarizeRequest>,
    message_log_path: PathBuf,
    db: Arc<SqlitePool>,
}

impl SummarizerService {
    fn new(
        message_log_path: PathBuf,
        summarize_rx: Receiver<SummarizeRequest>,
        db: Arc<SqlitePool>,
    ) -> Self {
        Self {
            message_log_path,
            summarize_rx,
            db,
        }
    }
    async fn run(&mut self) {
        while let Some(data) = self.summarize_rx.recv().await {
            match data {
                SummarizeRequest::FileWithIndex(log_file_index) => {
                    info!("Summarizing contents of message log file with index {log_file_index}");
                    let fpath = self
                        .message_log_path
                        .join(format!("messages_{log_file_index}.txt"));
                    let file_contents = match std::fs::read_to_string(&fpath) {
                        Ok(f) => f,
                        Err(e) => {
                            error!("Could not read file to summarize: {e}");
                            continue;
                        }
                    };
                    let summary = match summarize(&file_contents).await {
                        Ok(txt) => txt,
                        Err(e) => {
                            error!("Could not summarize message log: {e}");
                            continue;
                        }
                    };
                    info!("Summary: {summary}");

                    // Save the summary to the DB.
                    if let Err(e) = insert_summary(&self.db, &summary).await {
                        error!("Could not insert summary to DB: {e}, contents: {summary}");
                        continue;
                    }
                    info!("Wrote the summary to the DB");

                    // Delete the file with index that it came from.
                    if let Err(e) = std::fs::remove_file(&fpath) {
                        error!("Could not delete file at path: {e}");
                    }

                    info!("Deleted summarized messages log file at path: {:?}", fpath);
                }
            }
        }
    }
}

use sqlx::sqlite::SqlitePool;
use std::time::Duration;
use tokio::time::interval;

pub struct DailyRecapService {
    db: Arc<SqlitePool>,
    interval: Duration,
}

impl DailyRecapService {
    fn new(db: Arc<SqlitePool>, interval_seconds: u64) -> Self {
        Self {
            db,
            interval: Duration::from_secs(interval_seconds),
        }
    }

    async fn run(&mut self) {
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
                    Summary,
                    "SELECT * FROM summaries WHERE timestamp >= ? ORDER BY timestamp ASC",
                    last_timestamp,
                )
                .fetch_all(&*self.db)
                .await
                .unwrap(),
                None => sqlx::query_as!(Summary, "SELECT * FROM summaries")
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
            let digest = match summarize(&summaries_content).await {
                Ok(txt) => txt,
                Err(e) => {
                    error!("Could not summarize daily digest: {e}");
                    continue;
                }
            };
            info!("Obtained a summarized daily digest: {digest}");
            if let Err(e) = insert_daily_digest(&self.db, digest, summary_ids).await {
                error!("Could not insert summarized daily digest into DB: {e}");
                continue;
            }
            info!("Saved daily digest to DB");
        }
    }
}

async fn insert_summary(pool: &SqlitePool, text: &str) -> Result<i64, Error> {
    let result = sqlx::query!(
        "INSERT INTO summaries (daily_digest_id, text) VALUES (?, ?)",
        None::<i64>,
        text
    )
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

use sqlx::Error;

async fn insert_daily_digest(
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
use axum::{routing::get, Json, Router};

#[derive(Serialize, Deserialize)]
struct Summary {
    id: i64,
    daily_digest_id: Option<i64>,
    text: String,
    timestamp: NaiveDateTime,
}

#[derive(Serialize, Deserialize)]
struct DailyDigestData {
    id: i64,
    text: String,
    timestamp: NaiveDateTime,
}

#[derive(Serialize, Deserialize)]
struct DailyDigest {
    id: i64,
    text: String,
    timestamp: NaiveDateTime,
    summaries: Vec<Summary>,
}

async fn fetch_summaries(pool: Arc<SqlitePool>) -> Vec<Summary> {
    sqlx::query_as!(Summary, "SELECT * FROM summaries")
        .fetch_all(&*pool)
        .await
        .unwrap_or_else(|_| vec![])
}

use futures::stream::{self, StreamExt};

async fn fetch_daily_digests(pool: Arc<SqlitePool>) -> Vec<DailyDigest> {
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

async fn summaries_handler(Extension(db): Extension<Arc<SqlitePool>>) -> Json<Vec<Summary>> {
    let summaries = fetch_summaries(db.clone()).await;
    Json(summaries)
}

async fn daily_digests_handler(
    Extension(db): Extension<Arc<SqlitePool>>,
) -> Json<Vec<DailyDigest>> {
    let digests = fetch_daily_digests(db.clone()).await;
    Json(digests)
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt::init();

    let token = env::var("DISCORD_BOT_SECRET").expect("No DISCORD_BOT_SECRET provided");
    let config = AppConfig::load_from_file("config.toml")?;
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

    let mut message_log_srv = MessageLogService::new(messages_base, summarize_tx, discord_rx);
    tasks.push(task::spawn(async move {
        info!("Running message log service");
        message_log_srv.run().await;
    }));

    let mut daily_recap_srv =
        DailyRecapService::new(shared_db.clone(), config.service.interval_seconds);
    tasks.push(task::spawn(async move {
        info!("Running daily digest service");
        daily_recap_srv.run().await;
    }));

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut discord_client = Client::builder(token, intents)
        .event_handler(Handler::new(discord_tx))
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
        .route("/summaries", get(summaries_handler))
        .route("/daily_digests", get(daily_digests_handler))
        .layer(Extension(shared_db));

    tasks.push(task::spawn(async move {
        info!("Serving http API on port 3000");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
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
