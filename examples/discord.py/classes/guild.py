import logging

from discord import guild, utils
from discord.enums import *
from discord.enums import try_enum
from discord.channel import _channel_factory
from discord.member import Member, VoiceState
from discord.role import Role

log = logging.getLogger(__name__)


class Guild(guild.Guild):
    def __init__(self, *, data, state):
        self._state = state
        self._from_data(data)

    def _add_channel(self, channel):
        return

    def _remove_channel(self, channel):
        return

    def _add_member(self, member):
        return

    def _remove_member(self, member):
        return

    def _update_voice_state(self, data, channel_id):
        return

    def _add_role(self, role):
        return

    def _remove_role(self, role_id):
        return

    def _from_data(self, guild):
        member_count = guild.get("member_count", None)
        if member_count is not None:
            self._member_count = member_count

        self.name = guild.get("name")
        self.region = try_enum(VoiceRegion, guild.get("region"))
        self.verification_level = try_enum(VerificationLevel, guild.get("verification_level"))
        self.default_notifications = try_enum(NotificationLevel, guild.get("default_message_notifications"))
        self.explicit_content_filter = try_enum(ContentFilter, guild.get("explicit_content_filter", 0))
        self.afk_timeout = guild.get("afk_timeout")
        self.icon = guild.get("icon")
        self.banner = guild.get("banner")
        self.unavailable = guild.get("unavailable", False)
        self.id = int(guild["id"])
        self.mfa_level = guild.get("mfa_level")
        self.features = guild.get("features", [])
        self.splash = guild.get("splash")
        self._system_channel_id = utils._get_as_snowflake(guild, "system_channel_id")
        self.description = guild.get("description")
        self.max_presences = guild.get("max_presences")
        self.max_members = guild.get("max_members")
        self.max_video_channel_users = guild.get("max_video_channel_users")
        self.premium_tier = guild.get("premium_tier", 0)
        self.premium_subscription_count = guild.get("premium_subscription_count") or 0
        self._system_channel_flags = guild.get("system_channel_flags", 0)
        self.preferred_locale = guild.get("preferred_locale")
        self.discovery_splash = guild.get("discovery_splash")
        self._rules_channel_id = utils._get_as_snowflake(guild, "rules_channel_id")
        self._public_updates_channel_id = utils._get_as_snowflake(guild, "public_updates_channel_id")
        self._large = None if member_count is None else self._member_count >= 250
        self.owner_id = utils._get_as_snowflake(guild, "owner_id")
        self.afk_channel = self.get_channel(utils._get_as_snowflake(guild, "afk_channel_id"))

    @property
    def _channels(self):
        channels = []
        for channel in self._state._scan_and_get(f"channel:{self.id}:*"):
            factory, _ = _channel_factory(channel["type"])
            channels.append(factory(guild=self, state=self._state, data=channel))
        return channels

    @property
    def _members(self):
        members = []
        for member in self._state._scan_and_get(f"member:{self.id}:*"):
            members.append(Member(guild=self, state=self._state, data=member))
        return members

    @property
    def _roles(self):
        roles = []
        for role in self._state._scan_and_get(f"role:{self.id}:*"):
            roles.append(Role(guild=self, state=self._state, data=role))
        return roles

    @property
    def _voice_states(self):
        voices = []
        for voice in self._state._scan_and_get(f"voice:{self.id}:*"):
            if voice["channel_id"]:
                voices.append(VoiceState(channel=self.get_channel(int(voice["channel_id"])), data=voice))
            else:
                voices.append(VoiceState(channel=None, data=voice))
        return voices

    def _voice_state_for(self, user_id):
        result = self._state._get(f"voice:{self.id}:{user_id}")
        if not result:
            return None
        if result["channel_id"]:
            return VoiceState(channel=self.get_channel(int(result["channel_id"])), data=result)
        else:
            return VoiceState(channel=None, data=result)

    @property
    def channels(self):
        return self._channels

    def get_channel(self, channel_id):
        result = self._state._get(f"channel:{self.id}:{channel_id}")
        if not result:
            return None
        factory, _ = _channel_factory(result["type"])
        return factory(guild=self, state=self._state, data=result)

    @property
    def system_channel(self):
        channel_id = self._system_channel_id
        return channel_id and self.get_channel(channel_id)

    @property
    def rules_channel(self):
        channel_id = self._rules_channel_id
        return channel_id and self.get_channel(channel_id)

    @property
    def public_updates_channel(self):
        channel_id = self._public_updates_channel_id
        return channel_id and self.get_channel(channel_id)

    @property
    def members(self):
        return self._members

    def get_member(self, user_id):
        result = self._state._get(f"member:{self.id}:{user_id}")
        if not result:
            return None
        return Member(guild=self, state=self._state, data=result)

    @property
    def roles(self):
        return self._roles

    def get_role(self, role_id):
        result = self._state._get(f"role:{self.id}:{role_id}")
        if not result:
            return None
        return Role(guild=self, state=self._state, data=result)
