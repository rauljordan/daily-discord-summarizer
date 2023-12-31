use serde::Deserialize;
use serde_json::json;
use std::env;
use std::io;
use std::path::PathBuf;

pub const CHARS_PER_TOKEN: usize = 4;

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

pub async fn summarize(text: &str) -> eyre::Result<String> {
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

pub fn estimate_token_count(fpath: PathBuf) -> io::Result<usize> {
    let contents = std::fs::read_to_string(fpath)?;
    let message_contents: Vec<String> = contents
        .lines()
        .filter_map(|line| line.split("content: ").nth(1))
        .map(|content| content.trim().to_string())
        .collect();

    let char_count = message_contents.join(" ").chars().count();
    Ok(char_count / CHARS_PER_TOKEN)
}
