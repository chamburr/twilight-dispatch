import asyncio
import config
import logging

from classes.bot import Bot

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger()
logger.setLevel(logging.INFO)

log = logging.getLogger(__name__)

bot = Bot(
    command_prefix=config.prefix,
    case_insensitive=True,
    help_command=None,
    owner_id=config.owner,
)

loop = asyncio.get_event_loop()
loop.run_until_complete(bot.start())
