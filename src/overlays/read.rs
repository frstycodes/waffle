// Marking channels, servers and mentions read or unread, and telling Discord.
use super::*;

// Discord's Mark Unread: it acks the newest loaded message before the one marked (or the millisecond
// before it) and counts the loaded messages after that ack which `counts` as mentions.
pub(super) fn manual_ack(loaded: &[Message], marked: u64, counts: impl Fn(&Message) -> bool) -> (u64, u32) {
    let ack = loaded.iter().map(|m| crate::order(&m.id)).filter(|id| *id < marked).max().unwrap_or_else(|| previous_millisecond(marked));
    let mentions = loaded.iter().filter(|m| crate::order(&m.id) > ack && counts(m)).count();
    (ack, mentions as u32)
}

// The lowest snowflake one millisecond before `id`: the timestamp sits above bit 22.
pub(super) fn previous_millisecond(id: u64) -> u64 {
    ((id >> 22).saturating_sub(1)) << 22
}

impl Waffle {
    pub(crate) fn mark_channel_read(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        let Some(&last) = self.last_message.get(channel_id) else { return };
        let read = self.read_states.entry(channel_id.to_owned()).or_default();
        read.last_read = last;
        read.mentions = 0;
        if let Some(client) = self.client.clone() {
            let id = channel_id.to_owned();
            crate::discord::runtime().spawn(async move { client.ack(&id, &last.to_string()).await });
        }
        cx.notify(); // lint-allow(global-notify): the unread marks draw in the rail, the channel list and the chat
    }

    pub(crate) fn mark_guild_read(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        let channels: Vec<String> = guild.channels.iter().filter(|c| self.unread(&c.id) || self.mention_count(&c.id) > 0).map(|c| c.id.clone()).collect();
        self.mark_channels_read(&channels, cx);
    }

    /// Acks every channel in `channels` at its newest message in one request.
    pub(crate) fn mark_channels_read(&mut self, channels: &[String], cx: &mut Context<Self>) {
        let unread: Vec<(String, u64)> = channels.iter().filter_map(|c| Some((c.clone(), *self.last_message.get(c)?))).collect();
        for (channel, last) in &unread {
            let read = self.read_states.entry(channel.clone()).or_default();
            read.last_read = *last;
            read.mentions = 0;
        }
        if let Some(client) = self.client.clone().filter(|_| !unread.is_empty()) {
            crate::discord::runtime().spawn(async move { client.ack_bulk(unread).await });
        }
        cx.notify(); // lint-allow(global-notify): the unread marks draw in the rail, the channel list and the chat
    }

    /// Moves the read line to just before this message, as Discord's "Mark Unread", and tells Discord, so the
    /// channel stays unread on your other devices.
    pub(crate) fn mark_unread(&mut self, message: &Message, cx: &mut Context<Self>) {
        let Ok(id) = message.id.parse::<u64>() else { return };
        let loaded = self.feeds.get(&message.channel_id).map(|f| f.messages.clone()).unwrap_or_default();
        let (ack_id, mentions) = manual_ack(&loaded, id, |m| self.counts_as_mention(m));
        let read = self.read_states.entry(message.channel_id.clone()).or_default();
        read.last_read = id - 1;
        read.mentions = mentions;
        self.unread_marker = Some(id - 1);
        self.held_unread = Some(message.channel_id.clone());
        if let Some(client) = self.client.clone() {
            let channel_id = message.channel_id.clone();
            crate::discord::runtime().spawn(async move { client.ack_manual(&channel_id, ack_id, mentions).await });
        }
        cx.notify(); // lint-allow(global-notify): the unread marks draw in the rail, the channel list and the chat
    }

    /// Clears a mention from the Inbox for good, without reading its channel; it comes back if Discord refuses.
    pub(crate) fn mark_mention_read(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(Some(Ok(list))) = &mut self.inbox else { return };
        let Some(at) = list.iter().position(|m| m.id == message_id) else { return };
        let removed = list.remove(at);
        cx.notify(); // lint-allow(global-notify): the mention list draws in the Inbox popover and on the Inbox page
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let Err(error) = client.delete_mention(&message_id).await else { return };
            this.update(cx, |this, cx| {
                if let Some(Some(Ok(list))) = &mut this.inbox {
                    list.insert(at.min(list.len()), removed);
                }
                this.show_toast(format!("Couldn't mark that mention read: {error}"), cx);
            })
            .ok();
        })
        // lint-allow(detached-task): ends when Discord answers the delete, and holds only a WeakEntity.
        .detach();
    }
}
