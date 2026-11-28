// The top of a channel's history: a DM's header, or Discord's channel intro for a server text channel (a 68px
// disc with the channel's glyph, "Welcome to #name!", "This is the start of the #name channel." and its
// topic, then Edit Channel for whoever can manage it). Like the DM header, it is the feed's first item once
// the beginning is loaded, and rests at the foot of an empty channel.
use gpui_kit::{AnyElement, App, FontWeight, SharedString, WeakEntity, component::Icon, div, prelude::*, px};

use super::{DmStart, dm_start_header};
use crate::{
    Waffle,
    chat::emphasised,
    discord::{Channel, ChannelKind, perm},
    icons::Glyph,
    palette,
    ui::{self},
};

// Discord's intro disc, and the glyph inside it.
const DISC: f32 = 68.;
const DISC_GLYPH: f32 = 40.;

#[derive(Clone)]
pub(crate) enum HistoryStart {
    Dm(DmStart),
    Channel(ChannelStart),
}

#[derive(Clone)]
pub(crate) struct ChannelStart {
    channel: Channel,
    guild_id: String,
    can_edit: bool,
}

impl Waffle {
    pub(crate) fn history_start(&self, channel: &Channel, has_more: bool) -> Option<HistoryStart> {
        if let Some(dm) = self.dm_start(channel, has_more) {
            return Some(HistoryStart::Dm(dm));
        }
        if channel.kind != ChannelKind::Text || has_more {
            return None;
        }
        let guild_id = self.guild_id.clone()?;
        Some(HistoryStart::Channel(ChannelStart { channel: channel.clone(), guild_id, can_edit: self.can_in(&channel.id, perm::MANAGE_CHANNELS) }))
    }
}

pub(crate) fn history_start_header(start: &HistoryStart, this: WeakEntity<Waffle>, cx: &App) -> AnyElement {
    match start {
        HistoryStart::Dm(dm) => dm_start_header(dm, this, cx),
        HistoryStart::Channel(channel) => channel_start_header(channel, this, cx),
    }
}

// Discord's copy: "This is the start of the #name channel." or "…the #name **private** channel.", then the topic.
pub(crate) fn intro_line(name: &str, private: bool, topic: Option<&str>) -> Vec<(String, bool)> {
    let topic = topic.filter(|t| !t.trim().is_empty()).map(|t| format!(" {t}")).unwrap_or_default();
    match private {
        true => vec![(format!("This is the start of the #{name} "), false), ("private".to_owned(), true), (format!(" channel.{topic}"), false)],
        false => vec![(format!("This is the start of the #{name} channel.{topic}"), false)],
    }
}

fn channel_start_header(start: &ChannelStart, this: WeakEntity<Waffle>, cx: &App) -> AnyElement {
    let channel = &start.channel;
    let glyph = if channel.private { Glyph::HashtagLock } else { Glyph::Hashtag };
    let disc = ui::row().size(px(DISC)).flex_none().rounded_full().justify_center().bg(palette::hover(cx)).text_color(palette::strong(cx)).child(Icon::new(glyph).size(px(DISC_GLYPH)));
    let edit = start.can_edit.then(|| {
        let (channel, guild_id) = (channel.clone(), start.guild_id.clone());
        ui::Button::new(SharedString::from(format!("edit-channel-{}", channel.id))).variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child(Icon::new(Glyph::Pen).size(ui::ICON_MD)).child("Edit Channel").on_click(move |_, window, cx| {
            cx.stop_propagation();
            this.update(cx, |this, cx| this.open_channel_settings(channel.clone(), guild_id.clone(), window, cx)).ok();
        })
    });
    ui::col()
        .w_full()
        .px(px(crate::chat::GUTTER))
        .pt_4()
        .pb_4()
        .items_start()
        .gap_2()
        .child(disc)
        .child(ui::line(format!("Welcome to #{}!", channel.name)).max_w_full().text_size(ui::TEXT_XL).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)))
        .child(div().text_size(ui::TEXT_MD).text_color(palette::text(cx)).child(emphasised(&intro_line(&channel.name, channel.private, channel.topic.as_deref()), palette::strong(cx))))
        .children(edit.map(|e| div().pt_2().child(e)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::intro_line;

    #[test]
    fn intro_reads_like_discords() {
        let plain = |l: Vec<(String, bool)>| l.into_iter().map(|(t, _)| t).collect::<String>();
        assert_eq!(plain(intro_line("general", false, None)), "This is the start of the #general channel.");
        assert_eq!(plain(intro_line("mods", true, Some("Be nice"))), "This is the start of the #mods private channel. Be nice");
        assert!(intro_line("mods", true, None)[1].1);
    }
}
