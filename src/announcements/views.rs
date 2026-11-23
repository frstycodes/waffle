// What announcements draw by themselves: the dialogs' parts, the Publish button and bump, a crosspost's tag
// and source trigger.
use gpui_kit::{AnyElement, App, Context, Div, FontWeight, SharedString, Stateful, WeakEntity, component::Icon, div, prelude::*, px};

use super::{PUBLISH_LIMIT, Newsroom, Source, is_official, published};
use crate::{
    Waffle,
    discord::Message,
    icons::Glyph,
    palette,
    ui,
};

// The quota's pip gap, and the widest the publish bump grows.
const PIP_GAP: f32 = 3.;
const STRIP_WIDTH: f32 = 520.;

pub(super) fn dialog_title(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(text.into())
}

pub(super) fn footer() -> Div {
    ui::row().items_stretch().justify_end().gap_2()
}

// Better than Discord: how many of the hour's ten Waffle has used here.
pub(super) fn quota(used: usize, cx: &App) -> Div {
    let (on, off) = (palette::muted(cx), palette::line_strong(cx));
    let pips = (0..PUBLISH_LIMIT).map(|i| div().w_2p5().h_1().rounded_xs().bg(if i < used { on } else { off }));
    ui::row()
        .gap_2()
        .text_size(ui::TEXT_XS)
        .text_color(palette::muted(cx))
        .child(ui::row().items_stretch().gap(px(PIP_GAP)).children(pips))
        .child(format!("{} of {PUBLISH_LIMIT} published this hour", used.min(PUBLISH_LIMIT)))
}

pub(super) fn alert_dialog(title: &'static str, text: String, cx: &mut Context<Waffle>) -> Div {
    ui::dialog_body()
        .child(dialog_title(title, cx))
        .child(div().text_color(palette::muted(cx)).child(text))
        .child(footer().child(ui::button("publish-alert-okay", ui::Tone::Primary, cx).child("Okay").on_click(cx.listener(|this, _, _, cx| this.close_announcement_dialog(cx)))))
}

pub(super) fn followed_dialog(adjective: &str, cx: &mut Context<Waffle>) -> Div {
    ui::dialog_body()
        .items_center()
        .text_center()
        .child(crate::mascot::mascot_at("followed-mascot", crate::mascot::Mood::Happy, 0.8, cx))
        .child(dialog_title(format!("What a nice addition to your server. Lookin' real {adjective} in here."), cx))
        .child(div().text_color(palette::muted(cx)).child("The most important updates from this channel will now show up directly in your server. Enjoy!"))
        .child(ui::button("followed-okay", ui::Tone::Primary, cx).child("Got it!").on_click(cx.listener(|this, _, _, cx| this.close_announcement_dialog(cx))))
}

// The hover toolbar's Publish; once published it reads "Published" and is disabled.
pub(crate) fn publish_button(message: &Message, mine: bool, newsroom: &Newsroom, this: WeakEntity<Waffle>, cx: &App) -> Option<AnyElement> {
    if !newsroom.can_publish(message, mine) {
        return None;
    }
    let done = published(message);
    let msg = message.clone();
    Some(
        ui::icon_button(SharedString::from(format!("publish-{}", message.id)), Glyph::Megaphone, if done { "Published" } else { "Publish" }, cx)
            .disabled(done)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                this.update(cx, |this, cx| this.open_publish(msg.clone(), cx)).ok();
            })
            .into_any_element(),
    )
}

// "Share with your followers!" under a message just sent: Publish, dismiss, and Don't show again for the channel.
pub(crate) fn publish_bump(message: &Message, mine: bool, newsroom: &Newsroom, this: WeakEntity<Waffle>, cx: &App) -> Option<AnyElement> {
    if !newsroom.shows_bump(message, mine) {
        return None;
    }
    let (publish, dismiss, hide) = (this.clone(), this.clone(), this);
    let (msg, id, channel_id) = (message.clone(), message.id.clone(), message.channel_id.clone());
    let strip = ui::strip(ui::StripFill::Selected, cx)
        .max_w(px(STRIP_WIDTH))
        .min_h_0()
        .py_2()
        .child(Icon::new(Glyph::Megaphone).size(ui::ICON_MD).text_color(palette::muted(cx)))
        .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Share with your followers!"))
        .child(ui::button(SharedString::from(format!("bump-publish-{}", message.id)), ui::Tone::Link, cx).child("Publish").on_click(move |_, _, cx| {
            cx.stop_propagation();
            publish.update(cx, |this, cx| this.open_publish(msg.clone(), cx)).ok();
        }))
        .child(div().flex_1())
        .child(ui::icon_button(SharedString::from(format!("bump-dismiss-{}", message.id)), Glyph::X, "Dismiss", cx).small().on_click(move |_, _, cx| {
            cx.stop_propagation();
            dismiss.update(cx, |this, cx| this.dismiss_publish_bump(&id, cx)).ok();
        }));
    let faint = palette::faint(cx);
    let never = div()
        .id(SharedString::from(format!("bump-hide-{}", message.id)))
        .pt_1()
        .w_auto()
        .text_size(ui::TEXT_XS)
        .text_color(faint)
        .cursor_pointer()
        .hover(|s| s.underline())
        .child("Don't show again")
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            hide.update(cx, |this, cx| this.hide_publish_bumps(channel_id.clone(), cx)).ok();
        });
    Some(ui::col().items_start().pt_1p5().child(strip).child(never).into_any_element())
}

// A crosspost's author tag, where a bot's reads APP; Discord's own community updates read OFFICIAL.
pub(crate) fn server_tag(message: &Message, cx: &App) -> Div {
    ui::tag(if is_official(message) { "Official" } else { "Server" }, cx)
}

// A crosspost's avatar and name open its source server's card, not the webhook's profile.
pub(crate) fn source_trigger(id: String, message: &Message, this: WeakEntity<Waffle>) -> Option<Stateful<Div>> {
    let source = Source::of(message)?;
    Some(div().id(SharedString::from(id)).cursor_pointer().on_click(move |event, _, cx| {
        cx.stop_propagation();
        let at = event.position();
        this.update(cx, |this, cx| this.open_source_card(source.clone(), at, cx)).ok();
    }))
}

pub(crate) fn source_deleted(cx: &App) -> AnyElement {
    div().italic().text_color(palette::muted(cx)).child("[Original Message Deleted]").into_any_element()
}
