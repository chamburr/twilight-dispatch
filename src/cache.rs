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

use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use serde::{de::DeserializeOwned, Serialize};
use simd_json::owned::Value;
use std::{collections::HashMap, hash::Hash, iter};
use tokio::time::{sleep, Duration};
use tracing::warn;
use twilight_gateway::Cluster;
use twilight_model::{
    channel::{Channel, GuildChannel, Message, PrivateChannel, TextChannel},
    gateway::event::Event,
    guild::{Emoji, Member},
    id::{GuildId, UserId},
};

pub async fn get<K, T>(conn: &mut redis::aio::Connection, key: K) -> ApiResult<Option<T>>
where
    K: ToRedisArgs + Send + Sync,
    T: DeserializeOwned,
{
    let res: Option<String> = conn.get(key).await?;

    Ok(res
        .map(|mut value| simd_json::from_str(value.as_mut_str()))
        .transpose()?)
}

pub async fn get_all<K, T>(
    conn: &mut redis::aio::Connection,
    keys: &[K],
) -> ApiResult<Vec<Option<T>>>
where
    K: ToRedisArgs + Send + Sync,
    T: DeserializeOwned,
{
    if keys.is_empty() {
        return Ok(vec![]);
    }

    let res: Vec<Option<String>> = conn.get(keys).await?;

    res.into_iter()
        .map(|option| {
            option
                .map(|mut value| simd_json::from_str(value.as_mut_str()).map_err(ApiError::from))
                .transpose()
        })
        .collect()
}

pub async fn get_members<K, T>(conn: &mut redis::aio::Connection, key: K) -> ApiResult<Vec<T>>
where
    K: ToRedisArgs + Send + Sync,
    T: FromRedisValue,
{
    let res = conn.smembers(key).await?;

    Ok(res)
}

pub async fn get_members_len<K>(conn: &mut redis::aio::Connection, key: K) -> ApiResult<u64>
where
    K: ToRedisArgs + Send + Sync,
{
    let res = conn.scard(key).await?;

    Ok(res)
}

pub async fn get_hashmap<K, T, U>(
    conn: &mut redis::aio::Connection,
    key: K,
) -> ApiResult<HashMap<T, U>>
where
    K: ToRedisArgs + Send + Sync,
    T: FromRedisValue + Eq + Hash,
    U: FromRedisValue,
{
    let res = conn.hgetall(key).await?;

    Ok(res)
}

pub async fn set<K, T>(conn: &mut redis::aio::Connection, key: K, value: T) -> ApiResult<()>
where
    K: AsRef<str>,
    T: Serialize,
{
    set_all(conn, iter::once((key, value))).await?;

    Ok(())
}

pub async fn set_all<I, K, T>(conn: &mut redis::aio::Connection, keys: I) -> ApiResult<()>
where
    I: IntoIterator<Item = (K, T)>,
    K: AsRef<str>,
    T: Serialize,
{
    let mut members = HashMap::new();

    let keys = keys
        .into_iter()
        .map(|(key, value)| {
            let key = key.as_ref();
            let parts = get_keys(key);

            let new_key = if parts.len() > 2 && parts[0] == CHANNEL_KEY {
                format!("{}:{}", parts[0], parts[2])
            } else {
                key.to_owned()
            };

            if parts.len() > 1 {
                members
                    .entry(format!("{}{}", parts[0], KEYS_SUFFIX))
                    .or_insert_with(Vec::new)
                    .push(new_key.clone());
            }

            if parts.len() > 2 && parts[0] != MESSAGE_KEY {
                members
                    .entry(format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, parts[1]))
                    .or_insert_with(Vec::new)
                    .push(new_key.clone());
            } else if parts.len() > 2 {
                members
                    .entry(format!("{}{}:{}", CHANNEL_KEY, KEYS_SUFFIX, parts[1]))
                    .or_insert_with(Vec::new)
                    .push(new_key.clone());
            }

            simd_json::to_string(&value)
                .map(|value| (new_key, value))
                .map_err(ApiError::from)
        })
        .collect::<ApiResult<Vec<(String, String)>>>()?;

    if keys.is_empty() {
        return Ok(());
    }

    conn.set_multiple(keys.as_slice()).await?;

    for (key, value) in members {
        conn.sadd(key, value.as_slice()).await?;
    }

    Ok(())
}

