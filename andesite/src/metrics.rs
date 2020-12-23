use crate::config::CONFIG;
use crate::models::ApiResult;

use hyper::header::CONTENT_TYPE;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use lazy_static::lazy_static;
use prometheus::{register_int_counter_vec, Encoder, IntCounterVec, TextEncoder};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

lazy_static! {
    pub static ref PLAYER_EVENTS: IntCounterVec = register_int_counter_vec!(
        "player_events",
        "Events received through Andesite",
        &["type"]
    )
    .unwrap();
    pub static ref PLAYED_TRACKS: IntCounterVec = register_int_counter_vec!(
        "played_tracks",
        "All the tracks played with Andesite",
        &["title", "length"]
    )
    .unwrap();
    pub static ref VOICE_CLOSES: IntCounterVec = register_int_counter_vec!(
        "voice_closes",
        "Discord voice gateway close events",
        &["code"]
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
