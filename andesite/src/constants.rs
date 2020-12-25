use twilight_model::id::GuildId;

pub const EXCHANGE: &str = "player";
pub const QUEUE_RECV: &str = "player.recv";
pub const QUEUE_SEND: &str = "player.send";

pub const PLAYER_BUFFER: usize = 300000;
pub const PLAYER_EXPIRY: usize = 30000;

pub const PLAYER_KEY: &str = "player";
pub const PLAYER_ID_KEY: &str = "player_id";
pub const PLAYER_STATS_KEY: &str = "player_stats";

pub fn player_key(id: GuildId) -> String {
    format!("{}:{}", PLAYER_KEY, id)
}
