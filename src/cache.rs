use crate::config::CONFIG;
use crate::constants::{
    channel_key, emoji_key, guild_key, member_key, message_key, presence_key, private_channel_key,
    role_key, voice_key, BOT_USER_KEY, CACHE_CLEANUP_INTERVAL, CACHE_DUMP_INTERVAL, CHANNEL_KEY,
    EMOJI_KEY, EXPIRY_KEYS, GUILD_KEY, KEYS_SUFFIX, MESSAGE_KEY, SESSIONS_KEY, STATUSES_KEY,
};
use crate::models::{ApiResult, FormattedDateTime, SessionInfo, StatusInfo};
use crate::utils::{get_guild_shard, get_keys, to_value};

use ::time::Duration;
use redis::{AsyncCommands, FromRedisValue};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_mappable_seq::Key;
use simd_json::owned::Value;
use std::collections::HashMap;
use std::hash::Hash;
use tokio::time;
use tracing::warn;
use twilight_gateway::Cluster;
use twilight_model::channel::{Channel, GuildChannel, Message, PrivateChannel, TextChannel};
use twilight_model::gateway::event::Event;
use twilight_model::guild::{Emoji, GuildStatus, Member};
use twilight_model::id::GuildId;

pub async fn get<T: DeserializeOwned>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
) -> ApiResult<Option<T>> {
    let res: Option<String> = conn.get(key.to_string()).await?;

    Ok(res
        .map(|mut value| simd_json::from_str(value.as_mut_str()))
        .transpose()?)
}

pub async fn get_members<T: FromRedisValue>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
) -> ApiResult<Vec<T>> {
    let res = conn.smembers(key.to_string()).await?;

    Ok(res)
}

pub async fn get_members_len(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
) -> ApiResult<u64> {
    let res = conn.scard(key.to_string()).await?;

    Ok(res)
}

pub async fn get_hashmap<T: FromRedisValue + Eq + Hash, U: FromRedisValue>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
) -> ApiResult<HashMap<T, U>> {
    let res = conn.hgetall(key.to_string()).await?;

    Ok(res)
}

pub async fn set<T: Serialize>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
    value: &T,
) -> ApiResult<()> {
    let value = simd_json::to_string(value)?;
    conn.set(key.to_string(), value).await?;

    let parts = get_keys(key.to_string());

    if parts.len() > 1 {
        let _: () = conn
            .sadd(format!("{}{}", parts[0], KEYS_SUFFIX), key.to_string())
            .await?;
    }

    if parts.len() > 2 {
        if parts[0] != MESSAGE_KEY {
            let _: () = conn
                .sadd(
                    format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, parts[1]),
                    key.to_string(),
                )
                .await?;
        } else {
            let _: () = conn
                .sadd(
                    format!("{}{}:{}", CHANNEL_KEY, KEYS_SUFFIX, parts[1]),
                    key.to_string(),
                )
                .await?;
        }
    }

    Ok(())
}

pub async fn set_and_expire<T: Serialize>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
    value: &T,
    expiry: u64,
) -> ApiResult<()> {
    set(conn, key.to_string(), value).await?;

    if expiry != 0 {
        let timestamp = FormattedDateTime::now() + Duration::milliseconds(expiry as i64);
        conn.hset(
            EXPIRY_KEYS,
            key.to_string(),
            simd_json::to_string(&timestamp)?,
        )
        .await?;
    }

    Ok(())
}

pub async fn del_all(conn: &mut redis::aio::Connection, keys: &[String]) -> ApiResult<()> {
    if keys.is_empty() {
        return Ok(());
    }

    conn.del(keys).await?;

    let mut all_keys = HashMap::new();

    for key in keys {
        let parts = get_keys(key.to_owned());

        if parts.len() > 1 {
            all_keys
                .entry(format!("{}{}", parts[0], KEYS_SUFFIX))
                .or_insert_with(Vec::new)
                .push(key);
        }

        if parts.len() > 2 {
            if parts[0] != MESSAGE_KEY {
                all_keys
                    .entry(format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, parts[1]))
                    .or_insert_with(Vec::new)
                    .push(key);
            } else {
                all_keys
                    .entry(format!("{}{}:{}", CHANNEL_KEY, KEYS_SUFFIX, parts[1]))
                    .or_insert_with(Vec::new)
                    .push(key);
            }
        }
    }

    for (key, value) in all_keys {
        conn.srem(key, value).await?;
    }

    Ok(())
}

pub async fn del<T: ToString>(conn: &mut redis::aio::Connection, key: T) -> ApiResult<()> {
    del_all(conn, &[key.to_string()]).await?;

    Ok(())
}

pub async fn del_hashmap(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
    fields: &[String],
) -> ApiResult<()> {
    if fields.is_empty() {
        return Ok(());
    }

    let _: () = conn.hdel(key.to_string(), fields).await?;

    Ok(())
}

