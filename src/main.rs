#![recursion_limit = "128"]
#![deny(clippy::all, nonstandard_style, rust_2018_idioms, unused, warnings)]

use crate::{
    config::CONFIG,
    constants::{EXCHANGE, QUEUE_RECV, QUEUE_SEND, SESSIONS_KEY, SHARDS_KEY, STARTED_KEY},
    models::{ApiResult, FormattedDateTime, SessionInfo},
    utils::{get_clusters, get_current_user, get_queue, get_resume_sessions, get_shards},
};

use dotenv::dotenv;
use lapin::{
    options::{ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions},
    types::FieldTable,
    ExchangeKind,
};
use std::collections::HashMap;
use tokio::{join, signal::ctrl_c};
use tracing::{error, info};

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
        "redis://{}:{}@{}:{}/",
        CONFIG.redis_username, CONFIG.redis_password, CONFIG.redis_host, CONFIG.redis_port
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

    if CONFIG.default_queue {
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
    let resumes_len = resumes.len();
    let queue = get_queue();
    let (clusters, events) = get_clusters(resumes, queue).await?;

    info!("Starting up {} clusters", clusters.len());
    info!("Starting up {} shards", shards);
    info!("Resuming {} sessions", resumes_len);

    cache::set(&mut conn, STARTED_KEY, &FormattedDateTime::now()).await?;
    cache::set(&mut conn, SHARDS_KEY, &CONFIG.shards_total).await?;

    tokio::spawn(async {
        let _ = metrics::run_server().await;
    });

    let mut conn_clone = redis.get_async_connection().await?;
    let mut conn_clone_two = redis.get_async_connection().await?;
    let mut conn_clone_three = redis.get_async_connection().await?;
    let clusters_clone = clusters.clone();
    tokio::spawn(async move {
        join!(
            cache::run_jobs(&mut conn_clone, clusters_clone.as_slice()),
            cache::run_cleanups(&mut conn_clone_two),
            metrics::run_jobs(&mut conn_clone_three, clusters_clone.as_slice()),
        )
    });

    let user_id = get_current_user(&mut conn).await?.map(|user| user.id);
    for (cluster, events) in clusters.clone().into_iter().zip(events.into_iter()) {
        let cluster_clone = cluster.clone();
        tokio::spawn(async move {
            cluster_clone.up().await;
        });

        let mut conn_clone = redis.get_async_connection().await?;
        let cluster_clone = cluster.clone();
        let channel_clone = channel.clone();
        tokio::spawn(async move {
            handler::outgoing(
                &mut conn_clone,
                &cluster_clone,
                &channel_clone,
                user_id,
                events,
            )
            .await;
        });
    }

    let channel_clone = channel_send.clone();
    let clusters_clone = clusters.clone();
    tokio::spawn(async move {
        handler::incoming(clusters_clone.as_slice(), &channel_clone).await;
    });

    ctrl_c().await?;

    info!("Shutting down");

    let mut sessions = HashMap::new();
    for cluster in clusters {
        for (key, value) in cluster.down_resumable().into_iter() {
            sessions.insert(
                key.to_string(),
                SessionInfo {
                    session_id: value.session_id,
                    sequence: value.sequence,
                },
            );
        }
    }

    cache::set(&mut conn, SESSIONS_KEY, &sessions).await?;

    Ok(())
}
