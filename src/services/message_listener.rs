use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
};

use tokio::sync::mpsc::Receiver;
use tokio::sync::mpsc::Sender;
use tracing::{error, info, warn};

use super::{discord_handler::DiscordMessage, summarizer::SummarizeRequest};

pub struct MessageLogService {
    summarize_tx: Sender<SummarizeRequest>,
    discord_rx: Receiver<DiscordMessage>,
    message_log_path: PathBuf,
    log_file_index: usize,
    curr_file_token_count: usize,
    message_log: File,
    summary_tokens_threshold: usize,
}

impl MessageLogService {
    pub fn new(
        message_log_path: PathBuf,
        summarize_tx: Sender<SummarizeRequest>,
        discord_rx: Receiver<DiscordMessage>,
        summary_tokens_threshold: usize,
    ) -> Self {
        let log_file_index: usize = find_last_log_file_index(&message_log_path).unwrap_or(0);
        info!("{}", log_file_index);
        let fpath = message_log_path.join(format!("messages_{log_file_index}.txt"));
        let message_log = OpenOptions::new()
            .append(true) // Set to append mode
            .create(true) // Create file if it does not exist
            .open(&fpath) // Specify the file path
            .expect("Unable to open messages log");

        let curr_file_token_count = crate::gpt::estimate_token_count(fpath)
            .expect("Could not estimate token count of file on init");
        Self {
            summarize_tx,
            discord_rx,
            message_log_path,
            log_file_index,
            curr_file_token_count,
            message_log,
            summary_tokens_threshold,
        }
    }

    pub async fn run(&mut self) {
        while let Some(data) = self.discord_rx.recv().await {
            match data {
                DiscordMessage::Received(msg) => {
                    // Check if the file has reached the critical mass, then figure out what we need to do:
                    // Have we reached the max tokens we want in our request? If so, then increase the log file index
                    // and emit a summarize request.
                    let incoming_token_count =
                        msg.content.chars().count() / crate::gpt::CHARS_PER_TOKEN;
                    if self.curr_file_token_count + incoming_token_count
                        > self.summary_tokens_threshold
                    {
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