pub async fn expire<K>(conn: &mut redis::aio::Connection, key: K, expiry: u64) -> ApiResult<()>
where
    K: ToRedisArgs + Send + Sync,
{
    expire_all(conn, iter::once((key, expiry))).await?;

    Ok(())
}

pub async fn expire_all<I, K>(conn: &mut redis::aio::Connection, keys: I) -> ApiResult<()>
where
    I: IntoIterator<Item = (K, u64)>,
    K: ToRedisArgs + Send + Sync,
{
    let keys = keys
        .into_iter()
        .map(|(key, value)| {
            let timestamp = FormattedDateTime::now() + time::Duration::milliseconds(value as i64);

            simd_json::to_string(&timestamp)
                .map(|value| (key, value))
                .map_err(ApiError::from)
        })
        .collect::<ApiResult<Vec<(K, String)>>>()?;

    if keys.is_empty() {
        return Ok(());
    }

    conn.hset_multiple(EXPIRY_KEYS, keys.as_slice()).await?;

    Ok(())
}

pub async fn del_all<I, K>(conn: &mut redis::aio::Connection, keys: I) -> ApiResult<()>
where
    I: IntoIterator<Item = K>,
    K: AsRef<str>,
{
    let mut members = HashMap::new();

    let keys = keys
        .into_iter()
        .map(|key| {
            let key = key.as_ref();
            let parts = get_keys(key);

            let new_key = if parts.len() > 2 && parts[0] == CHANNEL_KEY {
                format!("{}:{}", parts[0], parts[2])
            } else {
                key.to_owned()
            };

            if parts.len() > 1 {
                members
                    .entry(format!("{}{}", parts[0], KEYS_SUFFIX))
                    .or_insert_with(Vec::new)
                    .push(new_key.clone());
            }

            if parts.len() > 2 {
                if parts[0] != MESSAGE_KEY {
                    members
                        .entry(format!("{}{}:{}", GUILD_KEY, KEYS_SUFFIX, parts[1]))
                        .or_insert_with(Vec::new)
                        .push(new_key.clone());
                } else {
                    members
                        .entry(format!("{}{}:{}", CHANNEL_KEY, KEYS_SUFFIX, parts[1]))
                        .or_insert_with(Vec::new)
                        .push(new_key.clone());
                }
            }

            new_key
        })
        .collect::<Vec<String>>();

    if keys.is_empty() {
        return Ok(());
    }

    conn.del(keys).await?;

    for (key, value) in members {
        conn.srem(key, value).await?;
    }

    Ok(())
}

pub async fn del(conn: &mut redis::aio::Connection, key: impl AsRef<str>) -> ApiResult<()> {
    del_all(conn, iter::once(key)).await?;

    Ok(())
}

pub async fn del_hashmap<K>(
    conn: &mut redis::aio::Connection,
    key: K,
    keys: &[String],
) -> ApiResult<()>
where
    K: ToRedisArgs + Send + Sync,
{
    if keys.is_empty() {
        return Ok(());
    }

    let _: () = conn.hdel(key, keys).await?;

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

    del_all(conn, members).await?;

    let guild = get(conn, guild_key(guild_id)).await?;
    del(conn, guild_key(guild_id)).await?;

    Ok(guild)
}

pub async fn update(
    conn: &mut redis::aio::Connection,
    event: &Event,
    bot_id: UserId,
) -> ApiResult<Option<Value>> {
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
                    GuildChannel::Stage(channel) => {
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
                if CONFIG.state_member || member.user.id == bot_id {
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

            set_all(conn, items).await?;
            if CONFIG.state_member {
                expire_all(
                    conn,
                    data.members.iter().map(|member| {
                        (member_key(data.id, member.user.id), CONFIG.state_member_ttl)
                    }),
                )
                .await?;
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
                    .map(|emoji| emoji_key(data.guild_id, emoji.id)),
            )
            .await?;
            set_all(
                conn,
                data.emojis
                    .iter()
                    .map(|emoji| (emoji_key(data.guild_id, emoji.id), emoji)),
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
            if CONFIG.state_member || data.user.id == bot_id {
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
                        .map(|member| (member_key(data.guild_id, member.user.id), member)),
                )
                .await?;
                expire_all(
                    conn,
                    data.members.iter().map(|member| {
                        (
                            member_key(data.guild_id, member.user.id),
                            CONFIG.state_member_ttl,
                        )
                    }),
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
                del_all(conn, message_keys).await?;
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
                data.guilds.iter().map(|guild| (guild_key(guild.id), guild)),
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

    Ok(old)
}
