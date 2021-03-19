use crate::{
    config::CONFIG,
    constants::{
        channel_key, emoji_key, guild_key, member_key, message_key, presence_key,
        private_channel_key, role_key, voice_key, BOT_USER_KEY, CACHE_CLEANUP_INTERVAL,
        CACHE_DUMP_INTERVAL, CHANNEL_KEY, EMOJI_KEY, EXPIRY_KEYS, GUILD_KEY, KEYS_SUFFIX,
        MESSAGE_KEY, SESSIONS_KEY, STATUSES_KEY,
    },
    models::{ApiError, ApiResult, FormattedDateTime, GuildItem, SessionInfo, StatusInfo},
    utils::{get_keys, get_user_id, to_value},
};

use redis::{AsyncCommands, FromRedisValue};
use serde::{de::DeserializeOwned, Serialize};
use simd_json::owned::Value;
use std::{collections::HashMap, hash::Hash};
use tokio::time::{sleep, Duration};
use tracing::warn;
use twilight_gateway::Cluster;
use twilight_model::{
    channel::{Channel, GuildChannel, Message, PrivateChannel, TextChannel},
    gateway::event::Event,
    guild::{Emoji, Member, UnavailableGuild},
    id::GuildId,
};

pub async fn get<T: DeserializeOwned>(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
) -> ApiResult<Option<T>> {
    let res: Option<String> = conn.get(key.to_string()).await?;

    Ok(res
        .map(|mut value| simd_json::from_str(value.as_mut_str()))
        .transpose()?)
}

pub async fn get_all<T: DeserializeOwned>(
    conn: &mut redis::aio::Connection,
    keys: &[String],
) -> ApiResult<Vec<Option<T>>> {
    if keys.is_empty() {
        return Ok(vec![]);
    }

    let res: Vec<Option<String>> = conn.get(keys).await?;

    Ok(res
        .into_iter()
        .map(|option| {
            option
                .map(|mut value| simd_json::from_str(value.as_mut_str()).map_err(ApiError::from))
                .transpose()
        })
        .collect::<ApiResult<Vec<Option<T>>>>()?)
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
    set_all(conn, &[(key.to_string(), value)]).await?;

    Ok(())
}

pub async fn set_all<T: Serialize>(
    conn: &mut redis::aio::Connection,
    keys: &[(String, T)],
) -> ApiResult<()> {
    if keys.is_empty() {
        return Ok(());
    }

    let mut members = HashMap::new();

    let keys = keys
        .iter()
        .map(|(key, value)| {
            let parts = get_keys(key);
            if parts.len() > 1 {
                members
                    .entry(format!("{}{}", parts[0], KEYS_SUFFIX))
                    .or_insert_with(Vec::new)
                    .push(key);
            }
            if parts.len() > 2 && parts[0] != MESSAGE_KEY {
                members
                    .entry(format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, parts[1]))
                    .or_insert_with(Vec::new)
                    .push(key);
            } else if parts.len() > 2 {
                members
                    .entry(format!("{}{}:{}", CHANNEL_KEY, KEYS_SUFFIX, parts[1]))
                    .or_insert_with(Vec::new)
                    .push(key);
            }

            simd_json::to_string(value)
                .map(|value| (key, value))
                .map_err(ApiError::from)
        })
        .collect::<ApiResult<Vec<(&String, String)>>>()?;

    conn.set_multiple(keys.as_slice()).await?;

    for (key, value) in members {
        conn.sadd(key, value.as_slice()).await?;
    }

    Ok(())
}

pub async fn expire(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
    expiry: u64,
) -> ApiResult<()> {
    expire_all(conn, &[(key.to_string(), expiry)]).await?;

    Ok(())
}

pub async fn expire_all(
    conn: &mut redis::aio::Connection,
    keys: &[(String, u64)],
) -> ApiResult<()> {
    if keys.is_empty() {
        return Ok(());
    }

    let keys = keys
        .iter()
        .map(|(key, value)| {
            let timestamp = FormattedDateTime::now() + time::Duration::milliseconds(*value as i64);
            simd_json::to_string(&timestamp)
                .map(|value| (key, value))
                .map_err(ApiError::from)
        })
        .collect::<ApiResult<Vec<(&String, String)>>>()?;

    conn.hset_multiple(EXPIRY_KEYS, keys.as_slice()).await?;

    Ok(())
}

