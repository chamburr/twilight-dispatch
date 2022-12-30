use crate::{
    config::CONFIG,
    constants::{EXCHANGE, QUEUE_RECV, QUEUE_SEND},
    models::ApiResult,
};

use lapin::{
    options::{ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    Channel, ExchangeKind,
};
use lazy_static::lazy_static;
use serde::Serialize;
use simd_json::owned::Value;
use time::OffsetDateTime;
use tracing::warn;
use twilight_http::client::Client;
use twilight_model::{
    channel::message::Embed, id::Id, user::CurrentUser, util::datetime::Timestamp,
};

lazy_static! {
    static ref CLIENT: Client = Client::new(CONFIG.bot_token.clone());
}

pub async fn declare_queues(channel: &Channel, channel_send: &Channel) -> ApiResult<()> {
    channel
        .exchange_declare(
            EXCHANGE,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                passive: false,
                durable: true,
                auto_delete: false,
                internal: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;
    channel_send
        .queue_declare(
            QUEUE_SEND,
            QueueDeclareOptions {
                passive: false,
                durable: true,
                exclusive: false,
                auto_delete: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;

    if CONFIG.default_queue {
        channel
            .queue_declare(
                QUEUE_RECV,
                QueueDeclareOptions {
                    passive: false,
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    nowait: false,
                },
                FieldTable::default(),
            )
            .await?;

        channel
            .queue_bind(
                QUEUE_RECV,
                EXCHANGE,
                "#",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;
    }

    Ok(())
}

pub async fn get_current_user() -> ApiResult<CurrentUser> {
    let user = CLIENT.current_user().await?.model().await?;

    Ok(user)
}

pub fn log_discord(color: usize, message: impl Into<String>) {
    if CONFIG.log_channel == 0 {
        return;
    }

    let message = message.into();

    tokio::spawn(async move {
        let embeds = &[Embed {
            author: None,
            color: Some(color as u32),
            description: None,
            fields: vec![],
            footer: None,
            image: None,
            kind: "".to_owned(),
            provider: None,
            thumbnail: None,
            timestamp: Some(
                Timestamp::from_secs(OffsetDateTime::now_utc().unix_timestamp()).unwrap(),
            ),
            title: Some(message),
            url: None,
            video: None,
        }];

        let message = CLIENT
            .create_message(Id::new(CONFIG.log_channel))
            .embeds(embeds);

        if let Ok(message) = message {
            if let Err(err) = message.await {
                warn!("Failed to post message to Discord: {:?}", err)
            }
        }
    });
}

pub fn log_discord_guild(color: usize, title: impl Into<String>, message: impl Into<String>) {
    if CONFIG.log_guild_channel == 0 {
        return;
    }

    let title = title.into();
    let message = message.into();

    tokio::spawn(async move {
        let embeds = &[Embed {
            author: None,
            color: Some(color as u32),
            description: Some(message),
            fields: vec![],
            footer: None,
            image: None,
            kind: "".to_owned(),
            provider: None,
            thumbnail: None,
            timestamp: Some(
                Timestamp::from_secs(OffsetDateTime::now_utc().unix_timestamp()).unwrap(),
            ),
            title: Some(title),
            url: None,
            video: None,
        }];

        let message = CLIENT
            .create_message(Id::new(CONFIG.log_guild_channel))
            .embeds(embeds);

        if let Ok(message) = message {
            if let Err(err) = message.await {
                warn!("Failed to post message to Discord: {:?}", err)
            }
        }
    });
}

pub fn to_value<T>(value: &T) -> ApiResult<Value>
where
    T: Serialize + ?Sized,
{
    let mut bytes = simd_json::to_vec(value)?;
    let result = simd_json::owned::to_value(bytes.as_mut_slice())?;

    Ok(result)
}
