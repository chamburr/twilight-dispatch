use crate::config::CONFIG;
use crate::constants::METRICS_DUMP_INTERVAL;
use crate::models::ApiResult;

use hyper::header::CONTENT_TYPE;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use lazy_static::lazy_static;
use prometheus::{
    register_int_counter_vec, register_int_gauge, register_int_gauge_vec, Encoder, IntCounterVec,
    IntGauge, IntGaugeVec, TextEncoder,
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use tokio::time;
use twilight_gateway::shard::Stage;
use twilight_gateway::Cluster;

lazy_static! {
    pub static ref GATEWAY_EVENTS: IntCounterVec = register_int_counter_vec!(
        "gateway_events",
        "Events received through the Discord gateway",
        &["type", "shard"]
    )
    .unwrap();
    pub static ref SHARD_EVENTS: IntCounterVec = register_int_counter_vec!(
        "gateway_shard_events",
        "Discord shard connection events",
        &["type"]
    )
    .unwrap();
    pub static ref GATEWAY_SHARDS: IntGauge = register_int_gauge!(
        "gateway_shards",
        "Number of gateway connections with Discord"
    )
    .unwrap();
    pub static ref GATEWAY_STATUS: IntGaugeVec = register_int_gauge_vec!(
        "gateway_status",
        "Status of the gateway connections",
        &["type"]
    )
    .unwrap();
    pub static ref GATEWAY_LATENCY: IntGaugeVec = register_int_gauge_vec!(
        "gateway_latency",
        "API latency with the Discord gateway",
        &["shard"]
    )
    .unwrap();
}

async fn serve(req: Request<Body>) -> ApiResult<Response<Body>> {
    if req.method() == Method::GET && req.uri().path() == "/metrics" {
        let mut buffer = vec![];
        let metrics = prometheus::gather();

        let encoder = TextEncoder::new();
        encoder.encode(metrics.as_slice(), &mut buffer)?;

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, encoder.format_type())
            .body(Body::from(buffer))?)
    } else {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())?)
    }
}

pub async fn run_server() -> ApiResult<()> {
    let addr = SocketAddr::new(
        IpAddr::from_str(CONFIG.prometheus_host.as_str())?,
        CONFIG.prometheus_port as u16,
    );

    let make_svc = make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(serve)) });

    Server::bind(&addr).serve(make_svc).await?;

    Err(().into())
}

pub async fn run_jobs(clusters: &[Cluster]) {
    loop {
        let mut shards = vec![];
        for cluster in clusters {
            shards.append(&mut cluster.shards())
        }

        GATEWAY_SHARDS.set(shards.len() as i64);

        let mut statuses = HashMap::new();
        statuses.insert(format!("{}", Stage::Connected), 0);
        statuses.insert(format!("{}", Stage::Disconnected), 0);
        statuses.insert(format!("{}", Stage::Handshaking), 0);
        statuses.insert(format!("{}", Stage::Identifying), 0);
        statuses.insert(format!("{}", Stage::Resuming), 0);

        for shard in shards {
            if let Ok(info) = shard.info() {
                GATEWAY_LATENCY
                    .with_label_values(&[info.id().to_string().as_str()])
                    .set(
                        info.latency()
                            .recent()
                            .back()
                            .map(|value| value.as_millis() as i64)
                            .unwrap_or_default(),
                    );

                *statuses
                    .get_mut(format!("{}", info.stage()).as_str())
                    .unwrap() += 1;
            }
        }

        for (stage, amount) in statuses {
            GATEWAY_STATUS
                .with_label_values(&[stage.as_str()])
                .set(amount);
        }

        time::delay_for(time::Duration::from_millis(METRICS_DUMP_INTERVAL as u64)).await;
    }
}