pub async fn run_jobs(conn: &mut redis::aio::Connection, clusters: &[Cluster]) {
    loop {
        let mut statuses = vec![];
        let mut sessions = HashMap::new();

        for cluster in clusters {
            let mut status: Vec<StatusInfo> = cluster
                .info()
                .iter()
                .map(|(k, v)| StatusInfo {
                    shard: *k,
                    status: format!("{}", v.stage()),
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
                            FormattedDateTime::now()
                                - Duration::milliseconds(value.elapsed().as_millis() as i64)
                        })
                        .unwrap_or_else(FormattedDateTime::now),
                })
                .collect();

            statuses.append(&mut status);

            for (shard, info) in cluster.info() {
                sessions.insert(
                    shard.to_string(),
                    SessionInfo {
                        session_id: info.session_id().unwrap_or_default().to_owned(),
                        sequence: info.seq(),
                    },
                );
            }
        }

        statuses.sort_by(|a, b| a.shard.cmp(&b.shard));

        if let Err(err) = set(conn, STATUSES_KEY, &statuses).await {
            warn!("Failed to dump gateway statuses: {:?}", err);
        }

        if let Err(err) = set(conn, SESSIONS_KEY, &sessions).await {
            warn!("Failed to dump gateway sessions: {:?}", err);
        }

        time::delay_for(time::Duration::from_millis(CACHE_DUMP_INTERVAL as u64)).await;
    }
}

pub async fn run_cleanups(conn: &mut redis::aio::Connection) {
    loop {
        let hashmap: ApiResult<HashMap<String, String>> = get_hashmap(conn, EXPIRY_KEYS).await;

        match hashmap {
            Ok(hashmap) => {
                let mut keys = vec![];

                for (key, mut value) in hashmap {
                    match simd_json::from_str::<FormattedDateTime>(value.as_mut_str()) {
                        Ok(timestamp) => {
                            if (timestamp - FormattedDateTime::now()).is_negative() {
                                keys.push(key);
                            }
                        }
                        Err(err) => {
                            warn!("Failed to get expiry timestamp: {:?}", err);
                        }
                    }
                }

                if let Err(err) = del_all(conn, keys.as_slice()).await {
                    warn!("Failed to delete expired keys: {:?}", err);
                } else if let Err(err) = del_hashmap(conn, EXPIRY_KEYS, keys.as_slice()).await {
                    warn!("Failed to delete expired keys hashmap: {:?}", err);
                }
            }
            Err(err) => {
                warn!("Failed to get expiry keys: {:?}", err);
            }
        }

        time::delay_for(time::Duration::from_millis(CACHE_CLEANUP_INTERVAL as u64)).await;
    }
}

async fn clear_guild<T: DeserializeOwned>(
    conn: &mut redis::aio::Connection,
    guild_id: GuildId,
) -> ApiResult<Option<T>> {
    let members: Vec<String> =
        get_members(conn, format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, guild_id)).await?;

    del_all(conn, members.as_slice()).await?;

    let guild = get(conn, guild_key(guild_id)).await?;
    del(conn, guild_key(guild_id)).await?;

    Ok(guild)
}

