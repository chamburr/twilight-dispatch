use crate::{
    cache,
    config::CONFIG,
    constants::{
        CONNECT_COLOR, DISCONNECT_COLOR, EXCHANGE, JOIN_COLOR, LEAVE_COLOR, READY_COLOR,
        RESUME_COLOR,
    },
    metrics::{GATEWAY_EVENTS, GUILD_EVENTS, SHARD_EVENTS},
    models::{DeliveryInfo, DeliveryOpcode, PayloadInfo},
    utils::{get_event_flags, log_discord, log_discord_guild},
};

use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicPublishOptions},
    BasicProperties, Consumer,
};
use redis::RedisResult;
use simd_json::{json, Value};
use time::Instant;
use tracing::{info, warn};
use twilight_gateway::{Cluster, Event};

pub async fn outgoing(
    conn: &mut redis::aio::Connection,
    cluster: Cluster,
    channel: lapin::Channel,
) {
    let shard_strings: Vec<String> = (0..CONFIG.shards_total).map(|x| x.to_string()).collect();

    let mut events = cluster.some_events(get_event_flags());

    let mut initial_pipe = vec![redis::pipe(); CONFIG.shards_total as usize];
    let mut last_guild_create = vec![None; CONFIG.shards_total as usize];
    let mut ready = vec![false; CONFIG.shards_total as usize];

    while let Some((shard, event)) = events.next().await {
        let mut old = None;
        let shard = shard as usize;

        if CONFIG.state_enabled {
            if !ready[shard] {
                if let Event::GuildCreate(_) = &event {
                    last_guild_create[shard] = Some(Instant::now());
                }

                if let Some(last) = last_guild_create[shard] {
                    if last.elapsed().as_seconds_f64() > 5.0 {
                        ready[shard] = true;
                        let result: RedisResult<()> = initial_pipe[shard].query_async(conn).await;
                        initial_pipe[shard].clear();
                        if let Err(err) = result {
                            warn!(
                                "[Shard {}] Failed to update initial state: {:?}",
                                shard, err
                            );
                        }
                    }
                }
            } else if let Event::Ready(_) = &event {
                ready[shard] = false;
                last_guild_create[shard] = None;
            }

            match cache::update(conn, &event).await {
                Ok((value, pipe)) => {
                    old = value;

                    if ready[shard] {
                        let result: RedisResult<()> = pipe.query_async(conn).await;
                        if let Err(err) = result {
                            warn!("[Shard {}] Failed to update guild state: {:?}", shard, err);
                        }
                    } else {
                        for command in pipe.cmd_iter() {
                            initial_pipe[shard].add_command(command.clone());
                        }
                    }
                }
                Err(err) => {
                    warn!("[Shard {}] Failed to update state: {:?}", shard, err);
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
                log_discord(&cluster, READY_COLOR, format!("[Shard {}] Ready", shard));
                SHARD_EVENTS.with_label_values(&["Ready"]).inc();
            }
            Event::Resumed => {
                if let Ok(info) = cluster.shard(shard as u64).unwrap().info() {
                    info!(
                        "[Shard {}] Resumed (session: {})",
                        shard,
                        info.session_id().unwrap_or_default()
                    );
                } else {
                    info!("[Shard {}] Resumed", shard);
                }
                log_discord(&cluster, RESUME_COLOR, format!("[Shard {}] Resumed", shard));
                SHARD_EVENTS.with_label_values(&["Resumed"]).inc();
            }
            Event::ShardConnected(_) => {
                info!("[Shard {}] Connected", shard);
                log_discord(
                    &cluster,
                    CONNECT_COLOR,
                    format!("[Shard {}] Connected", shard),
                );
                SHARD_EVENTS.with_label_values(&["Connected"]).inc();
            }
            Event::ShardConnecting(data) => {
                info!("[Shard {}] Connecting (url: {})", shard, data.gateway);
                SHARD_EVENTS.with_label_values(&["Connecting"]).inc();
            }
            Event::ShardDisconnected(data) => {
                if let Some(code) = data.code {
                    let reason = data.reason.unwrap_or_default();
                    if !reason.is_empty() {
                        info!(
                            "[Shard {}] Disconnected (code: {}, reason: {})",
                            shard, code, reason
                        );
                    } else {
                        info!("[Shard {}] Disconnected (code: {})", shard, code);
                    }
                } else {
                    info!("[Shard {}] Disconnected", shard);
                }
                log_discord(
                    &cluster,
                    DISCONNECT_COLOR,
                    format!("[Shard {}] Disconnected", shard),
                );
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
            Event::ShardPayload(mut data) => {
                match simd_json::from_slice::<PayloadInfo>(data.bytes.as_mut_slice()) {
                    Ok(mut payload) => {
                        if let Some(kind) = payload.t.as_deref() {
                            GATEWAY_EVENTS
                                .with_label_values(&[kind, shard_strings[shard as usize].as_str()])
                                .inc();

                            payload.old = old;

                            match simd_json::to_vec(&payload) {
                                Ok(payload) => {
                                    let result = channel
                                        .basic_publish(
                                            EXCHANGE,
                                            kind,
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
            Event::GuildCreate(data) => {
                if ready[shard] {
                    GUILD_EVENTS.with_label_values(&["Join"]).inc();
                    log_discord_guild(
                        &cluster,
                        JOIN_COLOR,
                        "Guild Join",
                        format!("{} ({})", data.name, data.id),
                    );
                }
            }
            Event::GuildDelete(data) => {
                if !data.unavailable {
                    GUILD_EVENTS.with_label_values(&["Leave"]).inc();
                    let old_data = old.unwrap_or(json!({}));
                    let guild = old_data.as_object().unwrap();
                    log_discord_guild(
                        &cluster,
                        LEAVE_COLOR,
                        "Guild Leave",
                        format!(
                            "{} ({})",
                            guild
                                .get("name")
                                .map(|name| name.as_str().unwrap())
                                .unwrap_or("Unknown"),
                            guild
                                .get("id")
                                .map(|id| id.as_str().unwrap())
                                .unwrap_or("0")
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}

pub async fn incoming(clusters: Vec<Cluster>, mut consumer: Consumer) {
    while let Some(message) = consumer.next().await {
        match message {
            Ok((channel, mut delivery)) => {
                let _ = channel
                    .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                    .await;
                match simd_json::from_slice::<DeliveryInfo>(delivery.data.as_mut_slice()) {
                    Ok(payload) => {
                        let cluster = clusters
                            .iter()
                            .find(|cluster| cluster.shard(payload.shard).is_some());
                        if let Some(cluster) = cluster {
                            match payload.op {
                                DeliveryOpcode::Send => {
                                    if let Err(err) = cluster
                                        .command(payload.shard, &payload.data.unwrap_or_default())
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
}
