use std::collections::HashSet;

use axum::async_trait;
use serenity::{
    all::{ChannelId, Message, Ready},
    client::{Context, EventHandler},
};
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

pub enum DiscordMessage {
    Received(Message),
}

pub struct Handler {
    tx: Sender<DiscordMessage>,
    allowed_channels: HashSet<ChannelId>,
}

impl Handler {
    pub fn new(tx: Sender<DiscordMessage>, allowed_channels: HashSet<ChannelId>) -> Self {
        Self {
            tx,
            allowed_channels,
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _: Context, msg: Message) {
        if !self.allowed_channels.contains(&msg.channel_id) {
            return;
        }
        if let Err(e) = self.tx.send(DiscordMessage::Received(msg)).await {
            error!("Could not send received message tx over channel: {e}");
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
    }
}
