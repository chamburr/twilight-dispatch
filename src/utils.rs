use crate::{
    cache,
    config::CONFIG,
    constants::{SESSIONS_KEY, SHARDS_KEY},
    models::{ApiResult, SessionInfo},
};

use serde::Serialize;
use simd_json::owned::Value;
use std::{collections::HashMap, sync::Arc};
use time::{Format, OffsetDateTime};
use tracing::warn;
use twilight_gateway::{
    cluster::ShardScheme,
    queue::{LargeBotQueue, LocalQueue, Queue},
    shard::ResumeSession,
    Cluster, EventTypeFlags, Intents,
};
use twilight_http::Client;
use twilight_model::{
    channel::embed::Embed,
    gateway::{
        payload::update_status::UpdateStatusInfo,
        presence::{Activity, UserOrId},
    },
    guild::GuildStatus,
    id::{ChannelId, GuildId, UserId},
};

#[inline]
pub fn get_user_id(user: &UserOrId) -> UserId {
    match user {
        UserOrId::User(u) => u.id,
        UserOrId::UserId { id } => *id,
    }
}

#[inline]
pub fn get_guild_status_id(guild_status: &GuildStatus) -> GuildId {
    match guild_status {
        GuildStatus::Online(g) => g.id,
        GuildStatus::Offline(g) => g.id,
    }
}

pub async fn get_clusters(
    resumes: HashMap<u64, ResumeSession>,
    queue: Arc<Box<dyn Queue>>,
) -> ApiResult<Vec<Cluster>> {
    let shards = get_shards();
    let base = shards / CONFIG.clusters;
    let extra = shards % CONFIG.clusters;

    let mut clusters = vec![];
    let mut last_index = CONFIG.shards_start;

    for i in 0..CONFIG.clusters {
        let index = if i < extra {
            last_index + base
        } else {
            last_index + base - 1
        };

        let cluster = Cluster::builder(
            CONFIG.bot_token.clone(),
            Intents::from_bits(CONFIG.intents).unwrap(),
        )
        .gateway_url(Some("wss://gateway.discord.gg".to_owned()))
        .shard_scheme(ShardScheme::Range {
            from: last_index,
            to: index,
            total: CONFIG.shards_total,
        })
        .queue(queue.clone())
        .presence(UpdateStatusInfo::new(
            vec![Activity {
                application_id: None,
                assets: None,
                created_at: None,
                details: None,
                emoji: None,
                flags: None,
                id: None,
                instance: None,
                kind: CONFIG.activity_type,
                name: CONFIG.activity_name.clone(),
                party: None,
                secrets: None,
                state: None,
                timestamps: None,
                url: None,
            }],
            false,
            None,
            CONFIG.status,
        ))
        .large_threshold(CONFIG.large_threshold)?
        .resume_sessions(resumes.clone())
        .build()
        .await?;

        clusters.push(cluster);

        last_index = index + 1;
    }

    Ok(clusters)
}

pub async fn get_queue() -> Arc<Box<dyn Queue>> {
    let concurrency = CONFIG.shards_concurrency as usize;
    if concurrency == 1 {
        Arc::new(Box::new(LocalQueue::new()))
    } else {
        let client = Client::new(CONFIG.bot_token.clone());
        Arc::new(Box::new(LargeBotQueue::new(concurrency, &client).await))
    }
}

pub async fn get_resume_sessions(
    conn: &mut redis::aio::Connection,
) -> ApiResult<HashMap<u64, ResumeSession>> {
    let shards: u64 = cache::get(conn, SHARDS_KEY).await?.unwrap_or_default();
    if shards != CONFIG.shards_total || !CONFIG.resume {
        return Ok(HashMap::new());
    }

    let sessions: HashMap<String, SessionInfo> =
        cache::get(conn, SESSIONS_KEY).await?.unwrap_or_default();

    Ok(sessions
        .into_iter()
        .map(|(k, v)| {
            (
                k.parse().unwrap(),
                ResumeSession {
                    session_id: v.session_id,
                    sequence: v.sequence,
                },
            )
        })
        .collect())
}

