import io
import logging
import textwrap
import traceback

from contextlib import redirect_stdout
from discord.ext import commands

log = logging.getLogger(__name__)


class General(commands.Cog):
    def __init__(self, bot):
        self.bot = bot

    @commands.command(description="Get started with the bot.")
    async def help(self, ctx):
        await ctx.send("This is a sample bot using twilight-dispatch: https://github.com/chamburr/twilight-dispatch")

    @commands.command(description="Play ping pong!")
    async def ping(self, ctx):
        await ctx.send("Pong! 🏓")

    @commands.is_owner()
    @commands.command(name="eval", description="Evaluate code and play around.")
    async def _eval(self, ctx, *, code: str):
        env = {"bot": self.bot, "ctx": ctx}
        env.update(globals())
        stdout = io.StringIO()

        try:
            exec(f"async def func():\n{textwrap.indent(code, '  ')}", env)
        except Exception as e:
            await ctx.send(f"```py\n{e.__class__.__name__}: {e}\n```")
            return

        try:
            with redirect_stdout(stdout):
                result = await env["func"]()
        except Exception:
            await ctx.send(f"```py\n{stdout.getvalue()}{traceback.format_exc()}\n```")
        else:
            await ctx.send(f"```py\n{stdout.getvalue()}{result}\n```")


def setup(bot):
    bot.add_cog(General(bot))
