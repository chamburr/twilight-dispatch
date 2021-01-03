use hyper::http::Error as HyperHTTPError;
use hyper::Error as HyperError;
use lapin::Error as LapinError;
use prometheus::Error as PrometheusError;
use redis::RedisError;
use reqwest::Error as ReqwestError;
use serde::export::Formatter;
use serde::{Deserialize, Serialize};
use simd_json::Error as SimdJsonError;
use std::error::Error;
use std::fmt::{self, Display};
use std::net::AddrParseError;
use twilight_andesite::model::Filters;
use twilight_andesite::node::NodeError;
use twilight_model::id::GuildId;

#[derive(Debug, Deserialize, Serialize)]
pub struct PayloadInfo {
    pub op: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Player {
    pub guild_id: GuildId,
    pub time: i64,
    pub position: Option<i64>,
    pub paused: bool,
    pub volume: i64,
    pub filters: Filters,
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub enum ApiError {
    EmptyError(()),
    SimdJsonError(SimdJsonError),
    RedisError(RedisError),
    LapinError(LapinError),
    HyperError(HyperError),
    HyperHTTPError(HyperHTTPError),
    AddrParseError(AddrParseError),
    PrometheusError(PrometheusError),
    ReqwestError(ReqwestError),
    NodeError(NodeError),
}

impl Error for ApiError {}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<()> for ApiError {
    fn from(_: ()) -> Self {
        Self::EmptyError(())
    }
}

impl From<SimdJsonError> for ApiError {
    fn from(err: SimdJsonError) -> Self {
        Self::SimdJsonError(err)
    }
}

impl From<RedisError> for ApiError {
    fn from(err: RedisError) -> Self {
        Self::RedisError(err)
    }
}

impl From<LapinError> for ApiError {
    fn from(err: LapinError) -> Self {
        Self::LapinError(err)
    }
}

impl From<HyperError> for ApiError {
    fn from(err: HyperError) -> Self {
        Self::HyperError(err)
    }
}

impl From<HyperHTTPError> for ApiError {
    fn from(err: HyperHTTPError) -> Self {
        Self::HyperHTTPError(err)
    }
}

impl From<AddrParseError> for ApiError {
    fn from(err: AddrParseError) -> Self {
        Self::AddrParseError(err)
    }
}

impl From<PrometheusError> for ApiError {
    fn from(err: PrometheusError) -> Self {
        Self::PrometheusError(err)
    }
}

impl From<ReqwestError> for ApiError {
    fn from(err: ReqwestError) -> Self {
        Self::ReqwestError(err)
    }
}

impl From<NodeError> for ApiError {
    fn from(err: NodeError) -> Self {
        Self::NodeError(err)
    }
}
