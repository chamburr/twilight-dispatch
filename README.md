# twilight-dispatch

A standalone service for connecting to the Discord gateway, written in Rust with the
[twilight](https://github.com/twilight-rs/twilight) crate. This allows the gateway logic to be
separate from your application layer, bringing many benefits such as the following.

1. Minimal downtime. You will almost never need to restart the gateway service, allowing absolute
   100% uptime for your bot. Even when a restart is required, twilight-dispatch will resume the
   sessions, so you will not lose a single event.

2. Flexibility and scalability. Since the events received are in the exchange, you can route them to
   different queues based on event type and consume them from multiple processes. This allows for
   load balancing between your service workers.

If you encounter any issues while running the service, please feel free create an issue here, or you
can contact CHamburr#2591 on Discord. We will try our best to help you.

## Changes in fork
#### Docker Image notice
Docker Images has more options for CPU Optimization other than `haswell` only (upstream), including `znver2`, which our infrastructure runs on.

## Features

-   Low CPU and RAM footprint
-   Large bot sharding support
-   Resumable sessions after restart
-   Discord channel status logging
-   Prometheus metrics
-   Shard information in Redis
-   State cache with Redis
-   Docker container support

## Implementation

### Events

Gateway events are forwarded to and from RabbitMQ.

Events are sent to a topic exchange `gateway`, with the event name as the routing key. By default,
there is a `gateway.recv` channel bound to all messages from the exchange. The decoded and
decompressed dispatch events from the gateway will be available in the queue.

To send events to the gateway, connect to the channel `gateway.send`, then publish a message like
the following. Note that the outermost `op` is not the Discord gateway OP code. The only option for
now is 0, but there may be others in the future to reconnect to a shard, etc.

```json
{
    "op": 0,
    "shard": 0,
    "data": {
        "op": 4,
        "d": {
            "guild_id": "41771983423143937",
            "channel_id": "127121515262115840",
            "self_mute": false,
            "self_deaf": false
        }
    }
}
```

### State Cache

State caching with Redis is supported out of the box.

The objects available are in the table below. All values are stored in the plain text form, and you
will need to properly deserialize them before using. Some objects such as presence and member could
be missing if you disable them in the configurations.

Furthermore, when the old state option is enabled, there will be an additional field for gateway
events in the message queue, `old`, containing the previous state (only if it exists). This could
be useful for the `MESSAGE_DELETE` event and such.

| Key                             | Description                      |
| ------------------------------- | -------------------------------- |
| `bot_user`                      | Bot user object.                 |
| `guild:guild_id`                | Guild object.                    |
| `role:guild_id:role_id`         | Guild role object.               |
| `emoji:guild_id:emoji_id`       | Guild emoji object.              |
| `member:guild_id:user_id`       | Guild member object.             |
| `presence:guild_id:user_id`     | Guild member presence object.    |
| `voice:guild_id:user_id`        | Guild member voice state object. |
| `channel:channel_id`            | Channel object.                  |
| `message:channel_id:message_id` | Channel message object.          |

There are additionally some helper keys for state cache below, stored as sets.

| Key                       | Description                            |
| ------------------------- | -------------------------------------- |
| `guild_keys`              | List of guild keys.                    |
| `guild_keys:guild_id`     | List of keys related to a guild.       |
| `channel_keys`            | List of guild channel keys.            |
| `role_keys`               | List of guild role keys.               |
| `emoji_keys`              | List of guild emoji keys.              |
| `member_keys`             | List of guild member keys.             |
| `presence_keys`           | List of guild member presence keys.    |
| `voice_keys`              | List of guild member voice state keys. |
| `channel_keys:channel_id` | List of keys related to a channel.     |
| `message_keys`            | List of channel message keys.          |

### Information

Information related to the gateway are stored in Redis.

| Key                | Description                         |
| ------------------ | ----------------------------------- |
| `gateway_sessions` | Array of shard session information. |
| `gateway_statuses` | Array of shard status information.  |
| `gateway_started`  | Timestamp when the service started. |
| `gateway_shards`   | Total number of shards being ran.   |

## Installing

These are the steps to installing and running the service.

### Prerequisites

-   [RabbitMQ](https://www.rabbitmq.com/download.html)
-   [Redis](https://redis.io/download)
-   [Rust](https://www.rust-lang.org/tools/install)

### Configuration

The gateway can be configured with environmental variables or a `.env` file at the root of the
project. An example can be found [here](.env.example).

### Running

Run the following commands to start the service.

```
cargo build --release
cargo run --release
```

### Running (Docker)

If you prefer, the service can also be ran with Docker. Run the following commands to start the
container.

```
docker build -t twilight-dispatch:latest .
docker run -it --network host --env-file .env twilight-dispatch:latest
```

The Docker image is also available here: https://hub.docker.com/r/chamburr/twilight-dispatch.

Note: You do not need to install Rust if you are using Docker.

## Related Resources

Twilight: https://github.com/twilight-rs/twilight

## License

This project is licensed under [ISC](LICENSE).
