# twilight-dispatch

A standalone service for connecting to the Discord gateway, written in Rust with
the [twilight](https://github.com/twilight-rs/twilight) crate. This allows the gateway logic to be
separate from your application layer, bringing many benefits such as the following.

1. Minimal downtime. You will almost never need to restart the gateway service, allowing absolute
100% uptime for your bot. Even when a restart is required, twilight-dispatch will resume the
sessions, so you will not lose any message.

2. Flexibility and scalability. Since the events received are all in the same message queue, you can
consume them from multiple processes. This allows for load balancing between your service workers.

If you encounter issues while running the service, feel free to contact CHamburr#2591 on Discord,
either through Direct Message or on the [Twilight server](https://discord.gg/7jj8n7D).

## Features

- Low CPU and RAM footprint
- Custom gateway API version
- Resumable sessions after restart
- Discord channel status logging
- Prometheus metrics
- Shard information in Redis

## Implementation

### Events

Gateway events are forwarded to and from RabbitMQ.

To receive events, connect to a channel `gateway`. The decoded and decompressed events from Discord
gateway will be available in the message queue. You should consume the events and send an ack in
response before processing it.

To send events to the gateway, connect to a channel `gateway.send`, then publish a message such as
the following. Note that the outer most `op` is not the Discord gateway OP code. The only option for
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

### Information

Information related to the gateway are stored in Redis.

| Key              | Description                         |
| ---------------- | ----------------------------------- |
| gateway_sessions | Array of shard session information. |
| gateway_statuses | Array of shard status information.  |
| gateway_started  | Timestamp when the service started. |
| gateway_shards   | Total number of shards being ran.   |

## Installing

These are the steps to installing and running the service.

### Prerequisites 

- Rust: https://www.rust-lang.org/tools/install
- RabbitMQ: https://www.rabbitmq.com/download.html
- Redis: https://redis.io/download

### Configuration

The gateway can be configured with environmental variables or a `.env` file at the root of the
project. An example can be found [here](.env.example).

### Running

Run the following commands to start the service.

```
cargo build --release
cargo run
```

## Related Resources

Twilight: https://github.com/twilight-rs/twilight

## License
This project is licensed under [ISC](LICENSE).
