import logging
import orjson

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

    @commands.Cog.listener()
    async def on_reaction_add(self, reaction, member):
        if reaction.emoji not in ["◀️", "▶️"]:
            return
        if member.bot:
            return
        menus = await self.bot._connection._get("reaction_menus") or []
        for (index, menu) in enumerate(menus):
            channel = menu["channel"]
            message = menu["message"]
            if reaction.message.channel.id != channel or reaction.message.id != message:
                continue
            page = menu["page"]
            all_pages = menu["all_pages"]
            if reaction.emoji == "◀️" and page > 0:
                page -= 1
            elif reaction.emoji == "▶️" and page < len(all_pages) - 1:
                page += 1
            await self.bot.http.edit_message(channel, message, content=all_pages[page])
            menu["page"] = page
            menus[index] = menu
            await self.bot._connection.redis.set("reaction_menus", orjson.dumps(menus).decode("utf-8"))
            break

def setup(bot):
    bot.add_cog(Events(bot))
