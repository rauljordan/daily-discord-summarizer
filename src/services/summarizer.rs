use std::{path::PathBuf, sync::Arc};

use sqlx::SqlitePool;
use tokio::sync::mpsc::Receiver;
use tracing::{error, info};

pub enum SummarizeRequest {
    FileWithIndex(usize),
}

pub struct SummarizerService {
    summarize_rx: Receiver<SummarizeRequest>,
    message_log_path: PathBuf,
    db: Arc<SqlitePool>,
}

impl SummarizerService {
    pub fn new(
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
    pub async fn run(&mut self) {
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
                    let summary = match crate::gpt::summarize(&file_contents).await {
                        Ok(txt) => txt,
                        Err(e) => {
                            error!("Could not summarize message log: {e}");
                            continue;
                        }
                    };
                    info!("Summary: {summary}");

                    // Save the summary to the DB.
                    if let Err(e) = crate::db::insert_summary(&self.db, &summary).await {
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
