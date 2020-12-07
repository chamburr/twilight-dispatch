use crate::config::get_config;
use crate::constants::{
    CONNECT_COLOR, DISCONNECT_COLOR, QUEUE_RECV, QUEUE_SEND, READY_COLOR, RESUME_COLOR,
    SESSIONS_KEY, SHARDS_KEY, STARTED_KEY,
};
use crate::metrics::{GATEWAY_EVENTS, SHARD_EVENTS};
use crate::models::{
    ApiResult, DeliveryInfo, DeliveryOpcode, FormattedDateTime, PayloadData, PayloadInfo,
    SessionInfo,
};
use crate::utils::{
    get_gateway_url, get_queue, get_resume_sessions, get_shard_scheme, get_update_status_info,
    log_discord,
};

use dotenv::dotenv;
use futures::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::BasicProperties;
use std::collections::HashMap;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use twilight_gateway::{Cluster, Event, EventTypeFlags, Intents};

mod cache;
mod config;
mod constants;
mod metrics;
mod models;
mod utils;

#[tokio::main]
async fn main() {
    dotenv().ok();
    config::init();

    if get_config().rust_log.to_lowercase() == "info" {
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("twilight_gateway=warn".parse().unwrap())
                    .add_directive("twilight_gateway_queue=warn".parse().unwrap()),
            )
            .init()
    } else {
        tracing_subscriber::fmt::init()
    }

    let result = real_main().await;

    if let Err(err) = result {
        error!("{:?}", err);
    }
}

