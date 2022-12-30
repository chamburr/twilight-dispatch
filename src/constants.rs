use twilight_model::id::{
    marker::{ChannelMarker, EmojiMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker},
    Id,
};

pub const EXCHANGE: &str = "gateway";
pub const QUEUE_RECV: &str = "gateway.recv";
pub const QUEUE_SEND: &str = "gateway.send";

pub const SESSIONS_KEY: &str = "gateway_sessions";
pub const STATUSES_KEY: &str = "gateway_statuses";
pub const STARTED_KEY: &str = "gateway_started";
pub const SHARDS_KEY: &str = "gateway_shards";

pub const BOT_USER_KEY: &str = "bot_user";
pub const GUILD_KEY: &str = "guild";
pub const CHANNEL_KEY: &str = "channel";
pub const MESSAGE_KEY: &str = "message";
pub const ROLE_KEY: &str = "role";
pub const EMOJI_KEY: &str = "emoji";
pub const MEMBER_KEY: &str = "member";
pub const PRESENCE_KEY: &str = "presence";
pub const VOICE_KEY: &str = "voice";

pub const KEYS_SUFFIX: &str = "_keys";
pub const EXPIRY_KEYS: &str = "expiry_keys";

pub const CACHE_JOB_INTERVAL: usize = 1000;
pub const METRICS_JOB_INTERVAL: usize = 1000;

pub const CONNECT_COLOR: usize = 0x00FF00;
pub const DISCONNECT_COLOR: usize = 0xFF0000;
pub const READY_COLOR: usize = 0x00FF00;
pub const RESUME_COLOR: usize = 0x1E90FF;
pub const JOIN_COLOR: usize = 0x00FF00;
pub const LEAVE_COLOR: usize = 0xFF0000;

pub fn guild_key(guild: Id<GuildMarker>) -> String {
    format!("{}:{}", GUILD_KEY, guild)
}

pub fn channel_key(guild: Option<Id<GuildMarker>>, channel: Id<ChannelMarker>) -> String {
    if let Some(guild) = guild {
        guild_channel_key(guild, channel)
    } else {
        private_channel_key(channel)
    }
}

fn guild_channel_key(guild: Id<GuildMarker>, channel: Id<ChannelMarker>) -> String {
    format!("{}:{}:{}", CHANNEL_KEY, guild, channel)
}

fn private_channel_key(channel: Id<ChannelMarker>) -> String {
    format!("{}:{}", CHANNEL_KEY, channel)
}

pub fn message_key(channel: Id<ChannelMarker>, message: Id<MessageMarker>) -> String {
    format!("{}:{}:{}", MESSAGE_KEY, channel, message)
}

pub fn role_key(guild: Id<GuildMarker>, role: Id<RoleMarker>) -> String {
    format!("{}:{}:{}", ROLE_KEY, guild, role)
}

pub fn emoji_key(guild: Id<GuildMarker>, emoji: Id<EmojiMarker>) -> String {
    format!("{}:{}:{}", EMOJI_KEY, guild, emoji)
}

pub fn member_key(guild: Id<GuildMarker>, member: Id<UserMarker>) -> String {
    format!("{}:{}:{}", MEMBER_KEY, guild, member)
}

pub fn presence_key(guild: Id<GuildMarker>, member: Id<UserMarker>) -> String {
    format!("{}:{}:{}", PRESENCE_KEY, guild, member)
}

pub fn voice_key(guild: Id<GuildMarker>, member: Id<UserMarker>) -> String {
    format!("{}:{}:{}", VOICE_KEY, guild, member)
}
