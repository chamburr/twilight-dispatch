use hyper::http::Error as HyperHTTPError;
use hyper::Error as HyperError;
use lapin::Error as LapinError;
use prometheus::Error as PrometheusError;
use redis::RedisError;
use serde::export::Formatter;
use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use serde_repr::{Deserialize_repr, Serialize_repr};
use simd_json::owned::Value;
use simd_json::Error as SimdJsonError;
use std::env::VarError;
use std::error::Error;
use std::fmt::{self, Display};
use std::io::Error as IoError;
use std::net::AddrParseError;
use std::num::ParseIntError;
use std::ops::Sub;
use time::{Duration, OffsetDateTime};
use twilight_gateway::cluster::ClusterStartError;
use twilight_gateway::shard::LargeThresholdError;
use twilight_model::gateway::OpCode;

#[derive(Debug, Clone)]
pub struct FormattedOffsetDateTime(OffsetDateTime);

impl FormattedOffsetDateTime {
    pub fn now_utc() -> Self {
        Self(OffsetDateTime::now_utc())
    }
}

impl Sub<Duration> for FormattedOffsetDateTime {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self::Output {
        Self(self.0.sub(duration))
    }
}

impl Serialize for FormattedOffsetDateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.format("%FT%T.%N"))
    }
}

impl<'de> Deserialize<'de> for FormattedOffsetDateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        match OffsetDateTime::parse(string, "%FT%T.%N") {
            Ok(dt) => Ok(Self(dt)),
            Err(_) => Err(DeError::custom("not a valid formatted timestamp")),
        }
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)?;
        match OffsetDateTime::parse(string, "%FT%T.%N") {
            Ok(dt) => {
                place.0 = dt;
                Ok(())
            }
            Err(_) => Err(DeError::custom("not a valid formatted timestamp")),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StatusInfo {
    pub shard: u64,
    pub status: String,
    pub session: String,
    pub latency: u64,
    pub last_ack: FormattedOffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PayloadInfo {
    pub op: OpCode,
    pub t: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PayloadData {
    pub op: OpCode,
    pub t: Option<String>,
    pub d: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<Value>,
}

#[derive(Clone, Debug, Deserialize_repr, Serialize_repr)]
#[repr(u8)]
pub enum DeliveryOpcode {
    Send,
    Reconnect,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeliveryInfo {
    pub op: DeliveryOpcode,
    pub shard: u64,
    pub data: Option<Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub enum ApiError {
    EmptyError(()),
    SimdJsonError(SimdJsonError),
    RedisError(RedisError),
    VarError(VarError),
    ParseIntError(ParseIntError),
    LapinError(LapinError),
    ClusterStartError(ClusterStartError),
    LargeThresholdError(LargeThresholdError),
    HyperError(HyperError),
    HyperHTTPError(HyperHTTPError),
    AddrParseError(AddrParseError),
    PrometheusError(PrometheusError),
    IoError(IoError),
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

impl From<VarError> for ApiError {
    fn from(err: VarError) -> Self {
        Self::VarError(err)
    }
}

impl From<ParseIntError> for ApiError {
    fn from(err: ParseIntError) -> Self {
        Self::ParseIntError(err)
    }
}

impl From<LapinError> for ApiError {
    fn from(err: LapinError) -> Self {
        Self::LapinError(err)
    }
}

impl From<ClusterStartError> for ApiError {
    fn from(err: ClusterStartError) -> Self {
        Self::ClusterStartError(err)
    }
}

impl From<LargeThresholdError> for ApiError {
    fn from(err: LargeThresholdError) -> Self {
        Self::LargeThresholdError(err)
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

impl From<IoError> for ApiError {
    fn from(err: IoError) -> Self {
        Self::IoError(err)
    }
}
