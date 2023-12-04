# Daily Discord Summarizer

This PR implements a bot that listens for public Discord messages in a server and performs **summaries of all of them** using chat GPT-4. It also exposes an HTTP JSON API that allows for reading these daily digests from a sqlite database.

## How it Works

- The bot listens for all messages sent in a Discord server, and aggregates them locally
- Once the total amount of content in the messages hits a threshold, it summaries them using GPT-4 and stores these summaries in a DB
- At a configurable interval, it takes all the summaries and produces a total summary of them, called a `digest`. This can be configured to run daily to produce daily digests of what's happening in a Discord server

## Installing

**Requirements**

- Rust 1.74.0
- `OPEN_AI_SECRET` env var: Open AI API key
- `DISCORD_BOT_SECRET` env var: Discord bot secret key with "read messages permissions"

**Configuring**

Edit the `cargo.toml` file with the following values:

```toml
[database]
url = "db.sqlite" # your sqlite database url

[service]
# How often to create a single digest summary of all summaries
produce_digest_interval_seconds = 10800 # Default of every 3 hours
# Where to store message logs, ensure this dir exists
message_log_directory = "messages"
# Http api port
port = 3000
# Http api host
host = "127.0.0.1"
# Number of max request tokens in chat gpt api calls. The max allowed by GPT-4 is 4096
# including the response tokens. So here, we want to leave room for the response
max_gpt_request_tokens = 2048
```

You can use a `.env` file to store your Open AI and Discord bot secrets, or set them as env vars before running.

```
DISCORD_BOT_SECRET=...
OPEN_AI_SECRET=...
```

## Running

`mkdir messages && cargo build --release` and then:

```
./target/release/daily-discord-summarizer
```

## API

Summaries are available via an HTTP JSON API on port 3000 by default:

- `/summaries` retrieves all summaries created by chat GPT-4
- `/daily_digests` retrieves all digests from the database, along with all their associated summaries

## License

This project is licensed under either of

- Apache License, Version 2.0 (licenses/Apache-2.0)
- MIT license (licenses/MIT)

at your option.

The SPDX license identifier for this project is MIT OR Apache-2.0.