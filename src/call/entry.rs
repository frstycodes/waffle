// Discord's ways into a voice channel's stage: the entry tile at the grid's end (Start An Activity with company,
// Invite to Voice alone) and the join screen of a voice channel you're looking at but not in.
use super::*;
use crate::ui;

// Under this width the entry tile is just its one button.
const ENTRY_FULL: f32 = 250.;
// The join screen shows at most this many faces.
const JOIN_FACES: usize = 5;
// A face on the join screen: the avatar inside its speaking ring's room.
const JOIN_FACE: f32 = 48.;

impl Waffle {
    // Which entry tile the grid ends with: `Some(true)` alone (Invite to Voice), `Some(false)` with company and no
    // game running (Start An Activity, until dismissed). Server voice only.
    pub(super) fn entry_kind(&self) -> Option<bool> {
        let call = self.call.as_ref()?;
        call.guild_id.as_ref()?;
        let alone = self.participants().len() <= 1;
        let playing = !self.activity_tiles().is_empty();
        match (alone, playing || self.stage_ui.entry_hidden) {
            (true, _) => Some(true),
            (false, false) => Some(false),
            (false, true) => None,
        }
    }

    pub(super) fn entry_tile(&self, alone: bool, w: f32, cx: &mut Context<Self>) -> AnyElement {
        let ground = gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(palette::accent().opacity(0.22), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.8));
        let card = ui::col().id("call-entry").relative().w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(RADIUS_LG).overflow_hidden().bg(stage::ended()).items_center().justify_center().gap_2p5().p_3().text_center();
        let card = card.child(div().absolute().inset_0().bg(ground));
        let invite = self.call.as_ref().and_then(|c| Some((c.guild_id.clone()?, c.channel.id.clone())));
        let invite_button = move |cx: &mut Context<Self>| {
            let invite = invite.clone();
            ui::button("entry-invite", ui::Tone::Overlay, cx).small().child("Invite to Voice").on_click(cx.listener(move |this, _, window, cx| {
                if let Some((guild, channel)) = invite.clone() {
                    this.show_invite(guild, channel, window, cx);
                }
            }))
        };
        let choose = |tone: ui::Tone, cx: &mut Context<Self>| {
            ui::button("entry-activity", tone, cx).small().child("Choose Activity").on_click(cx.listener(|this, e: &gpui_kit::ClickEvent, window, cx| this.toggle_launcher(e.position(), window, cx)))
        };
        let pitch = div().text_size(ui::TEXT_SM).text_color(stage::muted()).child("Looking for something to do? Start a game or video playlist instantly.");
        if w < ENTRY_FULL {
            return match alone {
                true => card.child(invite_button(cx)),
                false => card.child(ui::button("entry-activity", ui::Tone::Overlay, cx).small().child("Start An Activity").on_click(cx.listener(|this, e: &gpui_kit::ClickEvent, window, cx| this.toggle_launcher(e.position(), window, cx)))),
            }
            .into_any_element();
        }
        if alone {
            return card
                .child(Icon::new(Glyph::ActivitiesFill).size_8().text_color(palette::logo()))
                .child(pitch)
                .child(ui::row().gap_2().child(invite_button(cx)).child(choose(ui::Tone::Stage, cx)))
                .into_any_element();
        }
        // GAP: "Don't show again" lasts the session; Discord keeps it in its settings.
        let dismiss = |this: &mut Self, cx: &mut Context<Self>| {
            this.stage_ui.entry_hidden = true;
            this.redraw(Column::Main, cx);
        };
        card.child(div().absolute().top_2().right_2().child(stage_icon_button("entry-close", Glyph::X, "Close", false, cx).on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx)))))
            .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()).child("Start An Activity"))
            .child(pitch)
            .child(choose(ui::Tone::Overlay, cx))
            .child(ui::button("entry-never", ui::Tone::Ghost, cx).small().on_stage().child("Don't show again").on_click(cx.listener(move |this, _, _, cx| dismiss(this, cx))))
            .into_any_element()
    }

    // A voice channel left while looking at it: who's in it (up to five faces), and Join Voice, or Channel Locked
    // when we can't connect.
    pub(crate) fn join_screen(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.stage_ui.join.clone()?;
        let people: Vec<VoiceState> = self.guilds.iter().flat_map(|g| g.voice_states.iter()).filter(|v| v.channel_id.as_deref() == Some(channel.id.as_str())).cloned().collect();
        let names: Vec<String> = people.iter().map(|p| p.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&p.user_id))).collect();
        let faces = people.iter().take(JOIN_FACES).map(|p| {
            let avatar = p.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| self.user_avatar(&p.user_id));
            ui::row().size(px(JOIN_FACE)).flex_none().justify_center().child(ui::avatar(avatar, ui::AVATAR_XL))
        });
        let locked = !self.can_in(&channel.id, crate::discord::perm::CONNECT);
        let join = channel.clone();
        let button = ui::button("join-voice", ui::Tone::Overlay, cx).child(if locked { "Channel Locked" } else { "Join Voice" }).disabled(locked).on_click(cx.listener(move |this, _, _, cx| {
            this.stage_ui.join = None;
            this.join_voice(join.clone(), cx);
        }));
        let glow = gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(palette::accent().opacity(0.18), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.6));
        let head = ui::row().h(px(FOCUS_HEAD)).gap_2().text_color(stage::strong()).child(Icon::new(Glyph::VolumeHigh).size(ui::ICON_XL).text_color(stage::muted())).child(ui::line(channel.name.clone()).text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD));
        Some(
            ui::col()
                .id("call-join")
                .relative()
                .size_full()
                .bg(stage::bg())
                .text_color(stage::text())
                .child(div().absolute().inset_0().bg(glow))
                // Not in the call: its chat can't be open beside this screen.
                .child(self.stage_head(head, true))
                .children(self.voice_notice_bar(cx))
                .child(
                    ui::col()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .p_6()
                        .text_center()
                        .child(ui::row().flex_wrap().justify_center().gap_2().children(faces))
                        .child(ui::line(channel.name.clone()).max_w_full().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(stage::strong()))
                        .child(div().text_size(ui::TEXT_SM).child(in_voice_line(&names)))
                        .child(button),
                )
                .into_any_element(),
        )
    }
}

// Discord's line under the join screen's title.
fn in_voice_line(names: &[String]) -> String {
    match names {
        [] => "No one is currently in voice".into(),
        [a] => format!("{a} is currently in voice"),
        [a, b] => format!("{a} and {b} are currently in voice"),
        [a, b, rest @ ..] => format!("{a}, {b}, and {} other{} are currently in voice", rest.len(), if rest.len() == 1 { "" } else { "s" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_join_line_names_who_is_in_voice() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(in_voice_line(&[]), "No one is currently in voice");
        assert_eq!(in_voice_line(&names(&["Mira"])), "Mira is currently in voice");
        assert_eq!(in_voice_line(&names(&["Mira", "Bao"])), "Mira and Bao are currently in voice");
        assert_eq!(in_voice_line(&names(&["Mira", "Bao", "Yuzu"])), "Mira, Bao, and 1 other are currently in voice");
        assert_eq!(in_voice_line(&names(&["Mira", "Bao", "Yuzu", "Kenji"])), "Mira, Bao, and 2 others are currently in voice");
    }
}
