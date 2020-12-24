#![deny(clippy::all, nonstandard_style, rust_2018_idioms)]

use crate::config::CONFIG;
use crate::constants::{EXCHANGE, PLAYER_ID_KEY, QUEUE_RECV, QUEUE_SEND};
use crate::models::ApiResult;
use crate::utils::{exchange_declare, get_node_config, queue_bind, queue_declare};

use dotenv::dotenv;
use lapin::options::BasicConsumeOptions;
use lapin::types::FieldTable;
use lapin::ExchangeKind;
use tracing::{error, info};
use twilight_andesite::Lavalink;
use twilight_model::id::UserId;

mod cache;
mod config;
mod constants;
mod handler;
mod metrics;
mod models;
mod utils;

#[tokio::main]
async fn main() {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let result = real_main().await;

    if let Err(err) = result {
        error!("{:?}", err);
    }
}

async fn real_main() -> ApiResult<()> {
    let redis = redis::Client::open(format!(
        "redis://{}:{}/",
        CONFIG.redis_host, CONFIG.redis_port
    ))?;

    let mut conn = redis.get_async_connection().await?;

    let amqp = lapin::Connection::connect(
        format!(
            "amqp://{}:{}@{}:{}/%2f",
            CONFIG.rabbit_username, CONFIG.rabbit_password, CONFIG.rabbit_host, CONFIG.rabbit_port
        )
        .as_str(),
        lapin::ConnectionProperties::default(),
    )
    .await?;

    let channel = amqp.create_channel().await?;
    let channel_send = amqp.create_channel().await?;

    exchange_declare(&channel, EXCHANGE, ExchangeKind::Topic).await?;
    queue_declare(&channel_send, QUEUE_SEND).await?;

    if CONFIG.default_queue {
        queue_declare(&channel, QUEUE_RECV).await?;
        queue_bind(&channel, QUEUE_RECV, EXCHANGE, "#").await?;
    }

    tokio::spawn(async {
        let _ = metrics::run_server().await;
    });

    let config = get_node_config(&mut conn).await?;
    let client = Lavalink::new(UserId(CONFIG.bot_id));
    let (node, receiver) = client
        .add_with_resume(config.address, config.authorization, config.resume)
        .await?;

    cache::set(&mut conn, PLAYER_ID_KEY, &node.connection_id()).await?;

    let mut conn_clone = redis.get_async_connection().await?;
    tokio::spawn(async move {
        handler::outgoing(&mut conn_clone, receiver, channel).await;
    });

    let consumer = channel_send
        .basic_consume(
            QUEUE_SEND,
            "",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    handler::incoming(&node, consumer).await;

    info!("Shutting down");

    Ok(())
}
