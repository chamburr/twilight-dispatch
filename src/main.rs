#![recursion_limit = "128"]
#![deny(clippy::all, nonstandard_style, rust_2018_idioms, unused, warnings)]

use crate::{
    cluster::get_clusters,
    config::CONFIG,
    constants::{SESSIONS_KEY, SHARDS_KEY, STARTED_KEY},
    models::{ApiResult, FormattedDateTime, SessionInfo},
    utils::{declare_queues, get_current_user},
};

use dotenv::dotenv;
use lapin::ConnectionProperties;
use std::collections::HashMap;
use tokio::{join, signal::ctrl_c};
use tracing::{error, info};

mod cache;
mod cluster;
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

    let mut conn = redis.get_multiplexed_tokio_connection().await?;

    let amqp = lapin::Connection::connect(
        format!(
            "amqp://{}:{}@{}:{}/%2f",
            CONFIG.rabbit_username, CONFIG.rabbit_password, CONFIG.rabbit_host, CONFIG.rabbit_port
        )
        .as_str(),
        ConnectionProperties::default(),
    )
    .await?;

    let channel = amqp.create_channel().await?;
    let channel_send = amqp.create_channel().await?;
    declare_queues(&channel, &channel_send).await?;

    let user = get_current_user().await?;
    let (clusters, events, info) = get_clusters(&mut conn).await?;

    info!("Starting up {} clusters", info.clusters);
    info!("Starting up {} shards", info.shards);
    info!("Resuming {} sessions", info.resumes);

    cache::set(&mut conn, STARTED_KEY, &FormattedDateTime::now()).await?;
    cache::set(&mut conn, SHARDS_KEY, &CONFIG.shards_total).await?;

    tokio::spawn(async {
        let _ = metrics::run_server().await;
    });

    let conn_clone = conn.clone();
    let clusters_clone = clusters.clone();
    tokio::spawn(async move {
        join!(
            cache::run_jobs(conn_clone.clone(), clusters_clone.as_slice()),
            metrics::run_jobs(conn_clone.clone(), clusters_clone.as_slice()),
        )
    });

    for (cluster, events) in clusters.clone().into_iter().zip(events.into_iter()) {
        let cluster_clone = cluster.clone();
        tokio::spawn(async move {
            cluster_clone.up().await;
        });

        let conn_clone = redis.get_multiplexed_tokio_connection().await?;
        let cluster_clone = cluster.clone();
        let channel_clone = channel.clone();
        tokio::spawn(async move {
            handler::outgoing(conn_clone, cluster_clone, channel_clone, user.id, events).await;
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
