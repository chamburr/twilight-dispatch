use crate::{
    cache,
    config::CONFIG,
    constants::{
        CONNECT_COLOR, DISCONNECT_COLOR, EXCHANGE, JOIN_COLOR, LEAVE_COLOR, QUEUE_SEND,
        READY_COLOR, RESUME_COLOR,
    },
    metrics::{GATEWAY_EVENTS, GUILD_EVENTS, SHARD_EVENTS},
    models::{DeliveryInfo, DeliveryOpcode, PayloadInfo},
    utils::{log_discord, log_discord_guild},
};

use futures_util::{Stream, StreamExt};
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions},
    types::FieldTable,
    BasicProperties, Channel,
};
use redis::aio::MultiplexedConnection;
use simd_json::{json, ValueAccess};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
use tracing::{info, warn};
use twilight_gateway::{shard::raw_message::Message, Cluster, Event, EventType};
use twilight_model::id::{marker::UserMarker, Id};

pub async fn outgoing(
    conn: MultiplexedConnection,
    cluster: Arc<Cluster>,
    channel: Channel,
    bot_id: Id<UserMarker>,
    mut events: impl Stream<Item = (u64, Event)> + Send + Sync + Unpin + 'static,
) {
    while let Some((shard, event)) = events.next().await {
        let mut conn_clone = conn.clone();
        let cluster_clone = cluster.clone();
        let channel_clone = channel.clone();

        tokio::spawn(async move {
            let mut old = None;

            if CONFIG.state_enabled && event.kind() != EventType::ShardPayload {
                match timeout(
                    Duration::from_millis(10000),
                    cache::update(&mut conn_clone, &event, bot_id),
                )
                .await
                {
                    Ok(Ok(value)) => {
                        old = value;
                    }
                    Ok(Err(err)) => {
                        warn!("[Shard {}] Failed to update state: {:?}", shard, err);
                    }
                    Err(_) => {
                        warn!("[Shard {}] Timed out while updating state", shard);
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
                    log_discord(READY_COLOR, format!("[Shard {}] Ready", shard));
                    SHARD_EVENTS.with_label_values(&["Ready"]).inc();
                }
                Event::Resumed => {
                    if let Some(Ok(info)) = cluster_clone.shard(shard).map(|shard| shard.info()) {
                        info!(
                            "[Shard {}] Resumed (session: {})",
                            shard,
                            info.session_id().unwrap_or_default()
                        );
                    } else {
                        info!("[Shard {}] Resumed", shard);
                    }
                    log_discord(RESUME_COLOR, format!("[Shard {}] Resumed", shard));
                    SHARD_EVENTS.with_label_values(&["Resumed"]).inc();
                }
                Event::ShardConnected(_) => {
                    info!("[Shard {}] Connected", shard);
                    log_discord(CONNECT_COLOR, format!("[Shard {}] Connected", shard));
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
                    log_discord(DISCONNECT_COLOR, format!("[Shard {}] Disconnected", shard));
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
                Event::GuildCreate(data) => {
                    if old.is_none() {
                        GUILD_EVENTS.with_label_values(&["Join"]).inc();
                        log_discord_guild(
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
                            LEAVE_COLOR,
                            "Guild Leave",
                            format!(
                                "{} ({})",
                                guild
                                    .get("name")
                                    .and_then(|name| name.as_str())
                                    .unwrap_or("Unknown"),
                                guild.get("id").and_then(|id| id.as_str()).unwrap_or("0")
                            ),
                        );
                    }
                }
                Event::ShardPayload(mut data) => {
                    match simd_json::from_slice::<PayloadInfo>(data.bytes.as_mut_slice()) {
                        Ok(mut payload) => {
                            if let Some(kind) = payload.t.as_deref() {
                                GATEWAY_EVENTS
                                    .with_label_values(&[kind, shard.to_string().as_str()])
                                    .inc();

                                payload.old = old;

                                match simd_json::to_vec(&payload) {
                                    Ok(payload) => {
                                        let result = channel_clone
                                            .basic_publish(
                                                EXCHANGE,
                                                kind,
                                                BasicPublishOptions::default(),
                                                &payload,
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
                _ => {}
            }
        });
    }
}

pub async fn incoming(clusters: &[Arc<Cluster>], channel: &Channel) {
    let mut consumer = match channel
        .basic_consume(
            QUEUE_SEND,
            "",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
    {
        Ok(channel) => channel,
        Err(err) => {
            warn!("Failed to consume delivery channel: {:?}", err);
            return;
        }
    };

    while let Some(message) = consumer.next().await {
        match message {
            Ok(mut delivery) => {
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
                                        .send(
                                            payload.shard,
                                            Message::Binary(
                                                simd_json::to_vec(
                                                    &payload.data.unwrap_or_default(),
                                                )
                                                .unwrap_or_default(),
                                            ),
                                        )
                                        .await
                                    {
                                        warn!("Failed to send gateway command: {:?}", err);
                                    }
                                }
                                DeliveryOpcode::Reconnect => {
                                    info!("Shutting down shard {}", payload.shard);
                                    cluster.shard(payload.shard).unwrap().shutdown();
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
