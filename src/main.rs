use crate::config::get_config;
use crate::constants::{EXCHANGE, QUEUE_SEND, SESSIONS_KEY, SHARDS_KEY, STARTED_KEY, QUEUE_RECV};
use crate::models::{ApiResult, FormattedDateTime, SessionInfo};
use crate::utils::{get_clusters, get_queue, get_resume_sessions, get_shards};

use dotenv::dotenv;
use lapin::options::{BasicConsumeOptions, ExchangeDeclareOptions, QueueDeclareOptions, QueueBindOptions};
use lapin::types::FieldTable;
use lapin::ExchangeKind;
use std::collections::HashMap;
use tokio::join;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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
        .exchange_declare(
            EXCHANGE,
            ExchangeKind::Topic,
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
    channel_send
        .queue_declare(
            QUEUE_SEND,
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

    if config.declare_queue {
        channel
            .queue_declare(
                QUEUE_RECV,
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

        channel
            .queue_bind(
                QUEUE_RECV,
                EXCHANGE,
                "#",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;
    }

    let shards = get_shards();
    let resumes = get_resume_sessions(&mut conn).await?;
    let queue = get_queue().await;
    let clusters = get_clusters(resumes.clone(), queue).await?;

    info!("Starting up {} shards", shards);
    info!("Resuming {} sessions", resumes.len());

    cache::set(&mut conn, STARTED_KEY, &FormattedDateTime::now_utc()).await?;
    cache::set(&mut conn, SHARDS_KEY, &shards).await?;

    tokio::spawn(async {
        let _ = metrics::run_server().await;
    });

    let mut conn_clone = redis.get_async_connection().await?;
    let clusters_clone = clusters.clone();
    tokio::spawn(async move {
        join!(
            cache::run_jobs(&mut conn_clone, &clusters_clone),
            metrics::run_jobs(&clusters_clone),
        )
    });

    for cluster in clusters.clone() {
        let conn_clone = redis.get_async_connection().await?;

        let cluster_clone = cluster.clone();
        tokio::spawn(async move {
            cluster_clone.up().await;
        });

        let cluster_clone = cluster.clone();
        let channel_clone = channel.clone();
        tokio::spawn(async move {
            handler::outgoing(conn_clone, cluster_clone, channel_clone).await;
        });
    }

    let consumer = channel_send
        .basic_consume(
            "",
            QUEUE_SEND,
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    let clusters_clone = clusters.clone();
    tokio::spawn(async move {
        handler::incoming(clusters_clone, consumer).await;
    });

    let mut sigint = signal(SignalKind::interrupt())?;
    sigint.recv().await;

    info!("Shutting down");

    let mut sessions = HashMap::new();
    for cluster in clusters {
        for (key, value) in cluster.down_resumable().iter() {
            sessions.insert(
                key.to_string(),
                SessionInfo {
                    session_id: value.session_id.clone(),
                    sequence: value.sequence,
                },
            );
        }
    }

    cache::set(&mut conn, SESSIONS_KEY, &sessions).await?;

    Ok(())
}