async fn real_main() -> ApiResult<()> {
    let config = get_config();

    let redis = redis::Client::open(format!(
        "redis://{}:{}/",
        config.redis_host, config.redis_port
    ))?;

    let mut conn = redis.get_async_connection().await?;

    let amqp = lapin::Connection::connect(
        format!(
            "amqp://{}:{}@{}:{}/%2f",
            config.rabbit_username, config.rabbit_password, config.rabbit_host, config.rabbit_port
        )
        .as_str(),
        lapin::ConnectionProperties::default(),
    )
    .await?;

    let channel = amqp.create_channel().await?;
    let channel_send = amqp.create_channel().await?;

    channel
        .queue_declare(
            QUEUE_RECV,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;
    channel_send
        .queue_declare(
            QUEUE_SEND,
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await?;

    tokio::spawn(async {
        let _ = metrics::run_server().await;
    });

    let resumes = get_resume_sessions(&mut conn).await?;
    let cluster = Cluster::builder(
        config.bot_token.clone(),
        Intents::from_bits(config.intents).unwrap(),
    )
    .gateway_url(Some(get_gateway_url()))
    .shard_scheme(get_shard_scheme()?)
    .queue(get_queue().await?)
    .presence(get_update_status_info()?)
    .large_threshold(config.large_threshold)?
    .resume_sessions(resumes.clone())
    .build()
    .await?;

    info!("Starting up {} shards", cluster.shards().len());
    info!("Resuming {} sessions", resumes.len());

    cache::set(&mut conn, STARTED_KEY, &FormattedDateTime::now_utc()).await?;
    cache::set(&mut conn, SHARDS_KEY, &cluster.shards().len()).await?;

    let mut conn_clone = redis.get_async_connection().await?;
    let cluster_clone = cluster.clone();
    tokio::spawn(async move {
        cache::run_jobs(&mut conn_clone, &cluster_clone).await;
    });

    let cluster_clone = cluster.clone();
    tokio::spawn(async move {
        metrics::run_jobs(&cluster_clone).await;
    });

    let cluster_clone = cluster.clone();
    tokio::spawn(async move {
        cluster_clone.up().await;
    });

    let mut conn_clone = redis.get_async_connection().await?;
    let cluster_clone = cluster.clone();
    tokio::spawn(async move {
        let shard_strings: Vec<String> = (0..cluster_clone.shards().len())
            .map(|x| x.to_string())
            .collect();

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

        if config.state_enabled {
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

            if config.state_member {
                event_flags |= EventTypeFlags::MEMBER_ADD
                    | EventTypeFlags::MEMBER_REMOVE
                    | EventTypeFlags::MEMBER_CHUNK
                    | EventTypeFlags::MEMBER_UPDATE;

                if config.state_presence {
                    event_flags |= EventTypeFlags::PRESENCE_UPDATE;
                }
            }

            if config.state_message {
                event_flags |= EventTypeFlags::MESSAGE_CREATE
                    | EventTypeFlags::MESSAGE_DELETE
                    | EventTypeFlags::MESSAGE_DELETE_BULK
                    | EventTypeFlags::MESSAGE_UPDATE;
            }
        }

        let mut events = cluster_clone.some_events(event_flags);

        while let Some((shard, event)) = events.next().await {
            let mut old = None;
            if config.state_enabled {
                match cache::update(&mut conn_clone, &event).await {
                    Ok(value) => {
                        old = value;
                    }
                    Err(err) => {
                        warn!("Failed to update state: {:?}", err);
                    }
                }
            }

            match event {
                Event::GatewayHello(data) => {
                    info!("[Shard {}] Hello (heartbeat interval: {})", shard, data);
                }
                Event::GatewayInvalidateSession(data) => {
                    info!("[Shard {}] Invalid Session (resumable: {})", shard, data);
                }
                Event::Ready(data) => {
                    info!("[Shard {}] Ready (session: {})", shard, data.session_id);
                    log_discord(
                        &cluster_clone,
                        READY_COLOR,
                        format!("[Shard {}] Ready", shard),
                    )
                    .await;
                    SHARD_EVENTS.with_label_values(&["Ready"]).inc();
                }
                Event::Resumed => {
                    if let Ok(info) = cluster_clone.shard(shard).unwrap().info() {
                        info!(
                            "[Shard {}] Resumed (session: {})",
                            shard,
                            info.session_id().unwrap()
                        );
                    } else {
                        info!("[Shard {}] Resumed", shard);
                    }
                    log_discord(
                        &cluster_clone,
                        RESUME_COLOR,
                        format!("[Shard {}] Resumed", shard),
                    )
                    .await;
                    SHARD_EVENTS.with_label_values(&["Resumed"]).inc();
                }
                Event::ShardConnected(_) => {
                    info!("[Shard {}] Connected", shard);
                    log_discord(
                        &cluster_clone,
                        CONNECT_COLOR,
                        format!("[Shard {}] Connected", shard),
                    )
                    .await;
                    SHARD_EVENTS.with_label_values(&["Connected"]).inc();
                }
                Event::ShardConnecting(data) => {
                    info!(
                        "[Shard {}] Connecting (url: {})",
                        shard,
                        data.gateway.split('#').next().unwrap()
                    );
                    SHARD_EVENTS.with_label_values(&["Connecting"]).inc();
                }
                Event::ShardDisconnected(data) => {
                    if let Some(code) = data.code {
                        if !data.reason.clone().unwrap_or_default().is_empty() {
                            info!(
                                "[Shard {}] Disconnected (code: {}, reason: {})",
                                shard,
                                code,
                                data.reason.clone().unwrap_or_default()
                            );
                        } else {
                            info!("[Shard {}] Disconnected (code: {})", shard, code);
                        }
                    } else {
                        info!("[Shard {}] Disconnected", shard);
                    }
                    log_discord(
                        &cluster_clone,
                        DISCONNECT_COLOR,
                        format!("[Shard {}] Disconnected", shard),
                    )
                    .await;
                    SHARD_EVENTS.with_label_values(&["Disconnected"]).inc();
                }
                Event::ShardIdentifying(_) => {
                    info!("[Shard {}] Identifying", shard);
                    SHARD_EVENTS.with_label_values(&["Identifying"]).inc();
                }
                Event::ShardReconnecting(_) => {
                    info!("[Shard {}] Reconnecting", shard);
                    SHARD_EVENTS.with_label_values(&["Reconnecting"]).inc();
                }
                Event::ShardResuming(data) => {
                    info!("[Shard {}] Resuming (sequence: {})", shard, data.seq);
                    SHARD_EVENTS.with_label_values(&["Resuming"]).inc();
                }
                Event::ShardPayload(mut data) if config.state_enabled => {
                    match simd_json::from_slice::<PayloadData>(data.bytes.as_mut_slice()) {
                        Ok(mut payload) => {
                            if let Some(kind) = payload.t.as_deref() {
                                GATEWAY_EVENTS
                                    .with_label_values(&[
                                        kind,
                                        shard_strings[shard as usize].as_str(),
                                    ])
                                    .inc();

                                payload.old = old;

                                match simd_json::to_vec(&payload) {
                                    Ok(payload) => {
                                        let result = channel
                                            .basic_publish(
                                                "",
                                                QUEUE_RECV,
                                                BasicPublishOptions::default(),
                                                payload,
                                                BasicProperties::default(),
                                            )
                                            .await;

                                        if let Err(err) = result {
                                            warn!(
                                                "[Shard {}] Failed to publish event: {:?}",
                                                shard, err
                                            );
                                        }
                                    }
                                    Err(err) => {
                                        warn!(
                                            "[Shard {}] Failed to serialize payload: {:?}",
                                            shard, err
                                        );
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            warn!("[Shard {}] Could not decode payload: {:?}", shard, err);
                        }
                    }
                }
                Event::ShardPayload(mut data) => {
                    match simd_json::from_slice::<PayloadInfo>(data.bytes.as_mut_slice()) {
                        Ok(payload) => {
                            if let Some(kind) = payload.t.as_deref() {
                                GATEWAY_EVENTS
                                    .with_label_values(&[
                                        kind,
                                        shard_strings[shard as usize].as_str(),
                                    ])
                                    .inc();

                                let result = channel
                                    .basic_publish(
                                        "",
                                        QUEUE_RECV,
                                        BasicPublishOptions::default(),
                                        data.bytes,
                                        BasicProperties::default(),
                                    )
                                    .await;

                                if let Err(err) = result {
                                    warn!("[Shard {}] Failed to publish event: {:?}", shard, err);
                                }
                            }
                        }
                        Err(err) => {
                            warn!("[Shard {}] Could not decode payload: {:?}", shard, err);
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let cluster_clone = cluster.clone();
    let mut consumer = channel_send
        .basic_consume(
            "",
            QUEUE_SEND,
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    tokio::spawn(async move {
        while let Some(message) = consumer.next().await {
            match message {
                Ok((channel, mut delivery)) => {
                    let _ = channel
                        .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                        .await;
                    match simd_json::from_slice::<DeliveryInfo>(delivery.data.as_mut_slice()) {
                        Ok(payload) => {
                            let shard = cluster_clone.shard(payload.shard);
                            if shard.is_some() {
                                match payload.op {
                                    DeliveryOpcode::Send => {
                                        if let Err(err) = cluster_clone
                                            .command(payload.shard, &payload.data.unwrap())
                                            .await
                                        {
                                            warn!("Failed to send gateway command: {:?}", err);
                                        }
                                    }
                                    DeliveryOpcode::Reconnect => {
                                        // Currently noop as twilight-gateway does not support this.
                                        // See https://github.com/twilight-rs/twilight/issues/615.
                                    }
                                }
                            } else {
                                warn!("Delivery received for invalid shard: {}", payload.shard)
                            }
                        }
                        Err(err) => {
                            warn!("Failed to deserialize payload: {:?}", err);
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to consume delivery: {:?}", err);
                }
            }
        }
    });

    let mut sigint = signal(SignalKind::interrupt())?;
    sigint.recv().await;

    info!("Shutting down");

    let sessions: HashMap<String, SessionInfo> = cluster
        .down_resumable()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                SessionInfo {
                    session_id: v.session_id.clone(),
                    sequence: v.sequence,
                },
            )
        })
        .collect();

    cache::set(&mut conn, SESSIONS_KEY, &sessions).await?;

    info!("Shutdown complete");

    Ok(())
}