pub async fn del_all(conn: &mut redis::aio::Connection, keys: &[String]) -> ApiResult<()> {
    if keys.is_empty() {
        return Ok(());
    }

    conn.del(keys).await?;

    let mut all_keys = HashMap::new();

    for key in keys {
        let parts = get_keys(key);

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

pub async fn del(conn: &mut redis::aio::Connection, key: impl ToString) -> ApiResult<()> {
    del_all(conn, &[key.to_string()]).await?;

    Ok(())
}

pub async fn del_hashmap(
    conn: &mut redis::aio::Connection,
    key: impl ToString,
    keys: &[String],
) -> ApiResult<()> {
    if keys.is_empty() {
        return Ok(());
    }

    let _: () = conn.hdel(key.to_string(), keys).await?;

    Ok(())
}

pub async fn run_jobs(conn: &mut redis::aio::Connection, clusters: &[Cluster]) {
    loop {
        let mut statuses = vec![];
        let mut sessions = HashMap::new();

        for cluster in clusters {
            let mut status: Vec<StatusInfo> = cluster
                .info()
                .into_iter()
                .map(|(k, v)| StatusInfo {
                    shard: k,
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
                                - time::Duration::milliseconds(value.elapsed().as_millis() as i64)
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

        sleep(Duration::from_millis(CACHE_DUMP_INTERVAL as u64)).await;
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

        sleep(Duration::from_millis(CACHE_CLEANUP_INTERVAL as u64)).await;
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

pub async fn update(
    conn: &mut redis::aio::Connection,
    event: &Event,
) -> ApiResult<(Option<Value>, redis::Pipeline)> {
    let mut old: Option<Value> = None;
    let mut pipe = redis::pipe();

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
            let mut items = vec![];
            let mut guild = data.clone();
            for mut channel in guild.channels.drain(..) {
                match &mut channel {
                    GuildChannel::Category(channel) => {
                        channel.guild_id = Some(data.id);
                    }
                    GuildChannel::Text(channel) => {
                        channel.guild_id = Some(data.id);
                    }
                    GuildChannel::Voice(channel) => {
                        channel.guild_id = Some(data.id);
                    }
                }
                items.push((
                    channel_key(data.id, channel.id()),
                    GuildItem::Channel(channel),
                ));
            }
            for role in guild.roles.drain(..) {
                items.push((role_key(data.id, role.id), GuildItem::Role(role)));
            }
            for emoji in guild.emojis.drain(..) {
                items.push((emoji_key(data.id, emoji.id), GuildItem::Emoji(emoji)));
            }
            for voice in guild.voice_states.drain(..) {
                items.push((voice_key(data.id, voice.user_id), GuildItem::Voice(voice)));
            }
            for member in guild.members.drain(..) {
                if CONFIG.state_member {
                    items.push((
                        member_key(data.id, member.user.id),
                        GuildItem::Member(member),
                    ));
                }
            }
            for presence in guild.presences.drain(..) {
                let id = get_user_id(&presence.user);
                if CONFIG.state_presence {
                    items.push((presence_key(data.id, id), GuildItem::Presence(presence)));
                }
            }
            items.push((guild_key(data.id), GuildItem::Guild(guild)));

            let mut members = HashMap::new();

            let items = items
                .iter()
                .map(|(key, value)| {
                    let parts = get_keys(key);
                    members
                        .entry(format!("{}{}", parts[0], KEYS_SUFFIX))
                        .or_insert_with(Vec::new)
                        .push(key);
                    members
                        .entry(format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, parts[1]))
                        .or_insert_with(Vec::new)
                        .push(key);

                    simd_json::to_string(value)
                        .map(|value| (key, value))
                        .map_err(ApiError::from)
                })
                .collect::<ApiResult<Vec<(&String, String)>>>()?;

            pipe = pipe.set_multiple(items.as_slice()).ignore().clone();

            for (key, value) in members {
                pipe = pipe.sadd(key, value.as_slice()).ignore().clone();
            }

            if CONFIG.state_member {
                let keys = data
                    .members
                    .iter()
                    .map(|member| {
                        let key = member_key(data.id, member.user.id);
                        let timestamp = FormattedDateTime::now()
                            + time::Duration::milliseconds(CONFIG.state_member_ttl as i64);
                        simd_json::to_string(&timestamp)
                            .map(|value| (key, value))
                            .map_err(ApiError::from)
                    })
                    .collect::<ApiResult<Vec<(String, String)>>>()?;

                pipe = pipe
                    .hset_multiple(EXPIRY_KEYS, keys.as_slice())
                    .ignore()
                    .clone();
            }
        }
        Event::GuildDelete(data) => {
            old = clear_guild(conn, data.id).await?;
        }
        Event::GuildEmojisUpdate(data) => {
            let keys: Vec<String> = get_members(
                conn,
                format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, data.guild_id),
            )
            .await?;
            let emoji_keys: Vec<String> = keys
                .into_iter()
                .filter(|key| get_keys(key)[0] == EMOJI_KEY)
                .collect();
            let emojis: Vec<Emoji> = get_all(conn, emoji_keys.as_slice())
                .await?
                .into_iter()
                .flatten()
                .collect();
            del_all(
                conn,
                emojis
                    .iter()
                    .filter(|emoji| !data.emojis.iter().any(|e| e.id == emoji.id))
                    .map(|emoji| emoji_key(data.guild_id, emoji.id))
                    .collect::<Vec<String>>()
                    .as_slice(),
            )
            .await?;
            set_all(
                conn,
                data.emojis
                    .iter()
                    .map(|emoji| (emoji_key(data.guild_id, emoji.id), emoji))
                    .collect::<Vec<(String, &Emoji)>>()
                    .as_slice(),
            )
            .await?;
            old = Some(to_value(&emojis)?);
        }
        Event::GuildUpdate(data) => {
            old = get(conn, guild_key(data.id)).await?;
            set(conn, guild_key(data.id), &data).await?;
        }
        Event::MemberAdd(data) => {
            if CONFIG.state_member {
                set(conn, member_key(data.guild_id, data.user.id), &data).await?;
                expire(
                    conn,
                    member_key(data.guild_id, data.user.id),
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
                    set(conn, member_key(data.guild_id, data.user.id), &member).await?;
                    expire(
                        conn,
                        member_key(data.guild_id, data.user.id),
                        CONFIG.state_member_ttl,
                    )
                    .await?;
                }
            }
        }
        Event::MemberChunk(data) => {
            if CONFIG.state_member {
                set_all(
                    conn,
                    data.members
                        .iter()
                        .map(|member| (member_key(data.guild_id, member.user.id), member))
                        .collect::<Vec<(String, &Member)>>()
                        .as_slice(),
                )
                .await?;
                expire_all(
                    conn,
                    data.members
                        .iter()
                        .map(|member| {
                            (
                                member_key(data.guild_id, member.user.id),
                                CONFIG.state_member_ttl,
                            )
                        })
                        .collect::<Vec<(String, u64)>>()
                        .as_slice(),
                )
                .await?;
            }
        }
        Event::MessageCreate(data) => {
            if CONFIG.state_message {
                set(conn, message_key(data.channel_id, data.id), &data).await?;
                expire(
                    conn,
                    message_key(data.channel_id, data.id),
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
                let message_keys: Vec<String> = data
                    .ids
                    .iter()
                    .map(|id| message_key(data.channel_id, *id))
                    .collect();
                let messages: Vec<Message> = get_all(conn, message_keys.as_slice())
                    .await?
                    .into_iter()
                    .flatten()
                    .collect();
                del_all(conn, message_keys.as_slice()).await?;
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
                        message.mentions = mentions.clone();
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
                    set(conn, message_key(data.channel_id, data.id), &message).await?;
                    expire(
                        conn,
                        message_key(data.channel_id, data.id),
                        CONFIG.state_message_ttl,
                    )
                    .await?;
                }
            }
        }
        Event::PresenceUpdate(data) => {
            if CONFIG.state_presence {
                old = get(conn, presence_key(data.guild_id, get_user_id(&data.user))).await?;
                set(
                    conn,
                    presence_key(data.guild_id, get_user_id(&data.user)),
                    &data,
                )
                .await?;
            }
        }
        Event::Ready(data) => {
            set(conn, BOT_USER_KEY, &data.user).await?;
            set_all(
                conn,
                data.guilds
                    .iter()
                    .map(|guild| (guild_key(guild.id), guild))
                    .collect::<Vec<(String, &UnavailableGuild)>>()
                    .as_slice(),
            )
            .await?;
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

    Ok((old, pipe))
}
