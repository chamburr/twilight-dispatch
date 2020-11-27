use crate::cache;
use crate::constants::{SESSIONS_KEY, SHARDS_KEY};
use crate::models::{ApiResult, SessionInfo};

use chrono::Utc;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tracing::warn;
use twilight_gateway::cluster::ShardScheme;
use twilight_gateway::queue::{LargeBotQueue, LocalQueue, Queue};
use twilight_gateway::shard::ResumeSession;
use twilight_gateway::Cluster;
use twilight_http::Client;
use twilight_model::channel::embed::Embed;
use twilight_model::gateway::payload::update_status::UpdateStatusInfo;
use twilight_model::gateway::presence::{Activity, ActivityType, Status};
use twilight_model::id::ChannelId;

pub fn get_gateway_url() -> ApiResult<String> {
    let version = env::var("API_VERSION")?;

    if version == "8" {
        Ok("wss://gateway.discord.gg".to_owned())
    } else {
        Ok(format!(
            "wss://gateway.discord.gg?v={}&compress=zlib-stream#",
            version
        ))
    }
}

pub fn get_shard_scheme() -> ApiResult<ShardScheme> {
    Ok(ShardScheme::Range {
        from: env::var("SHARDS_START")?.parse()?,
        to: env::var("SHARDS_END")?.parse()?,
        total: env::var("SHARDS_TOTAL")?.parse()?,
    })
}

pub async fn get_queue() -> ApiResult<Arc<Box<dyn Queue>>> {
    let concurrency: usize = env::var("SHARDS_CONCURRENCY")?.parse()?;

    if concurrency == 1 {
        Ok(Arc::new(Box::new(LocalQueue::new())))
    } else {
        let client = Client::new(env::var("BOT_TOKEN")?);
        Ok(Arc::new(Box::new(
            LargeBotQueue::new(concurrency, &client).await,
        )))
    }
}

pub fn get_update_status_info() -> ApiResult<UpdateStatusInfo> {
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
            kind: match env::var("ACTIVITY_TYPE")?.as_str() {
                "playing" => ActivityType::Playing,
                "watching" => ActivityType::Watching,
                "listening" => ActivityType::Listening,
                "streaming" => ActivityType::Streaming,
                "competing" => ActivityType::Competing,
                _ => panic!("Invalid ACTIVITY_TYPE provided"),
            },
            name: env::var("ACTIVITY_NAME")?,
            party: None,
            secrets: None,
            state: None,
            timestamps: None,
            url: None,
        }],
        false,
        None,
        match env::var("STATUS")?.as_str() {
            "online" => Status::Online,
            "idle" => Status::Idle,
            "dnd" => Status::DoNotDisturb,
            "invisible" => Status::Invisible,
            _ => panic!("Invalid STATUS provided"),
        },
    ))
}

pub async fn get_resume_sessions(
    conn: &mut redis::aio::Connection,
) -> ApiResult<HashMap<u64, ResumeSession>> {
    let shards: u64 = cache::get(conn, SHARDS_KEY).await?.unwrap_or_default();

    if shards != env::var("SHARDS_TOTAL")?.parse::<u64>()? || env::var("RESUME")? == "false" {
        return Ok(HashMap::new());
    }

    let sessions: HashMap<u64, SessionInfo> =
        cache::get(conn, SESSIONS_KEY).await?.unwrap_or_default();

    Ok(sessions
        .iter()
        .map(|(k, v)| {
            (
                *k,
                ResumeSession {
                    session_id: v.session_id.clone(),
                    sequence: v.sequence,
                },
            )
        })
        .collect())
}

pub async fn log_discord(cluster: &Cluster, color: usize, message: impl Into<String>) {
    let client = cluster.config().http_client();

    if env::var("LOG_CHANNEL").is_err() {
        return;
    }

    if env::var("LOG_CHANNEL").unwrap().parse::<u64>().is_err() {
        return;
    }

    let message = client
        .create_message(ChannelId(env::var("LOG_CHANNEL").unwrap().parse().unwrap()))
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
            timestamp: Some(Utc::now().to_rfc3339()),
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
