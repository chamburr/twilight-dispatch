use crate::constants::{player_key, PLAYER_STATS_KEY};
use crate::metrics::{PLAYED_TRACKS, VOICE_CLOSES};
use crate::models::{ApiResult, Player};
use crate::utils::decode_track;

use redis::AsyncCommands;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::warn;
use twilight_andesite::model::IncomingEvent;

pub async fn get<T: DeserializeOwned>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
) -> ApiResult<Option<T>> {
    let res: Option<String> = conn.get(key.to_string()).await?;

    Ok(res
        .map(|mut value| simd_json::from_str(value.as_mut_str()))
        .transpose()?)
}

pub async fn set<T: Serialize>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
    value: &T,
) -> ApiResult<()> {
    let value = simd_json::to_string(value)?;
    conn.set(key.to_string(), value).await?;

    Ok(())
}

pub async fn del_all(conn: &mut redis::aio::Connection, keys: &[String]) -> ApiResult<()> {
    if keys.is_empty() {
        return Ok(());
    }

    conn.del(keys).await?;

    Ok(())
}

pub async fn del<T: ToString>(conn: &mut redis::aio::Connection, key: T) -> ApiResult<()> {
    del_all(conn, &[key.to_string()]).await?;

    Ok(())
}

pub async fn update(conn: &mut redis::aio::Connection, event: &IncomingEvent) -> ApiResult<()> {
    match event {
        IncomingEvent::PlayerUpdate(data) => {
            let player = Player {
                guild_id: data.guild_id,
                time: data.state.time,
                position: data.state.position,
                paused: data.state.paused,
                volume: data.state.volume,
                filters: data.state.filters.clone(),
            };
            set(conn, player_key(data.guild_id), &player).await?;
        }
        IncomingEvent::Stats(data) => {
            set(conn, PLAYER_STATS_KEY, data).await?;
        }
        IncomingEvent::TrackStart(data) => match decode_track(data.track.clone()).await {
            Ok(track) => {
                PLAYED_TRACKS
                    .with_label_values(&[
                        track.info.title.as_str(),
                        track.info.length.to_string().as_str(),
                    ])
                    .inc();
            }
            Err(err) => {
                warn!("Failed to decode track: {:?}", err);
            }
        },
        IncomingEvent::WebsocketClose(data) => {
            VOICE_CLOSES
                .with_label_values(&[data.code.to_string().as_str()])
                .inc();
        }
        IncomingEvent::PlayerDestroy(data) => {
            del(conn, player_key(data.guild_id)).await?;
        }
        _ => {}
    }

    Ok(())
}
