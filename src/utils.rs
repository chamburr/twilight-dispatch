use crate::cache;
use crate::config::get_config;
use crate::constants::{SESSIONS_KEY, SHARDS_KEY};
use crate::models::{ApiResult, SessionInfo};

use serde::Serialize;
use simd_json::owned::Value;
use std::collections::HashMap;
use std::sync::Arc;
use time::{Format, OffsetDateTime};
use tracing::warn;
use twilight_gateway::cluster::ShardScheme;
use twilight_gateway::queue::{LargeBotQueue, LocalQueue, Queue};
use twilight_gateway::shard::ResumeSession;
use twilight_gateway::Cluster;
use twilight_http::Client;
use twilight_model::channel::embed::Embed;
use twilight_model::gateway::payload::update_status::UpdateStatusInfo;
use twilight_model::gateway::presence::Activity;
use twilight_model::id::{ChannelId, GuildId};

pub fn get_gateway_url() -> ApiResult<String> {
    let config = get_config();
    let version = config.api_version;

    if version == 8 {
        Ok("wss://gateway.discord.gg".to_owned())
    } else {
        Ok(format!(
            "wss://gateway.discord.gg?v={}&compress=zlib-stream#",
            version
        ))
    }
}

pub fn get_shard_scheme() -> ApiResult<ShardScheme> {
    let config = get_config();
    Ok(ShardScheme::Range {
        from: config.shards_start,
        to: config.shards_end,
        total: config.shards_total,
    })
}

pub async fn get_queue() -> ApiResult<Arc<Box<dyn Queue>>> {
    let config = get_config();
    let concurrency = config.shards_concurrency as usize;

    if concurrency == 1 {
        Ok(Arc::new(Box::new(LocalQueue::new())))
    } else {
        let client = Client::new(config.bot_token);
        Ok(Arc::new(Box::new(
            LargeBotQueue::new(concurrency, &client).await,
        )))
    }
}

pub fn get_update_status_info() -> ApiResult<UpdateStatusInfo> {
    let config = get_config();

    Ok(UpdateStatusInfo::new(
        vec![Activity {
            application_id: None,
            assets: None,
            created_at: None,
            details: None,
            emoji: None,
            flags: None,
            id: None,
            instance: None,
            kind: config.activity_type,
            name: config.activity_name,
            party: None,
            secrets: None,
            state: None,
            timestamps: None,
            url: None,
        }],
        false,
        None,
        config.status,
    ))
}

pub async fn get_resume_sessions(
    conn: &mut redis::aio::Connection,
) -> ApiResult<HashMap<u64, ResumeSession>> {
    let config = get_config();

    let shards: u64 = cache::get(conn, SHARDS_KEY).await?.unwrap_or_default();
    if shards != config.shards_total || !config.resume {
        return Ok(HashMap::new());
    }

    let sessions: HashMap<String, SessionInfo> =
        cache::get(conn, SESSIONS_KEY).await?.unwrap_or_default();

    Ok(sessions
        .iter()
        .map(|(k, v)| {
            (
                k.parse().unwrap(),
                ResumeSession {
                    session_id: v.session_id.clone(),
                    sequence: v.sequence,
                },
            )
        })
        .collect())
}

pub async fn log_discord(cluster: &Cluster, color: usize, message: impl Into<String>) {
    let config = get_config();
    let client = cluster.config().http_client();

    let message = client
        .create_message(ChannelId(config.log_channel))
        .embed(Embed {
            author: None,
            color: Some(color as u32),
            description: None,
            fields: vec![],
            footer: None,
            image: None,
            kind: "".to_string(),
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

pub fn get_guild_shard(guild_id: GuildId) -> u64 {
    (guild_id.0 >> 22) % get_config().shards_total
}

pub fn to_value<T>(value: &T) -> ApiResult<Value>
where
    T: Serialize + ?Sized,
{
    let mut bytes = simd_json::to_vec(value)?;
    let result = simd_json::owned::to_value(bytes.as_mut_slice())?;

    Ok(result)
}
