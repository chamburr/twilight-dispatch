use crate::cache;
use crate::constants::EXCHANGE;
use crate::metrics::PLAYER_EVENTS;
use crate::models::PayloadInfo;

use futures::channel::mpsc::UnboundedReceiver;
use lapin::options::{BasicAckOptions, BasicPublishOptions};
use lapin::{BasicProperties, Consumer};
use tokio::stream::StreamExt;
use tracing::warn;
use twilight_andesite::model::{
    Destroy, GetPlayer, IncomingEvent, OutgoingEvent, Play, Stop, Update, VoiceUpdate,
};
use twilight_andesite::node::Node;

pub async fn outgoing(
    conn: &mut redis::aio::Connection,
    mut receiver: UnboundedReceiver<IncomingEvent>,
    channel: lapin::Channel,
) {
    while let Some(event) = receiver.next().await {
        let op = match event {
            IncomingEvent::PlayerUpdate(_) => "PLAYER_UPDATE",
            IncomingEvent::Stats(_) => "STATS",
            IncomingEvent::TrackEnd(_) => "TRACK_END",
            IncomingEvent::TrackStart(_) => "TRACK_START",
            IncomingEvent::TrackException(_) => "TRACK_EXCEPTION",
            IncomingEvent::TrackStuck(_) => "TRACK_STUCK",
            IncomingEvent::WebsocketClose(_) => "WEBSOCKET_CLOSE",
            IncomingEvent::PlayerDestroy(_) => "PLAYER_DESTROY",
        };

        PLAYER_EVENTS.with_label_values(&[op]).inc();

        if let Err(err) = cache::update(conn, &event).await {
            warn!("Failed to update state: {:?}", err);
        }

        match simd_json::to_vec(&event) {
            Ok(payload) => {
                let result = channel
                    .basic_publish(
                        EXCHANGE,
                        op,
                        BasicPublishOptions::default(),
                        payload,
                        BasicProperties::default(),
                    )
                    .await;

                if let Err(err) = result {
                    warn!("Failed to publish event: {:?}", err);
                }
            }
            Err(err) => {
                warn!("Failed to serialize payload: {:?}", err);
            }
        }
    }
}

pub async fn incoming(node: &Node, mut consumer: Consumer) {
    while let Some(message) = consumer.next().await {
        match message {
            Ok((channel, mut delivery)) => {
                let _ = channel
                    .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                    .await;
                let mut data = delivery.data.clone();
                match simd_json::from_slice::<PayloadInfo>(delivery.data.as_mut_slice()).and_then(
                    |info| match info.op.as_str() {
                        "voiceUpdate" => simd_json::from_slice::<VoiceUpdate>(data.as_mut_slice())
                            .map(OutgoingEvent::from),
                        "getPlayer" => simd_json::from_slice::<GetPlayer>(data.as_mut_slice())
                            .map(OutgoingEvent::from),
                        "play" => simd_json::from_slice::<Play>(data.as_mut_slice())
                            .map(OutgoingEvent::from),
                        "stop" => simd_json::from_slice::<Stop>(data.as_mut_slice())
                            .map(OutgoingEvent::from),
                        "update" => simd_json::from_slice::<Update>(data.as_mut_slice())
                            .map(OutgoingEvent::from),
                        "destroy" => simd_json::from_slice::<Destroy>(data.as_mut_slice())
                            .map(OutgoingEvent::from),
                        _ => simd_json::from_slice::<OutgoingEvent>(data.as_mut_slice()),
                    },
                ) {
                    Ok(payload) => {
                        if let Err(err) = node.send(payload) {
                            warn!("Failed to send outgoing event: {:?}", err);
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
