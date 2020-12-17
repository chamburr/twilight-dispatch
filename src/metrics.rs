use crate::cache;
use crate::config::CONFIG;
use crate::constants::{
    CHANNEL_KEY, EMOJI_KEY, GUILD_KEY, KEYS_SUFFIX, MEMBER_KEY, MESSAGE_KEY, METRICS_DUMP_INTERVAL,
    PRESENCE_KEY, ROLE_KEY, VOICE_KEY,
};
use crate::models::ApiResult;

use hyper::header::CONTENT_TYPE;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use lazy_static::lazy_static;
use prometheus::{
    register_int_counter_vec, register_int_gauge, register_int_gauge_vec, Encoder, IntCounterVec,
    IntGauge, IntGaugeVec, TextEncoder,
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use tokio::time;
use tracing::warn;
use twilight_gateway::shard::Stage;
use twilight_gateway::Cluster;

lazy_static! {
    pub static ref GATEWAY_EVENTS: IntCounterVec = register_int_counter_vec!(
        "gateway_events",
        "Events received through the Discord gateway",
        &["type", "shard"]
    )
    .unwrap();
    pub static ref SHARD_EVENTS: IntCounterVec = register_int_counter_vec!(
        "gateway_shard_events",
        "Discord shard connection events",
        &["type"]
    )
    .unwrap();
    pub static ref GATEWAY_SHARDS: IntGauge = register_int_gauge!(
        "gateway_shards",
        "Number of gateway connections with Discord"
    )
    .unwrap();
    pub static ref GATEWAY_STATUSES: IntGaugeVec = register_int_gauge_vec!(
        "gateway_statuses",
        "Status of the gateway connections",
        &["type"]
    )
    .unwrap();
    pub static ref GATEWAY_LATENCIES: IntGaugeVec = register_int_gauge_vec!(
        "gateway_latencies",
        "API latency with the Discord gateway",
        &["shard"]
    )
    .unwrap();
    pub static ref STATE_GUILDS: IntGauge =
        register_int_gauge!("state_guilds", "Number of guilds in state cache").unwrap();
    pub static ref STATE_CHANNELS: IntGauge =
        register_int_gauge!("state_channels", "Number of channels in state cache").unwrap();
    pub static ref STATE_MESSAGES: IntGauge =
        register_int_gauge!("state_messages", "Number of messages in state cache").unwrap();
    pub static ref STATE_ROLES: IntGauge =
        register_int_gauge!("state_roles", "Number of roles in state cache").unwrap();
    pub static ref STATE_EMOJIS: IntGauge =
        register_int_gauge!("state_emojiS", "Number of emojis in state cache").unwrap();
    pub static ref STATE_MEMBERS: IntGauge =
        register_int_gauge!("state_members", "Number of members in state cache").unwrap();
    pub static ref STATE_PRESENCES: IntGauge =
        register_int_gauge!("state_presences", "Number of presences in state cache").unwrap();
    pub static ref STATE_VOICES: IntGauge =
        register_int_gauge!("state_voices", "Number of voices in state cache").unwrap();
}

async fn serve(req: Request<Body>) -> ApiResult<Response<Body>> {
    if req.method() == Method::GET && req.uri().path() == "/metrics" {
        let mut buffer = vec![];
        let metrics = prometheus::gather();

        let encoder = TextEncoder::new();
        encoder.encode(metrics.as_slice(), &mut buffer)?;

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, encoder.format_type())
            .body(Body::from(buffer))?)
    } else {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())?)
    }
}

pub async fn run_server() -> ApiResult<()> {
    let addr = SocketAddr::new(
        IpAddr::from_str(CONFIG.prometheus_host.as_str())?,
        CONFIG.prometheus_port as u16,
    );

    let make_svc = make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(serve)) });

    Server::bind(&addr).serve(make_svc).await?;

    Err(().into())
}

struct StateStats {
    guilds: u64,
    channels: u64,
    messages: u64,
    roles: u64,
    emojis: u64,
    members: u64,
    presences: u64,
    voices: u64,
}

async fn get_state_stats(conn: &mut redis::aio::Connection) -> ApiResult<StateStats> {
    Ok(StateStats {
        guilds: cache::get_members_len(conn, format!("{}{}", GUILD_KEY, KEYS_SUFFIX)).await?,
        channels: cache::get_members_len(conn, format!("{}{}", CHANNEL_KEY, KEYS_SUFFIX)).await?,
        messages: cache::get_members_len(conn, format!("{}{}", MESSAGE_KEY, KEYS_SUFFIX)).await?,
        roles: cache::get_members_len(conn, format!("{}{}", ROLE_KEY, KEYS_SUFFIX)).await?,
        emojis: cache::get_members_len(conn, format!("{}{}", EMOJI_KEY, KEYS_SUFFIX)).await?,
        members: cache::get_members_len(conn, format!("{}{}", MEMBER_KEY, KEYS_SUFFIX)).await?,
        presences: cache::get_members_len(conn, format!("{}{}", PRESENCE_KEY, KEYS_SUFFIX)).await?,
        voices: cache::get_members_len(conn, format!("{}{}", VOICE_KEY, KEYS_SUFFIX)).await?,
    })
}

pub async fn run_jobs(conn: &mut redis::aio::Connection, clusters: &[Cluster]) {
    loop {
        let mut shards = vec![];
        for cluster in clusters {
            shards.append(&mut cluster.shards())
        }

        GATEWAY_SHARDS.set(shards.len() as i64);

        let mut statuses = HashMap::new();
        statuses.insert(format!("{}", Stage::Connected), 0);
        statuses.insert(format!("{}", Stage::Disconnected), 0);
        statuses.insert(format!("{}", Stage::Handshaking), 0);
        statuses.insert(format!("{}", Stage::Identifying), 0);
        statuses.insert(format!("{}", Stage::Resuming), 0);

        for shard in shards {
            if let Ok(info) = shard.info() {
                GATEWAY_LATENCIES
                    .with_label_values(&[info.id().to_string().as_str()])
                    .set(
                        info.latency()
                            .recent()
                            .back()
                            .map(|value| value.as_millis() as i64)
                            .unwrap_or_default(),
                    );

                *statuses
                    .get_mut(format!("{}", info.stage()).as_str())
                    .unwrap() += 1;
            }
        }

        for (stage, amount) in statuses {
            GATEWAY_STATUSES
                .with_label_values(&[stage.as_str()])
                .set(amount);
        }

        match get_state_stats(conn).await {
            Ok(stats) => {
                STATE_GUILDS.set(stats.guilds as i64);
                STATE_CHANNELS.set(stats.channels as i64);
                STATE_MESSAGES.set(stats.messages as i64);
                STATE_ROLES.set(stats.roles as i64);
                STATE_EMOJIS.set(stats.emojis as i64);
                STATE_MEMBERS.set(stats.members as i64);
                STATE_PRESENCES.set(stats.presences as i64);
                STATE_VOICES.set(stats.voices as i64);
            }
            Err(err) => {
                warn!("Failed to get state stats: {:?}", err);
            }
        }

        time::delay_for(time::Duration::from_millis(METRICS_DUMP_INTERVAL as u64)).await;
    }
}
