use crate::cache;
use crate::config::CONFIG;
use crate::constants::{CONNECT_COLOR, DISCONNECT_COLOR, EXCHANGE, READY_COLOR, RESUME_COLOR};
use crate::metrics::{GATEWAY_EVENTS, SHARD_EVENTS};
use crate::models::{DeliveryInfo, DeliveryOpcode, PayloadData, PayloadInfo};
use crate::utils::{get_event_flags, log_discord};

use lapin::options::{BasicAckOptions, BasicPublishOptions};
use lapin::{BasicProperties, Consumer};
use tokio::stream::StreamExt;
use tracing::{info, warn};
use twilight_gateway::{Cluster, Event};

pub async fn outgoing(mut conn: redis::aio::Connection, cluster: Cluster, channel: lapin::Channel) {
    let shard_strings: Vec<String> = (0..CONFIG.shards_total).map(|x| x.to_string()).collect();

    let mut events = cluster.some_events(get_event_flags());

    while let Some((shard, event)) = events.next().await {
        let mut old = None;
        if CONFIG.state_enabled {
            match cache::update(&mut conn, &event).await {
                Ok(value) => {
                    old = value;
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
                log_discord(&cluster, READY_COLOR, format!("[Shard {}] Ready", shard)).await;
                SHARD_EVENTS.with_label_values(&["Ready"]).inc();
            }
            Event::Resumed => {
                if let Ok(info) = cluster.shard(shard).unwrap().info() {
                    info!(
                        "[Shard {}] Resumed (session: {})",
                        shard,
                        info.session_id().unwrap()
                    );
                } else {
                    info!("[Shard {}] Resumed", shard);
                }
                log_discord(&cluster, RESUME_COLOR, format!("[Shard {}] Resumed", shard)).await;
                SHARD_EVENTS.with_label_values(&["Resumed"]).inc();
            }
            Event::ShardConnected(_) => {
                info!("[Shard {}] Connected", shard);
                log_discord(
                    &cluster,
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
                    &cluster,
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
            Event::ShardPayload(mut data) if CONFIG.state_enabled => {
                match simd_json::from_slice::<PayloadData>(data.bytes.as_mut_slice()) {
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
            Event::ShardPayload(mut data) => {
                match simd_json::from_slice::<PayloadInfo>(data.bytes.as_mut_slice()) {
                    Ok(payload) => {
                        if let Some(kind) = payload.t.as_deref() {
                            GATEWAY_EVENTS
                                .with_label_values(&[kind, shard_strings[shard as usize].as_str()])
                                .inc();

                            let result = channel
                                .basic_publish(
                                    EXCHANGE,
                                    kind,
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
                                    if let Err(err) =
                                        cluster.command(payload.shard, &payload.data.unwrap()).await
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
