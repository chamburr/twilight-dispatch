use hyper::{http::Error as HyperHttpError, Error as HyperError};
use lapin::Error as LapinError;
use prometheus::Error as PrometheusError;
use redis::RedisError;
use serde::{de::Error as SerdeDeError, Deserialize, Deserializer, Serialize, Serializer};
use serde_repr::{Deserialize_repr, Serialize_repr};
use simd_json::{owned::Value, Error as SimdJsonError};
use std::{
    env::VarError,
    error::Error,
    fmt::{self, Display, Formatter},
    io::Error as IoError,
    net::AddrParseError,
    num::ParseIntError,
    ops::{Add, Sub},
};
use time::{format_description, Duration, OffsetDateTime};
use twilight_gateway::cluster::ClusterStartError;
use twilight_http::{response::DeserializeBodyError, Error as TwilightHttpError};
use twilight_model::{
    channel::Channel,
    gateway::{payload::incoming::GuildCreate, presence::Presence, OpCode},
    guild::{Emoji, Member, Role},
    voice::VoiceState,
};

#[derive(Debug, Clone)]
pub struct FormattedDateTime(OffsetDateTime);

impl FormattedDateTime {
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }
}

impl Sub<Duration> for FormattedDateTime {
    type Output = Self;

    fn sub(self, rhs: Duration) -> Self::Output {
        Self(self.0.sub(rhs))
    }
}

impl Sub<FormattedDateTime> for FormattedDateTime {
    type Output = Duration;

    fn sub(self, rhs: FormattedDateTime) -> Self::Output {
        self.0.sub(rhs.0)
    }
}

impl Add<Duration> for FormattedDateTime {
    type Output = Self;

    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.0.add(rhs))
    }
}

impl Serialize for FormattedDateTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let format =
            format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]")
                .unwrap();
        serializer.serialize_str(&self.0.format(&format).unwrap())
    }
}

impl<'de> Deserialize<'de> for FormattedDateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)? + "+00";
        let format = format_description::parse(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]+[offset_hour]",
        )
        .unwrap();
        match OffsetDateTime::parse(string.as_str(), &format) {
            Ok(dt) => Ok(Self(dt)),
            Err(_) => Err(SerdeDeError::custom("not a valid formatted timestamp")),
        }
    }

    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = String::deserialize(deserializer)? + "+00";
        let format = format_description::parse(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]+[offset_hour]",
        )
        .unwrap();
        match OffsetDateTime::parse(string.as_str(), &format) {
            Ok(dt) => {
                place.0 = dt;
                Ok(())
            }
            Err(_) => Err(SerdeDeError::custom("not a valid formatted timestamp")),
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
    pub latency: u64,
    pub last_ack: FormattedDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PayloadInfo {
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

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum GuildItem {
    Guild(Box<GuildCreate>),
    Channel(Channel),
    Role(Role),
    Emoji(Emoji),
    Voice(VoiceState),
    Member(Member),
    Presence(Presence),
}

pub type ApiResult<T> = Result<T, ApiError>;

macro_rules! make_api_errors {
    ($($name:ident($ty:ty)),*) => {
        #[derive(Debug)]
        pub enum ApiError {
            $($name($ty)),*
        }

        $(
            impl From<$ty> for ApiError {
                fn from(err: $ty) -> Self {
                    Self::$name(err)
                }
            }
        )*
    }
}

make_api_errors! {
    Empty(()),
    SimdJson(SimdJsonError),
    Redis(RedisError),
    Var(VarError),
    ParseInt(ParseIntError),
    Lapin(LapinError),
    ClusterStart(ClusterStartError),
    Hyper(HyperError),
    HyperHttp(HyperHttpError),
    AddrParse(AddrParseError),
    Prometheus(PrometheusError),
    Io(IoError),
    TwilightHttp(TwilightHttpError),
    DeserializeBody(DeserializeBodyError)
}

impl Error for ApiError {}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
