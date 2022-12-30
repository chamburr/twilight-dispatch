use crate::{
    cache,
    config::CONFIG,
    constants::{SESSIONS_KEY, SHARDS_KEY},
    models::{ApiResult, SessionInfo},
};

use futures_util::Stream;
use redis::aio::MultiplexedConnection;
use std::{collections::HashMap, fmt::Debug, future::Future, pin::Pin, sync::Arc, time::Duration};
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
use twilight_model::gateway::{
    payload::outgoing::update_presence::UpdatePresencePayload, presence::Activity,
};

#[derive(Clone, Debug)]
pub struct ClusterInfo {
    pub clusters: u64,
    pub shards: u64,
    pub resumes: u64,
}

pub async fn get_clusters(
    conn: &mut MultiplexedConnection,
) -> ApiResult<(
    Vec<Arc<Cluster>>,
    Vec<impl Stream<Item = (u64, Event)> + Send + Sync + Unpin + 'static>,
    ClusterInfo,
)> {
    let queue: Arc<dyn Queue> = if CONFIG.shards_concurrency == 1 {
        Arc::new(LocalQueue::new(Duration::from_secs(CONFIG.shards_wait)))
    } else {
        Arc::new(LargeBotQueue::new(
            CONFIG.shards_concurrency as usize,
            Duration::from_secs(CONFIG.shards_wait),
        ))
    };

    let mut resumes = HashMap::new();
    if CONFIG.resume && cache::get(conn, SHARDS_KEY).await? == Some(CONFIG.shards_total) {
        let sessions: HashMap<String, SessionInfo> =
            cache::get(conn, SESSIONS_KEY).await?.unwrap_or_default();

        for (key, value) in sessions.into_iter() {
            resumes.insert(
                key.parse().unwrap(),
                ResumeSession {
                    resume_url: None,
                    session_id: value.session_id,
                    sequence: value.sequence,
                },
            );
        }
    };

    let shards = CONFIG.shards_end - CONFIG.shards_start + 1;
    let base = shards / CONFIG.clusters;
    let extra = shards % CONFIG.clusters;

    let mut clusters = Vec::with_capacity(CONFIG.clusters as usize);
    let mut events = Vec::with_capacity(CONFIG.clusters as usize);
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
        .gateway_url("wss://gateway.discord.gg".to_owned())
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
        .large_threshold(CONFIG.large_threshold)
        .resume_sessions(resumes.clone())
        .event_types(EventTypeFlags::all())
        .build()
        .await?;

        clusters.push(Arc::new(cluster));
        events.push(event);

        last_index = index + 1;
    }

    Ok((
        clusters,
        events,
        ClusterInfo {
            clusters: CONFIG.clusters,
            shards,
            resumes: resumes.len() as u64,
        },
    ))
}

#[derive(Debug)]
struct LocalQueue(UnboundedSender<Sender<()>>);

impl LocalQueue {
    fn new(duration: Duration) -> Self {
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
struct LargeBotQueue(Vec<UnboundedSender<Sender<()>>>);

impl LargeBotQueue {
    fn new(buckets: usize, duration: Duration) -> Self {
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
