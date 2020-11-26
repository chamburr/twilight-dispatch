use crate::cache;
use crate::constants::{CACHE_DUMP_INTERVAL, STATUSES_KEY};
use crate::models::{ApiResult, StatusInfo};

use chrono::{Duration, Utc};
use redis::AsyncCommands;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::ops::Sub;
use tokio::time;
use tracing::warn;
use twilight_gateway::Cluster;

pub async fn get<T: DeserializeOwned>(
    conn: &mut redis::aio::Connection,
    key: &str,
) -> ApiResult<Option<T>> {
    let res: Option<String> = conn.get(key).await?;

    Ok(res
        .map(|value| serde_json::from_str(value.as_str()))
        .transpose()?)
}

pub async fn set<T: Serialize>(
    conn: &mut redis::aio::Connection,
    key: &str,
    value: &T,
) -> ApiResult<()> {
    let value = serde_json::to_string(value)?;

    Ok(conn.set(key, value).await?)
}

pub async fn run_jobs(conn: &mut redis::aio::Connection, cluster: &Cluster) {
    loop {
        let mut statuses: Vec<StatusInfo> = cluster
            .info()
            .iter()
            .map(|(k, v)| StatusInfo {
                shard: *k,
                status: format!("{}", v.stage()),
                session: v.session_id().unwrap_or_default().to_string(),
                latency: v
                    .latency()
                    .recent()
                    .back()
                    .map(|value| value.as_millis() as u64)
                    .unwrap_or_default(),
                last_ack: v
                    .latency()
                    .received()
                    .map(|value| {
                        Utc::now()
                            .naive_utc()
                            .sub(Duration::milliseconds(value.elapsed().as_millis() as i64))
                    })
                    .unwrap_or_else(|| Utc::now().naive_utc()),
            })
            .collect();

        statuses.sort_by(|a, b| a.shard.cmp(&b.shard));

        if let Err(err) = cache::set(conn, STATUSES_KEY, &statuses).await {
            warn!("Failed to dump gateway statuses: {:?}", err);
        }

        time::delay_for(time::Duration::from_millis(CACHE_DUMP_INTERVAL as u64)).await;
    }
}
