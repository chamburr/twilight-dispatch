import asyncio
import logging

import config

from classes.bot import Bot

logging.basicConfig(level=logging.DEBUG)
logger = logging.getLogger()
logger.setLevel(logging.DEBUG)

log = logging.getLogger(__name__)

bot = Bot(
    command_prefix=config.prefix,
    case_insensitive=True,
    help_command=None,
    owner_id=config.owner,
)


loop = asyncio.get_event_loop()
loop.run_until_complete(bot.start())
