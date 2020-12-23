use crate::cache;
use crate::config::CONFIG;
use crate::constants::{PLAYER_BUFFER, PLAYER_ID_KEY};
use crate::models::ApiResult;

use lapin::options::{ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::ExchangeKind;
use std::convert::TryInto;
use std::net::SocketAddr;
use std::str::FromStr;
use twilight_andesite::http::Track;
use twilight_andesite::node::{NodeConfig, Resume};
use twilight_model::id::UserId;

pub async fn exchange_declare(
    channel: &lapin::Channel,
    name: &str,
    kind: ExchangeKind,
) -> ApiResult<()> {
    channel
        .exchange_declare(
            name,
            kind,
            ExchangeDeclareOptions {
                passive: false,
                durable: true,
                auto_delete: false,
                internal: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;

    Ok(())
}

pub async fn queue_declare(channel: &lapin::Channel, name: &str) -> ApiResult<()> {
    channel
        .queue_declare(
            name,
            QueueDeclareOptions {
                passive: false,
                durable: true,
                exclusive: false,
                auto_delete: false,
                nowait: false,
            },
            FieldTable::default(),
        )
        .await?;

    Ok(())
}

pub async fn queue_bind(
    channel: &lapin::Channel,
    name: &str,
    exchange: &str,
    key: &str,
) -> ApiResult<()> {
    channel
        .queue_bind(
            name,
            exchange,
            key,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;

    Ok(())
}

pub fn get_node_address() -> ApiResult<SocketAddr> {
    let address = SocketAddr::from_str(
        format!("{}:{}", CONFIG.andesite_host, CONFIG.andesite_port).as_str(),
    )?;

    Ok(address)
}

pub async fn get_node_config(conn: &mut redis::aio::Connection) -> ApiResult<NodeConfig> {
    let address = get_node_address()?;
    let resume_id: Option<u64> = cache::get(conn, PLAYER_ID_KEY).await?;
    let resume = Resume::new_with_id(PLAYER_BUFFER as u64, resume_id);

    Ok(NodeConfig {
        address,
        authorization: CONFIG.andesite_secret.clone(),
        resume: Some(resume),
        user_id: UserId(CONFIG.bot_id),
    })
}

pub async fn decode_track(track: String) -> ApiResult<Track> {
    let address = get_node_address()?;
    let config = NodeConfig {
        address,
        authorization: CONFIG.andesite_secret.clone(),
        resume: None,
        user_id: UserId(CONFIG.bot_id),
    };

    let request = twilight_andesite::http::decode_track(config, track)?;
    let response = reqwest::Client::new().execute(request.try_into()?).await?;

    Ok(response.json().await?)
}
