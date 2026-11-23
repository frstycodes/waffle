//! Reading state: names, mentions, unread counts and the server rail.

use crate::*;

impl Waffle {
    pub(crate) fn known_users(&self) -> Vec<User> {
        let mut users: Vec<User> = Vec::new();
        if let Some(list) = self.guild_id.as_ref().and_then(|g| self.member_lists.get(g)) {
            users.extend(list.iter().filter_map(|i| match i {
                ListItem::Member(m) => Some(m.user.clone()),
                ListItem::Group { .. } => None,
            }));
        }
        if let Some(feed) = self.channel.as_ref().and_then(|c| self.feeds.get(&c.id)) {
            users.extend(feed.messages.iter().rev().map(|m| m.author.clone()));
        }
        users.extend(self.dms.iter().flat_map(|d| d.recipients.iter().cloned().chain(d.recipient.clone())));
        users
    }
    // Asked once per server per connection, when it's opened, as Discord does.
    pub(crate) fn request_channel_info(&mut self) {
        let (Some(guild_id), Some(client)) = (self.guild_id.clone(), self.client.as_ref()) else { return };
        if self.channel_info_requested.insert(guild_id.clone()) {
            client.request_channel_info(&guild_id);
        }
    }
    // How long a voice channel's current session has run, from Discord's start time.
    pub(crate) fn voice_elapsed(&self, channel_id: &str) -> Option<u64> {
        let start = *self.voice_start.get(channel_id)?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(now.saturating_sub(start))
    }
    pub(crate) fn user_avatar(&self, user_id: &str) -> String {
        self.known_users().into_iter().find(|u| u.id == user_id).map(|u| u.avatar).unwrap_or_else(|| discord::avatar_url(user_id, None))
    }
    pub(crate) fn user_name(&self, user_id: &str) -> String {
        self.known_users().into_iter().find(|u| u.id == user_id).map(|u| u.name).unwrap_or_else(|| "Someone".into())
    }
    // A message changed in place (edit, reaction, embed, pin). gpui's list keeps each row's
    // measured height until told otherwise, so the open channel's row is re-measured.
    pub(crate) fn message_mut(&mut self, channel_id: &str, id: &str) -> Option<&mut Message> {
        let open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        let feed = self.feeds.get_mut(channel_id)?;
        let ix = feed.messages.iter().position(|m| m.id == id)?;
        if open {
            self.list.remeasure_items(ix..ix + 1);
        }
        Arc::make_mut(&mut feed.messages).get_mut(ix)
    }
    pub(crate) fn remove_message(&mut self, channel_id: &str, id: &str) {
        let is_open = self.channel.as_ref().is_some_and(|c| c.id == channel_id);
        let Some(feed) = self.feeds.get_mut(channel_id) else { return };
        let Some(ix) = feed.messages.iter().position(|m| m.id == id) else { return };
        Arc::make_mut(&mut feed.messages).remove(ix);
        if is_open {
            self.list.splice(ix..ix + 1, 0);
        }
    }
    pub(crate) fn guild(&self) -> Option<&Guild> {
        let id = self.guild_id.as_ref()?;
        self.guilds.iter().find(|g| &g.id == id)
    }
    pub(crate) fn mentions_me(&self, message: &Message) -> bool {
        let Some(me) = &self.me else { return false };
        message.mention_everyone || message.mentions.iter().any(|u| u.id == me.id) || self.mentions_role(message)
    }
    // A thread's parent channel, for the header breadcrumb and the sidebar.
    pub(crate) fn thread_parent(&self, channel: &Channel) -> Option<Channel> {
        if channel.kind != ChannelKind::Thread {
            return None;
        }
        let parent = channel.parent_id.as_ref()?;
        self.guilds.iter().flat_map(|g| g.channels.iter()).find(|c| &c.id == parent).cloned()
    }
    // Whether the message pings one of our roles in its server.
    pub(crate) fn mentions_role(&self, message: &Message) -> bool {
        let Some(me) = &self.me else { return false };
        let mine = self.guild_id_of(message).and_then(|g| self.member_roles.get(&g)).and_then(|r| r.get(&me.id));
        mine.is_some_and(|roles| message.mention_roles.iter().any(|r| roles.contains(r)))
    }
    /// The colour of the user's highest coloured role, clamped so the name stays readable in this theme.
    // The name the open server shows for someone: their nickname once the member is known.
    pub(crate) fn member_name(&self, user: &discord::User) -> String {
        let member = self.guild_id.as_ref().and_then(|g| self.members.get(g)).and_then(|m| m.get(&user.id));
        member.map_or_else(|| user.name.clone(), |m| m.name.clone())
    }
    pub(crate) fn role_color(&self, guild_id: Option<&String>, user_id: &str, hint: Option<&Vec<String>>, dark: bool) -> Option<Hsla> {
        let guild = self.guilds.iter().find(|g| Some(&g.id) == guild_id)?;
        let roles = hint.or_else(|| self.member_roles.get(&guild.id)?.get(user_id))?;
        guild.roles.iter().filter(|r| r.color != 0 && roles.contains(&r.id)).max_by_key(|r| r.position).map(|r| palette::role_ink(r.color, dark))
    }
    pub(crate) fn unread(&self, channel_id: &str) -> bool {
        let (Some(last), Some(read)) = (self.last_message.get(channel_id), self.read_states.get(channel_id)) else { return false };
        *last > read.last_read
    }
    /// Our user id, or "" before READY.
    pub(crate) fn my_id(&self) -> &str {
        self.me.as_ref().map_or("", |m| m.id.as_str())
    }
    pub(crate) fn mention_count(&self, channel_id: &str) -> u32 {
        self.read_states.get(channel_id).map_or(0, |r| r.mentions)
    }
    // A muted server, or muted channel, keeps its mention count but drops its unread pill, as in Discord.
    pub(crate) fn guild_badge(&self, guild: &Guild) -> (bool, u32) {
        let settings = self.guild_settings.get(&guild.id);
        let server_muted = settings.is_some_and(|s| s.is_muted());
        let me = self.my_id();
        let text = guild.channels.iter().filter(|c| c.kind == ChannelKind::Text && guild.can_view(me, c));
        text.fold((false, 0), |(unread, mentions), c| {
            let muted = server_muted || settings.is_some_and(|s| s.channel_muted(&c.id) || c.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
            (unread || (self.unread(&c.id) && !muted), mentions + self.mention_count(&c.id))
        })
    }
    pub(crate) fn typers(&self) -> Vec<Typer> {
        let Some(channel) = &self.channel else { return Vec::new() };
        let now = Instant::now();
        self.typing.get(&channel.id).map(|t| t.iter().filter(|t| t.until > now).cloned().collect()).unwrap_or_default()
    }
    /// The rail in Discord's order: folders and loose servers as the user arranged them, then any others.
    pub(crate) fn rail_entries(&self) -> Vec<RailEntry> {
        let mut placed: HashSet<&str> = HashSet::new();
        let mut entries = Vec::new();
        for folder in &self.folders {
            let guilds: Vec<usize> = folder.guild_ids.iter().filter_map(|id| self.guilds.iter().position(|g| &g.id == id)).collect();
            placed.extend(folder.guild_ids.iter().map(String::as_str));
            match (folder.id.is_empty() || folder.id == "0", guilds.len()) {
                (_, 0) => {}
                (true, _) => entries.extend(guilds.into_iter().map(RailEntry::Guild)),
                (false, _) => entries.push(RailEntry::Folder(folder.clone(), guilds)),
            }
        }
        entries.extend(self.guilds.iter().enumerate().filter(|(_, g)| !placed.contains(g.id.as_str())).map(|(i, _)| RailEntry::Guild(i)));
        entries
    }
    pub(crate) fn set_hovered(&mut self, key: &str, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.hovered = Some(key.to_owned());
        } else if self.hovered.as_deref() == Some(key) {
            self.hovered = None;
        }
        self.redraw(if key == crate::home::MASCOT { part::Column::Main } else { part::Column::Nav }, cx);
    }
}
