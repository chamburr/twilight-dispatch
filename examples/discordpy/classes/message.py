from discord import utils, message
from discord.embeds import Embed
from discord.enums import MessageType, try_enum
from discord.flags import MessageFlags
from discord.guild import Guild
from discord.member import Member
from discord.message import flatten_handlers, Attachment, MessageReference
from discord.reaction import Reaction


@flatten_handlers
class Message(message.Message):
    def __init__(self, *, state, channel, data):
        self._state = state
        self.id = int(data["id"])
        self.webhook_id = utils._get_as_snowflake(data, "webhook_id")
        self.reactions = [Reaction(message=self, data=d) for d in data.get("reactions", [])]
        self.attachments = [Attachment(data=a, state=self._state) for a in data["attachments"]]
        self.embeds = [Embed.from_dict(a) for a in data["embeds"]]
        self.application = data.get("application")
        self.activity = data.get("activity")
        self.channel = channel
        self._edited_timestamp = utils.parse_time(data["edited_timestamp"])
        self.type = try_enum(MessageType, data["type"])
        self.pinned = data["pinned"]
        self.flags = MessageFlags._from_value(data.get("flags", 0))
        self.mention_everyone = data["mention_everyone"]
        self.tts = data["tts"]
        self.content = data["content"]
        self.nonce = data.get("nonce")

        ref = data.get("message_reference")
        self.reference = MessageReference(state, **ref) if ref is not None else None

        for handler in ("call", "flags"):
            try:
                getattr(self, "_handle_%s" % handler)(data[handler])
            except KeyError:
                continue

    async def _fill_data(self, data):
        try:
            author = data["author"]
            self.author = self._state.store_user(author)
            if isinstance(self.guild, Guild):
                found = await self.guild.get_member(self.author.id)
                if found is not None:
                    self.author = found
        except KeyError:
            pass

        try:
            member = data["member"]
            author = self.author
            try:
                author._update_from_message(member)
            except AttributeError:
                self.author = Member._from_message(message=self, data=member)
        except KeyError:
            pass

        try:
            mentions = data["mentions"]
            self.mentions = r = []
            guild = self.guild
            state = self._state
            if not isinstance(guild, Guild):
                self.mentions = [state.store_user(m) for m in mentions]
            else:
                for mention in filter(None, mentions):
                    id_search = int(mention["id"])
                    member = await guild.get_member(id_search)
                    if member is not None:
                        r.append(member)
                    else:
                        r.append(Member._try_upgrade(data=mention, guild=guild, state=state))
        except KeyError:
            pass

        try:
            role_mentions = data["mention_roles"]
            self.role_mentions = []
            if isinstance(self.guild, Guild):
                for role_id in map(int, role_mentions):
                    role = await self.guild.get_role(role_id)
                    if role is not None:
                        self.role_mentions.append(role)
        except KeyError:
            pass
