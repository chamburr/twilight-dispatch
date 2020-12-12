import asyncio
import aio_pika
import redis

import config
import inspect
import json
import logging
import sys
import traceback

from classes.state import State
from classes.misc import Status, Session
from discord.gateway import DiscordWebSocket
from discord.ext import commands
from discord.ext.commands import DefaultHelpCommand
from discord.ext.commands.core import _CaseInsensitiveDict
from discord.http import HTTPClient
from discord.utils import parse_time, to_json

log = logging.getLogger(__name__)


class Bot(commands.AutoShardedBot):
    def __init__(self, command_prefix, help_command=DefaultHelpCommand(), description=None, **kwargs):
        self.command_prefix = command_prefix
        self.extra_events = {}
        self._BotBase__cogs = {}
        self._BotBase__extensions = {}
        self._checks = []
        self._check_once = []
        self._before_invoke = None
        self._after_invoke = None
        self._help_command = None
        self.description = inspect.cleandoc(description) if description else ''
        self.owner_id = kwargs.get('owner_id')
        self.owner_ids = kwargs.get('owner_ids', set())
        self.help_command = help_command
        self.case_insensitive = kwargs.get('case_insensitive', False)
        self.all_commands = _CaseInsensitiveDict() if self.case_insensitive else {}

        self.ws = None
        self.loop = asyncio.get_event_loop()
        self.http = HTTPClient(None, loop=self.loop)

        self._handlers = {"ready": self._handle_ready}
        self._hooks = {}
        self._listeners = {}

        self._connection = None
        self._closed = False
        self._ready = asyncio.Event()

        self._redis = None
        self._amqp = None
        self._amqp_channel = None
        self._amqp_queue = None

    @property
    def config(self):
        return config

    @property
    def shard_count(self):
        return int(self._redis.get("gateway_shards"))

    @property
    def started(self):
        return parse_time(self._connection._get("gateway_started").split(".")[0])

    @property
    def statuses(self):
        statuses = self._connection._get("gateway_statuses")
        return [Status(x) for x in statuses]

    @property
    def sessions(self):
        sessions = self._connection._get("gateway_sessions")
        return {int(x): Session(y) for x, y in sessions.items()}

    def _get_state(self, **options):
        return State(
            dispatch=self.dispatch,
            handlers=self._handlers,
            hooks=self._hooks,
            loop=self.loop,
            redis=self._redis,
            shard_count=self.shard_count,
            **options,
        )

    async def receive_message(self, msg):
        self.ws._dispatch("socket_raw_receive", msg)

        msg = json.loads(msg)

        self.ws._dispatch("socket_response", msg)

        op = msg.get("op")
        data = msg.get("d")
        event = msg.get("t")
        old = msg.get("old")

        if op != self.ws.DISPATCH:
            print("here")
            return

        try:
            func = self.ws._discord_parsers[event]
        except KeyError:
            log.debug("Unknown event %s.", event)
        else:
            func(data, old)

        removed = []
        for index, entry in enumerate(self.ws._dispatch_listeners):
            if entry.event != event:
                continue

            future = entry.future
            if future.cancelled():
                removed.append(index)
                continue

            try:
                valid = entry.predicate(data)
            except Exception as exc:
                future.set_exception(exc)
                removed.append(index)
            else:
                if valid:
                    ret = data if entry.result is None else entry.result(data)
                    future.set_result(ret)
                    removed.append(index)

        for index in reversed(removed):
            del self.ws._dispatch_listeners[index]

    async def send_message(self, msg):
        data = to_json(msg)
        self.ws._dispatch("socket_raw_send", data)
        await self._amqp_channel.default_exchange.publish(aio_pika.Message(body=data), routing_key="gateway.send")

    async def start(self):
        self._redis = redis.Redis.from_url(self.config.redis_url, decode_responses=True)
        self._amqp = await aio_pika.connect_robust(self.config.amqp_url)
        self._amqp_channel = await self._amqp.channel()
        self._amqp_queue = await self._amqp_channel.get_queue("gateway.recv")

        self._connection = self._get_state()
        self._connection._get_client = lambda: self

        self.ws = DiscordWebSocket(socket=None, loop=self.loop)
        self.ws.token = self.http.token
        self.ws._connection = self._connection
        self.ws._discord_parsers = self._connection.parsers
        self.ws._dispatch = self.dispatch
        self.ws.call_hooks = self._connection.call_hooks

        for extension in self.config.cogs:
            try:
                self.load_extension("cogs." + extension)
            except Exception:
                log.error(f"Failed to load extension {extension}.", file=sys.stderr)
                log.error(traceback.print_exc())

        async with self._amqp_queue.iterator() as queue_iter:
            async for message in queue_iter:
                async with message.process(ignore_processed=True):
                    await self.receive_message(message.body)
                    message.ack()
