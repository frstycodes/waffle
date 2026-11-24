// The voice parts of the profile's activity cards: where the channel is, who's in it, and the way in.
use gpui_kit::FontWeight;

use super::activity::{Fit, card_button, solid_tile};
use super::*;
use crate::discord::{Channel, Guild};

// Who's in the channel, drawn on its tile: (top, left, size) per head for one to four people, at the 60px tile;
// the 100px tile scales them.
const SEATS: [&[(f32, f32, f32)]; 4] = [
    &[(15., 15., 28.)],
    &[(7., 7., 20.), (25., 25., 26.)],
    &[(5., 10., 18.), (15., 31., 22.), (29., 5., 24.)],
    &[(5., 5., 22.), (7., 33., 18.), (33., 7., 18.), (31., 31., 22.)],
];
const SEAT_TILE: f32 = 60.;
// The ring each head wears on the tile.
const SEAT_RING: f32 = 2.;
// How many faces the place row shows before "+N".
const FACES: usize = 3;

// The channel, its icon nudged level with the name, then "in {server}" under it.
pub(super) fn voice_heading(guild: &Guild, channel: &Channel, ink: Ink) -> Div {
    let name = ui::row()
        .gap_0p5()
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ink.text)
        .child(crate::sidebar::channel_glyph(channel).size(ui::ICON_XS).flex_none().text_color(ink.text))
        .child(ui::line(channel.name.clone()));
    ui::col().min_w_0().child(name).child(ui::line(format!("in {}", guild.name)).text_size(ui::TEXT_XS).text_color(ink.text))
}

pub(super) fn hairline(ink: Ink) -> Div {
    ui::rule(ink.strong.opacity(0.12)).w_full().flex_none()
}

impl Waffle {
    fn channel_people(&self, guild: &Guild, channel: &Channel) -> Vec<ui::Entry> {
        guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .map(|v| match &v.user {
                Some(u) => ui::Entry::person(u.avatar.clone(), u.name.clone()),
                None => ui::Entry::person(crate::discord::avatar_url(&v.user_id, None), self.user_name(&v.user_id)),
            })
            .collect()
    }

    pub(super) fn voice_faces(&self, guild: &Guild, channel: &Channel, ink: Ink) -> gpui_kit::Stateful<Div> {
        let people = self.channel_people(guild, channel);
        ui::listed_stack(SharedString::from(format!("profile-voice-{}", channel.id)), "Participants", people, FACES, ui::AVATAR_XS, solid_tile(ink), ink.strong.opacity(0.2), ink.muted)
    }

    // The server › channel breadcrumb under a stream, and who's in it.
    pub(super) fn voice_place_row(&self, guild: &Guild, channel: &Channel, ink: Ink, cx: &App) -> Div {
        ui::row()
            .gap_1()
            .text_size(ui::TEXT_XS)
            .text_color(ink.muted)
            .child(ui::server_tile(guild.icon.as_deref(), &guild.name, ui::AVATAR_XS, cx))
            .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_XS).flex_none())
            .child(ui::row().flex_1().gap_0p5().child(crate::sidebar::channel_glyph(channel).size(ui::ICON_XS).flex_none()).child(ui::line(channel.name.clone())))
            .child(self.voice_faces(guild, channel, ink))
    }

    // The channel's tile with up to four heads placed on it, the second (or fourth, past ten more) a "+N".
    pub(super) fn voice_tile(&self, guild: &Guild, channel: &Channel, fit: Fit, ink: Ink) -> Div {
        let people = self.channel_people(guild, channel);
        let size = fit.art();
        let scale = size / SEAT_TILE;
        let seats = SEATS[people.len().clamp(1, 4) - 1];
        let more = people.len().saturating_sub(3);
        let more_at = if more >= 10 { 3 } else { 1 };
        let heads = seats.iter().enumerate().map(|(at, &(top, left, head))| {
            let spot = div().absolute().top(px(top * scale)).left(px(left * scale)).size(px(head * scale)).child(ui::gap(solid_tile(ink), SEAT_RING));
            match (more > 1 && at == more_at, people.get(at)) {
                // lint-allow(custom-badge): ui::capped_stack puts its "+n" at a row's end; Discord's tile puts it at one of the seats.
                (true, _) => spot.child(ui::row().size_full().rounded_full().justify_center().bg(solid_tile(ink)).text_size(ui::TEXT_XXS).font_weight(FontWeight::SEMIBOLD).text_color(ink.strong).child(if more > 99 { ">99".to_owned() } else { format!("+{more}") })),
                (false, Some(person)) => spot.child(ui::avatar(person.icon.clone(), head * scale)),
                (false, None) => spot,
            }
        });
        div().relative().flex_none().size(px(size)).squircle(fit.inner_radius()).bg(ink.strong.opacity(0.08)).border_1().border_color(ink.strong.opacity(0.08)).children(heads)
    }

    // Join Voice, or Open Voice once you're in it; the full profile says only Join or Open.
    pub(super) fn voice_way_in(&self, guild: &Guild, channel: &Channel, fit: Fit, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let (guild_id, channel) = (guild.id.clone(), channel.clone());
        let joined = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        let label = match (fit, joined) {
            (Fit::Compact, false) => "Join Voice",
            (Fit::Compact, true) => "Open Voice",
            (Fit::Wide, false) => "Join",
            (Fit::Wide, true) => "Open",
        };
        card_button("profile-join-voice", ink, cx)
            .when(fit == Fit::Compact, |b| b.w_full())
            .justify_center()
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.profile_card = None;
                match joined {
                    true => this.show_call(channel.clone(), window, cx),
                    false => this.join_voice_in(Some(guild_id.clone()), channel.clone(), cx),
                }
                cx.notify(); // lint-allow(global-notify): joining or opening a call changes the rail, the channel list and the call stage
            }))
            .into_any_element()
    }
}
