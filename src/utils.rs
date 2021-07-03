use crate::{
    cache,
    config::CONFIG,
    constants::{SESSIONS_KEY, SHARDS_KEY},
    models::{ApiResult, SessionInfo},
};

use futures_util::Stream;
use serde::Serialize;
use simd_json::owned::Value;
use std::{collections::HashMap, fmt::Debug, future::Future, pin::Pin, sync::Arc, time::Duration};
use time::{Format, OffsetDateTime};
use tokio::{
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
        oneshot::{self, Sender},
    },
    time::sleep,
};
use tracing::warn;
use twilight_gateway::{
    cluster::ShardScheme, queue::Queue, shard::ResumeSession, Cluster, Event, EventTypeFlags,
    Intents,
};
use twilight_model::{
    channel::embed::Embed,
    gateway::{
        payload::update_presence::UpdatePresencePayload,
        presence::{Activity, UserOrId},
    },
    id::{ChannelId, UserId},
};

#[derive(Clone, Debug)]
pub struct LocalQueue(UnboundedSender<Sender<()>>);

impl LocalQueue {
    pub fn new(duration: Duration) -> Self {
        let (tx, rx) = unbounded_channel();
        tokio::spawn(waiter(rx, duration));

        Self(tx)
    }
}

impl Queue for LocalQueue {
    fn request(&'_ self, [_, _]: [u64; 2]) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();

            if let Err(err) = self.0.clone().send(tx) {
                warn!("skipping, send failed: {:?}", err);
                return;
            }

            let _ = rx.await;
        })
    }
}

#[derive(Debug)]
pub struct LargeBotQueue(Vec<UnboundedSender<Sender<()>>>);

impl LargeBotQueue {
    pub fn new(buckets: usize, duration: Duration) -> Self {
        let mut queues = Vec::with_capacity(buckets);
        for _ in 0..buckets {
            let (tx, rx) = unbounded_channel();
            tokio::spawn(waiter(rx, duration));
            queues.push(tx)
        }

        Self(queues)
    }
}

impl Queue for LargeBotQueue {
    fn request(&'_ self, shard_id: [u64; 2]) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        #[allow(clippy::cast_possible_truncation)]
        let bucket = (shard_id[0] % (self.0.len() as u64)) as usize;
        let (tx, rx) = oneshot::channel();

        Box::pin(async move {
            if let Err(err) = self.0[bucket].clone().send(tx) {
                warn!("skipping, send failed: {:?}", err);
                return;
            }

            let _ = rx.await;
        })
    }
}

async fn waiter(mut rx: UnboundedReceiver<Sender<()>>, duration: Duration) {
    while let Some(req) = rx.recv().await {
        if let Err(err) = req.send(()) {
            warn!("skipping, send failed: {:?}", err);
        }
        sleep(duration).await;
    }
}

#[inline]
pub fn get_user_id(user: &UserOrId) -> UserId {
    match user {
        UserOrId::User(u) => u.id,
        UserOrId::UserId { id } => *id,
    }
}

pub async fn get_clusters(
    resumes: HashMap<u64, ResumeSession>,
    queue: Arc<Box<dyn Queue>>,
) -> ApiResult<(
    Vec<Cluster>,
    Vec<impl Stream<Item = (u64, Event)> + Send + Sync + Unpin + 'static>,
)> {
    let shards = get_shards();
    let base = shards / CONFIG.clusters;
    let extra = shards % CONFIG.clusters;

    let mut clusters = vec![];
    let mut events = vec![];
    let mut last_index = CONFIG.shards_start;

    for i in 0..CONFIG.clusters {
        let index = if i < extra {
            last_index + base
        } else {
            last_index + base - 1
        };

        let (cluster, event) = Cluster::builder(
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
        .presence(
            UpdatePresencePayload::new(
                vec![Activity {
                    application_id: None,
                    assets: None,
                    buttons: Vec::new(),
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
            )
            .unwrap(),
        )
        .large_threshold(CONFIG.large_threshold)?
        .resume_sessions(resumes.clone())
        .event_types(get_event_flags())
        .build()
        .await?;

        clusters.push(cluster);
        events.push(event);

        last_index = index + 1;
    }

    Ok((clusters, events))
}

pub fn get_queue() -> Arc<Box<dyn Queue>> {
    let concurrency = CONFIG.shards_concurrency as usize;
    let wait = Duration::from_secs(CONFIG.shards_wait);
    if concurrency == 1 {
        Arc::new(Box::new(LocalQueue::new(wait)))
    } else {
        Arc::new(Box::new(LargeBotQueue::new(concurrency, wait)))
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

pub fn log_discord(cluster: &Cluster, color: usize, message: impl Into<String>) {
    let client = cluster.config().http_client().clone();
    let message = message.into();

    tokio::spawn(async move {
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
                title: Some(message),
                url: None,
                video: None,
            });

        if let Ok(message) = message {
            if let Err(err) = message.await {
                warn!("Failed to post message to Discord: {:?}", err)
            }
        }
    });
}

pub fn log_discord_guild(
    cluster: &Cluster,
    color: usize,
    title: impl Into<String>,
    message: impl Into<String>,
) {
    let client = cluster.config().http_client().clone();
    let title = title.into();
    let message = message.into();

    tokio::spawn(async move {
        let message = client
            .create_message(ChannelId(CONFIG.log_guild_channel))
            .embed(Embed {
                author: None,
                color: Some(color as u32),
                description: Some(message),
                fields: vec![],
                footer: None,
                image: None,
                kind: "".to_owned(),
                provider: None,
                thumbnail: None,
                timestamp: Some(OffsetDateTime::now_utc().format(Format::Rfc3339)),
                title: Some(title),
                url: None,
                video: None,
            });

        if let Ok(message) = message {
            if let Err(err) = message.await {
                warn!("Failed to post message to Discord: {:?}", err)
            }
        }
    });
}

pub fn get_shards() -> u64 {
    CONFIG.shards_end - CONFIG.shards_start + 1
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
