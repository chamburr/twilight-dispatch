import logging

from discord.ext import commands

log = logging.getLogger(__name__)


class Events(commands.Cog):
    def __init__(self, bot):
        self.bot = bot

    @commands.Cog.listener()
    async def on_ready(self):
        user = await self.bot.user()
        log.info(f"{user.name}#{user.discriminator} is ready")
        log.info("--------")

    @commands.Cog.listener()
    async def on_socket_raw_receive(self, message):
        log.debug("[Receive] " + message.decode("utf-8"))

    @commands.Cog.listener()
    async def on_socket_raw_send(self, message):
        log.debug("[Send] " + message.decode("utf-8"))


def setup(bot):
    bot.add_cog(Events(bot))
