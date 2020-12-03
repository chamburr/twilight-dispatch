use twilight_model::id::{ChannelId, EmojiId, GuildId, MessageId, RoleId, UserId};

pub const QUEUE_RECV: &str = "gateway";
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

pub const CACHE_DUMP_INTERVAL: usize = 1000;
pub const METRICS_DUMP_INTERVAL: usize = 1000;

pub const DISCONNECT_COLOR: usize = 0xFF0000;
pub const CONNECT_COLOR: usize = 0x00FF00;
pub const READY_COLOR: usize = 0x00FF00;
pub const RESUME_COLOR: usize = 0x1E90FF;

pub fn guild_key(guild: GuildId) -> String {
    format!("{}:{}", GUILD_KEY, guild)
}

pub fn guild_match_key() -> String {
    format!("{}:*", GUILD_KEY)
}

pub fn channel_key(guild: GuildId, channel: ChannelId) -> String {
    format!("{}:{}:{}", CHANNEL_KEY, guild, channel)
}

pub fn channel_match_key(guild: GuildId) -> String {
    format!("{}:{}:*", CHANNEL_KEY, guild)
}

pub fn private_channel_key(channel: ChannelId) -> String {
    format!("{}:{}", CHANNEL_KEY, channel)
}

pub fn message_key(channel: ChannelId, message: MessageId) -> String {
    format!("{}:{}:{}", MESSAGE_KEY, channel, message)
}

pub fn message_match_key(channel: ChannelId) -> String {
    format!("{}:{}:*", MESSAGE_KEY, channel)
}

pub fn role_key(guild: GuildId, role: RoleId) -> String {
    format!("{}:{}:{}", ROLE_KEY, guild, role)
}

pub fn role_match_key(guild: GuildId) -> String {
    format!("{}:{}:*", ROLE_KEY, guild)
}

pub fn emoji_key(guild: GuildId, emoji: EmojiId) -> String {
    format!("{}:{}:{}", EMOJI_KEY, guild, emoji)
}

pub fn emoji_match_key(guild: GuildId) -> String {
    format!("{}:{}:*", EMOJI_KEY, guild)
}

pub fn member_key(guild: GuildId, member: UserId) -> String {
    format!("{}:{}:{}", MEMBER_KEY, guild, member)
}

pub fn member_match_key(guild: GuildId) -> String {
    format!("{}:{}:*", MEMBER_KEY, guild)
}

pub fn presence_key(guild: GuildId, member: UserId) -> String {
    format!("{}:{}:{}", PRESENCE_KEY, guild, member)
}

pub fn presence_match_key(guild: GuildId) -> String {
    format!("{}:{}:*", PRESENCE_KEY, guild)
}

pub fn voice_key(guild: GuildId, member: UserId) -> String {
    format!("{}:{}:{}", VOICE_KEY, guild, member)
}

pub fn voice_match_key(guild: GuildId) -> String {
    format!("{}:{}:*", VOICE_KEY, guild)
}