pub async fn update(conn: &mut redis::aio::Connection, event: &Event) -> ApiResult<Option<Value>> {
    let mut old: Option<Value> = None;

    match event {
        Event::ChannelCreate(data) => match &data.0 {
            Channel::Private(c) => {
                set(conn, private_channel_key(c.id), c).await?;
            }
            Channel::Guild(c) => {
                set(conn, channel_key(c.guild_id().unwrap(), c.id()), c).await?;
            }
            _ => {}
        },
        Event::ChannelDelete(data) => match &data.0 {
            Channel::Private(c) => {
                old = get(conn, private_channel_key(c.id)).await?;
                del(conn, private_channel_key(c.id)).await?;
            }
            Channel::Guild(c) => {
                old = get(conn, channel_key(c.guild_id().unwrap(), c.id())).await?;
                del(conn, channel_key(c.guild_id().unwrap(), c.id())).await?;
            }
            _ => {}
        },
        Event::ChannelPinsUpdate(data) => match data.guild_id {
            Some(guild_id) => {
                let channel: Option<TextChannel> =
                    get(conn, channel_key(guild_id, data.channel_id)).await?;
                if let Some(mut channel) = channel {
                    channel.last_pin_timestamp = data.last_pin_timestamp.clone();
                    set(conn, channel_key(guild_id, data.channel_id), &channel).await?;
                }
            }
            None => {
                let channel: Option<PrivateChannel> =
                    get(conn, private_channel_key(data.channel_id)).await?;
                if let Some(mut channel) = channel {
                    channel.last_pin_timestamp = data.last_pin_timestamp.clone();
                    set(conn, private_channel_key(data.channel_id), &channel).await?;
                }
            }
        },
        Event::ChannelUpdate(data) => match &data.0 {
            Channel::Private(c) => {
                old = get(conn, private_channel_key(c.id)).await?;
                set(conn, private_channel_key(c.id), c).await?;
            }
            Channel::Guild(c) => {
                old = get(conn, channel_key(c.guild_id().unwrap(), c.id())).await?;
                set(conn, channel_key(c.guild_id().unwrap(), c.id()), c).await?;
            }
            _ => {}
        },
        Event::GuildCreate(data) => {
            old = clear_guild(conn, data.id).await?;
            for channel in data.channels.values() {
                let channel = match channel.clone() {
                    GuildChannel::Category(mut c) => {
                        c.guild_id = Some(data.id);
                        GuildChannel::Category(c)
                    }
                    GuildChannel::Text(mut c) => {
                        c.guild_id = Some(data.id);
                        GuildChannel::Text(c)
                    }
                    GuildChannel::Voice(mut c) => {
                        c.guild_id = Some(data.id);
                        GuildChannel::Voice(c)
                    }
                };
                set(conn, channel_key(data.id, channel.id()), &channel).await?;
            }
            for role in data.roles.values() {
                set(conn, role_key(data.id, role.id), &role).await?;
            }
            for emoji in data.emojis.values() {
                set(conn, emoji_key(data.id, emoji.id), &emoji).await?;
            }
            for voice in data.voice_states.values() {
                set(conn, voice_key(data.id, voice.user_id), &voice).await?;
            }
            if CONFIG.state_member {
                for member in data.members.values() {
                    set_and_expire(
                        conn,
                        member_key(data.id, member.user.id),
                        &member,
                        CONFIG.state_member_ttl,
                    )
                    .await?;
                }
            }
            if CONFIG.state_presence {
                for presence in data.presences.values() {
                    set(conn, presence_key(data.id, presence.user.key()), &presence).await?;
                }
            }
            let mut guild = data.clone();
            guild.channels = HashMap::new();
            guild.roles = HashMap::new();
            guild.emojis = HashMap::new();
            guild.voice_states = HashMap::new();
            guild.members = HashMap::new();
            guild.presences = HashMap::new();
            set(conn, guild_key(data.id), &data).await?;
        }
        Event::GuildDelete(data) => {
            old = clear_guild(conn, data.id).await?;
        }
        Event::GuildEmojisUpdate(data) => {
            let mut emojis = vec![];
            let keys: Vec<String> = get_members(
                conn,
                format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, data.guild_id),
            )
            .await?;
            for key in keys {
                if get_keys(key.clone())[0] == EMOJI_KEY {
                    let emoji: Option<Emoji> = get(conn, key.as_str()).await?;
                    if let Some(emoji) = emoji {
                        emojis.push(emoji.clone());
                        if data.emojis.get(&emoji.id).is_none() {
                            del(conn, key).await?;
                        }
                    }
                }
            }
            old = Some(to_value(&emojis)?);
            for emoji in data.emojis.values() {
                set(conn, emoji_key(data.guild_id, emoji.id), emoji).await?
            }
        }
        Event::GuildUpdate(data) => {
            old = get(conn, guild_key(data.id)).await?;
            set(conn, guild_key(data.id), &data).await?;
        }
        Event::MemberAdd(data) => {
            if CONFIG.state_member {
                set_and_expire(
                    conn,
                    member_key(data.guild_id, data.user.id),
                    &data,
                    CONFIG.state_member_ttl,
                )
                .await?;
            }
        }
        Event::MemberRemove(data) => {
            if CONFIG.state_member {
                old = get(conn, member_key(data.guild_id, data.user.id)).await?;
                del(conn, member_key(data.guild_id, data.user.id)).await?;
            }
            if CONFIG.state_presence {
                del(conn, presence_key(data.guild_id, data.user.id)).await?;
            }
        }
        Event::MemberUpdate(data) => {
            if CONFIG.state_member {
                let member: Option<Member> =
                    get(conn, member_key(data.guild_id, data.user.id)).await?;
                if let Some(mut member) = member {
                    old = Some(to_value(&member)?);
                    member.joined_at = Some(data.joined_at.clone());
                    member.nick = data.nick.clone();
                    member.premium_since = data.premium_since.clone();
                    member.roles = data.roles.clone();
                    member.user = data.user.clone();
                    set_and_expire(
                        conn,
                        member_key(data.guild_id, data.user.id),
                        &member,
                        CONFIG.state_member_ttl,
                    )
                    .await?;
                }
            }
        }
        Event::MemberChunk(data) => {
            if CONFIG.state_member {
                for member in data.members.values() {
                    set_and_expire(
                        conn,
                        member_key(data.guild_id, member.user.id),
                        &member,
                        CONFIG.state_member_ttl,
                    )
                    .await?;
                }
            }
        }
        Event::MessageCreate(data) => {
            if CONFIG.state_message {
                set_and_expire(
                    conn,
                    message_key(data.channel_id, data.id),
                    &data,
                    CONFIG.state_message_ttl,
                )
                .await?;
            }
        }
        Event::MessageDelete(data) => {
            if CONFIG.state_message {
                old = get(conn, message_key(data.channel_id, data.id)).await?;
                del(conn, message_key(data.channel_id, data.id)).await?;
            }
        }
        Event::MessageDeleteBulk(data) => {
            if CONFIG.state_message {
                let mut messages = vec![];
                for id in &data.ids {
                    let message: Option<Message> =
                        get(conn, message_key(data.channel_id, *id)).await?;
                    if let Some(message) = message {
                        messages.push(message);
                        del(conn, message_key(data.channel_id, *id)).await?;
                    }
                }
                old = Some(to_value(&messages)?);
            }
        }
        Event::MessageUpdate(data) => {
            if CONFIG.state_message {
                let message: Option<Message> =
                    get(conn, message_key(data.channel_id, data.id)).await?;
                if let Some(mut message) = message {
                    old = Some(to_value(&message)?);
                    if let Some(attachments) = &data.attachments {
                        message.attachments = attachments.clone();
                    }
                    if let Some(content) = &data.content {
                        message.content = content.clone();
                    }
                    if let Some(edited_timestamp) = &data.edited_timestamp {
                        message.edited_timestamp = Some(edited_timestamp.clone());
                    }
                    if let Some(embeds) = &data.embeds {
                        message.embeds = embeds.clone();
                    }
                    if let Some(mention_everyone) = data.mention_everyone {
                        message.mention_everyone = mention_everyone;
                    }
                    if let Some(mention_roles) = &data.mention_roles {
                        message.mention_roles = mention_roles.clone();
                    }
                    if let Some(mentions) = &data.mentions {
                        message.mentions = mentions
                            .iter()
                            .map(|user| (user.id, user.clone()))
                            .collect();
                    }
                    if let Some(pinned) = data.pinned {
                        message.pinned = pinned;
                    }
                    if let Some(timestamp) = &data.timestamp {
                        message.timestamp = timestamp.clone();
                    }
                    if let Some(tts) = data.tts {
                        message.tts = tts;
                    }
                    set_and_expire(
                        conn,
                        message_key(data.channel_id, data.id),
                        &message,
                        CONFIG.state_message_ttl,
                    )
                    .await?;
                }
            }
        }
        Event::PresenceUpdate(data) => {
            if CONFIG.state_presence {
                old = get(conn, presence_key(data.guild_id, data.user.key())).await?;
                set(conn, presence_key(data.guild_id, data.user.key()), &data).await?;
            }
        }
        Event::Ready(data) => {
            set(conn, BOT_USER_KEY, &data.user).await?;
            if let Some(shards) = data.shard {
                for guild in get_members(conn, format!("{}{}", GUILD_KEY, KEYS_SUFFIX)).await? {
                    let id = GuildId(get_keys(guild)[1].parse()?);
                    if get_guild_shard(id) == shards[0] && data.guilds.get(&id).is_none() {
                        let _: Option<Value> = clear_guild(conn, id).await?;
                    }
                }
            }
            for guild in data.guilds.values() {
                if let GuildStatus::Offline(guild) = guild {
                    set(conn, guild_key(guild.id), guild).await?;
                }
            }
        }
        Event::RoleCreate(data) => {
            set(conn, role_key(data.guild_id, data.role.id), &data.role).await?;
        }
        Event::RoleDelete(data) => {
            old = get(conn, role_key(data.guild_id, data.role_id)).await?;
            del(conn, role_key(data.guild_id, data.role_id)).await?;
        }
        Event::RoleUpdate(data) => {
            old = get(conn, role_key(data.guild_id, data.role.id)).await?;
            set(conn, role_key(data.guild_id, data.role.id), &data.role).await?;
        }
        Event::UnavailableGuild(data) => {
            old = clear_guild(conn, data.id).await?;
            set(conn, guild_key(data.id), data).await?;
        }
        Event::UserUpdate(data) => {
            old = get(conn, BOT_USER_KEY).await?;
            set(conn, BOT_USER_KEY, &data).await?;
        }
        Event::VoiceStateUpdate(data) => {
            if let Some(guild_id) = data.0.guild_id {
                old = get(conn, voice_key(guild_id, data.0.user_id)).await?;
                match data.0.channel_id {
                    Some(_) => {
                        set(conn, voice_key(guild_id, data.0.user_id), &data.0).await?;
                    }
                    None => {
                        del(conn, voice_key(guild_id, data.0.user_id)).await?;
                    }
                }
            }
        }
        _ => {}
    }

    Ok(old)
}