pub fn get_event_flags() -> EventTypeFlags {
    let mut event_flags = EventTypeFlags::GATEWAY_HELLO
        | EventTypeFlags::GATEWAY_INVALIDATE_SESSION
        | EventTypeFlags::GATEWAY_RECONNECT
        | EventTypeFlags::READY
        | EventTypeFlags::RESUMED
        | EventTypeFlags::SHARD_CONNECTED
        | EventTypeFlags::SHARD_CONNECTING
        | EventTypeFlags::SHARD_DISCONNECTED
        | EventTypeFlags::SHARD_IDENTIFYING
        | EventTypeFlags::SHARD_PAYLOAD
        | EventTypeFlags::SHARD_RECONNECTING
        | EventTypeFlags::SHARD_RESUMING;

    if CONFIG.state_enabled {
        event_flags |= EventTypeFlags::CHANNEL_CREATE
            | EventTypeFlags::CHANNEL_DELETE
            | EventTypeFlags::CHANNEL_PINS_UPDATE
            | EventTypeFlags::CHANNEL_UPDATE
            | EventTypeFlags::GUILD_CREATE
            | EventTypeFlags::GUILD_DELETE
            | EventTypeFlags::GUILD_EMOJIS_UPDATE
            | EventTypeFlags::GUILD_UPDATE
            | EventTypeFlags::ROLE_CREATE
            | EventTypeFlags::ROLE_DELETE
            | EventTypeFlags::ROLE_UPDATE
            | EventTypeFlags::UNAVAILABLE_GUILD
            | EventTypeFlags::USER_UPDATE
            | EventTypeFlags::VOICE_STATE_UPDATE;

        if CONFIG.state_member {
            event_flags |= EventTypeFlags::MEMBER_ADD
                | EventTypeFlags::MEMBER_REMOVE
                | EventTypeFlags::MEMBER_CHUNK
                | EventTypeFlags::MEMBER_UPDATE;

            if CONFIG.state_presence {
                event_flags |= EventTypeFlags::PRESENCE_UPDATE;
            }
        }

        if CONFIG.state_message {
            event_flags |= EventTypeFlags::MESSAGE_CREATE
                | EventTypeFlags::MESSAGE_DELETE
                | EventTypeFlags::MESSAGE_DELETE_BULK
                | EventTypeFlags::MESSAGE_UPDATE;
        }
    }

    event_flags
}

pub async fn log_discord(cluster: &Cluster, color: usize, message: impl Into<String>) {
    let client = cluster.config().http_client();

    let message = client
        .create_message(ChannelId(CONFIG.log_channel))
        .embed(Embed {
            author: None,
            color: Some(color as u32),
            description: None,
            fields: vec![],
            footer: None,
            image: None,
            kind: "".to_owned(),
            provider: None,
            thumbnail: None,
            timestamp: Some(OffsetDateTime::now_utc().format(Format::Rfc3339)),
            title: Some(message.into()),
            url: None,
            video: None,
        });

    if let Ok(message) = message {
        if let Err(err) = message.await {
            warn!("Failed to post message to Discord: {:?}", err)
        }
    }
}

pub async fn log_discord_guild(
    cluster: &Cluster,
    color: usize,
    title: impl Into<String>,
    message: impl Into<String>,
) {
    let client = cluster.config().http_client();

    let message = client
        .create_message(ChannelId(CONFIG.log_guild_channel))
        .embed(Embed {
            author: None,
            color: Some(color as u32),
            description: Some(message.into()),
            fields: vec![],
            footer: None,
            image: None,
            kind: "".to_owned(),
            provider: None,
            thumbnail: None,
            timestamp: Some(OffsetDateTime::now_utc().format(Format::Rfc3339)),
            title: Some(title.into()),
            url: None,
            video: None,
        });

    if let Ok(message) = message {
        if let Err(err) = message.await {
            warn!("Failed to post message to Discord: {:?}", err)
        }
    }
}

pub fn get_shards() -> u64 {
    CONFIG.shards_end - CONFIG.shards_start + 1
}

pub fn get_guild_shard(guild_id: GuildId) -> u64 {
    (guild_id.0 >> 22) % CONFIG.shards_total
}

pub fn to_value<T>(value: &T) -> ApiResult<Value>
where
    T: Serialize + ?Sized,
{
    let mut bytes = simd_json::to_vec(value)?;
    let result = simd_json::owned::to_value(bytes.as_mut_slice())?;

    Ok(result)
}

pub fn get_keys(value: impl ToString) -> Vec<String> {
    return value
        .to_string()
        .split(':')
        .map(|part| part.to_owned())
        .collect();
}
