// An activity's post in a chat, as Discord's Game Invitation card: a wash of the game's own colours, its icon and
// name, where the game stands now, and one white button (Join, Open, or Play once it has ended).
use std::cell::RefCell;

use gpui_kit::Hsla;

use super::*;

// The card's width, its icon, and the corner its wash runs from (Discord's runs top left to bottom right).
const CARD_WIDTH: f32 = 432.;
const ICON: f32 = 64.;
const WASH_ANGLE: f32 = 135.;

// What chat cards need to stay live: each running instance (by launch and instance id), and the wash for each
// app's card once its icon's colours are read.
pub(crate) struct ChatCards {
    live: HashMap<String, LiveCard>,
    washes: HashMap<String, (Hsla, Hsla)>,
}

struct LiveCard {
    players: Vec<User>, // lint-allow(unbounded-growth): a card's players, built from one activity's participants on each render and never kept
    me_in: bool,
    max: Option<u32>,
}

// The two colours of each app's wash, read from its icon once (None while reading, or when it can't be).
#[derive(Default)]
pub(crate) struct Washes(RefCell<HashMap<String, Option<(Hsla, Hsla)>>>);

impl Waffle {
    // The cards' live state for `messages`, reading the colours of any app among them not read yet.
    pub(crate) fn chat_cards(&self, messages: &[discord::Message], cx: &mut Context<Self>) -> ChatCards {
        let me = self.me.as_ref().map(|m| m.id.clone());
        let playing = self.activities.frame.as_ref().map(|f| f.read(cx).app_id.clone());
        let mut live = HashMap::new();
        for instance in &self.activities.instances {
            let card = || LiveCard {
                players: instance.participants.iter().map(|p| self.player(&p.user_id)).collect(),
                me_in: playing.as_ref() == Some(&instance.application_id) && instance.participants.iter().any(|p| Some(&p.user_id) == me.as_ref()),
                max: self.activities.apps.get(&instance.application_id).and_then(|a| a.max_players),
            };
            live.insert(instance.instance_id.clone(), card());
            live.insert(instance.launch_id.clone(), card());
        }
        for activity in messages.iter().filter_map(|m| m.activity.as_ref()) {
            self.read_wash(activity, cx);
        }
        let washes = self.activities.washes.0.borrow().iter().filter_map(|(id, wash)| Some((id.clone(), (*wash)?))).collect();
        ChatCards { live, washes }
    }

    fn read_wash(&self, activity: &discord::MessageActivity, cx: &mut Context<Self>) {
        let Some(icon) = activity.app_icon.clone() else { return };
        let mut washes = self.activities.washes.0.borrow_mut();
        if washes.contains_key(&activity.app_id) {
            return;
        }
        washes.insert(activity.app_id.clone(), None);
        drop(washes);
        let app_id = activity.app_id.clone();
        cx.spawn(async move |this, cx| {
            let wash = crate::discord::runtime().spawn(async move { icon_wash(&icon).await }).await.ok().flatten();
            _ = this.update(cx, |this, cx| {
                this.activities.washes.0.borrow_mut().insert(app_id, wash);
                this.redraw(Column::Main, cx);
            });
        })
        .detach(); // lint-allow(detached-task): a one-shot icon load that ends when it returns, holding a WeakEntity
    }

    // Joining from a chat card: into its voice channel's call and the game, or the game over that chat.
    fn join_from_chat(&mut self, channel_id: &str, guild_id: Option<&str>, app_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let channel = match guild_id {
            Some(id) => self.guilds.iter().find(|g| g.id == id).and_then(|g| g.channel(channel_id).cloned()),
            None => self.dms.iter().find(|d| d.id == channel_id).cloned(),
        };
        let Some(channel) = channel else { return };
        if self.in_activity(&app_id, cx) {
            return match self.game_place() {
                Some(place) if place.voice => self.show_call(place.channel, window, cx),
                Some(place) => self.open_channel(place.channel, window, cx),
                None => {}
            };
        }
        match channel.kind {
            discord::ChannelKind::Voice => self.join_activity_in(channel, app_id, window, cx),
            _ => self.launch_activity_at(app_id, Place { channel, guild_id: guild_id.map(str::to_owned), voice: false }, cx),
        }
    }
}

pub(crate) fn activity_message_card(message: &discord::Message, activity: &discord::MessageActivity, cards: &ChatCards, this: gpui_kit::WeakEntity<Waffle>, cx: &gpui_kit::App) -> AnyElement {
    let name = activity.app_name.clone().unwrap_or_else(|| "Activity".into());
    let live = cards.live.get(&activity.instance_id);
    let full = live.is_some_and(|l| !l.me_in && l.max.is_some_and(|max| l.players.len() as u32 >= max));
    let (line, label) = match live {
        None => ("Game ended. Start a new one?".to_owned(), "Play"),
        Some(l) if l.me_in => ("You're playing".to_owned(), "Open"),
        Some(l) if full => (format!("Full: {} of {} playing", l.players.len(), l.max.unwrap_or_default()), "Full"),
        Some(l) => (who_is_playing(&l.players), "Join"),
    };
    // Without the icon's colours yet, the stage's own dark stands in; the card is dark in both themes.
    let (from, to) = cards.washes.get(&activity.app_id).copied().unwrap_or((palette::stage::dock(), palette::stage::bg()));
    let (channel_id, guild_id, app_id) = (message.channel_id.clone(), message.guild_id.clone(), activity.app_id.clone());
    let button = ui::button(SharedString::from(format!("activity-card-{}", message.id)), Tone::Overlay, cx).w_full().disabled(full).child(label).on_click(move |_, window, cx| {
        cx.stop_propagation();
        _ = this.update(cx, |this, cx| this.join_from_chat(&channel_id, guild_id.as_deref(), app_id.clone(), window, cx));
    });
    ui::col()
        .mt_1() // lint-allow(raw-margin): the card hangs a step under a message's text, which the message's own column doesn't gap
        .w_full()
        .max_w(px(CARD_WIDTH))
        .p_4()
        .gap_3()
        .squircle(RADIUS_MD)
        .bg(gpui_kit::linear_gradient(WASH_ANGLE, gpui_kit::linear_color_stop(from, 0.), gpui_kit::linear_color_stop(to, 1.)))
        .text_color(palette::stage::strong())
        .child(div().text_size(TEXT_SM).font_weight(gpui_kit::FontWeight::SEMIBOLD).text_color(palette::stage::text()).child("Game Invitation"))
        .child(
            ui::row()
                .gap_4()
                .child(ui::server_tile(activity.app_icon.as_deref(), &name, ICON, cx))
                .child(ui::col().min_h_0().flex_1().gap_0p5().child(ui::line(name).text_size(TEXT_MD).font_weight(gpui_kit::FontWeight::BOLD)).child(div().text_size(TEXT_SM).text_color(palette::stage::text()).child(line))),
        )
        .child(button)
        .into_any_element()
}

// An icon's wash: its two leading hues, as Discord's card runs from one to the other (Putt Party's navy field to
// its yellow stripe). Hues are bucketed and each pixel weighted by its saturation, so greys don't vote and a
// colour isn't averaged into mud; the second is the fullest bucket at least a third of the wheel from the first,
// or the first again, darker. Both are darkened so white reads on them.
async fn icon_wash(url: &str) -> Option<(Hsla, Hsla)> {
    let bytes = reqwest::get(url).await.ok()?.bytes().await.ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let pixels: Vec<Hsla> = rgba.pixels().filter(|p| p.0[3] > 128).map(|p| Hsla::from(gpui_kit::Rgba { r: p.0[0] as f32 / 255., g: p.0[1] as f32 / 255., b: p.0[2] as f32 / 255., a: 1. })).collect();
    let (first, second) = leading_hues(&pixels)?;
    let dark = |mut c: Hsla, (lo, hi): (f32, f32)| {
        c.l = c.l.clamp(lo, hi);
        c.s = c.s.min(WASH_SATURATION);
        c
    };
    let second = second.map(|c| dark(c, WASH_LIGHTNESS)).unwrap_or_else(|| dark(first, (WASH_LIGHTNESS.0 * 0.6, WASH_LIGHTNESS.0)));
    Some((dark(first, WASH_LIGHTNESS), second))
}

fn leading_hues(pixels: &[Hsla]) -> Option<(Hsla, Option<Hsla>)> {
    let mut buckets = [(0f32, 0f32, 0f32, 0f32, 0f32); HUE_BUCKETS];
    for p in pixels.iter().filter(|p| p.s > VIVID) {
        let b = &mut buckets[((p.h * HUE_BUCKETS as f32) as usize).min(HUE_BUCKETS - 1)];
        let (x, y) = (p.h * std::f32::consts::TAU).sin_cos();
        *b = (b.0 + p.s, b.1 + x * p.s, b.2 + y * p.s, b.3 + p.s * p.s, b.4 + p.l * p.s);
    }
    let colour = |(weight, x, y, sat, light): (f32, f32, f32, f32, f32)| Hsla { h: x.atan2(y).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU, s: sat / weight, l: light / weight, a: 1. };
    let top = (0..HUE_BUCKETS).filter(|i| buckets[*i].0 > 0.).max_by(|a, b| buckets[*a].0.total_cmp(&buckets[*b].0))?;
    let apart = |i: usize| (i + HUE_BUCKETS - top) % HUE_BUCKETS >= HUE_BUCKETS / 3 && (top + HUE_BUCKETS - i) % HUE_BUCKETS >= HUE_BUCKETS / 3;
    let next = (0..HUE_BUCKETS).filter(|i| apart(*i) && buckets[*i].0 > buckets[top].0 * MIN_SHARE).max_by(|a, b| buckets[*a].0.total_cmp(&buckets[*b].0));
    Some((colour(buckets[top]), next.map(|i| colour(buckets[i]))))
}

// A second hue needs at least this share of the first's weight to count; less is a speck, not a colour.
const MIN_SHARE: f32 = 0.08;
const HUE_BUCKETS: usize = 12;
// Below this saturation a pixel is grey and doesn't vote for a hue.
const VIVID: f32 = 0.25;

// How light the wash may be: dark enough for white text, light enough to keep the game's colour.
const WASH_LIGHTNESS: (f32, f32) = (0.16, 0.26);
// And how vivid: a wash, not a poster (Discord's olive is a dark one).
const WASH_SATURATION: f32 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wash_takes_two_distinct_hues() {
        let hsl = |h: f32, s: f32, l: f32| Hsla { h, s, l, a: 1. };
        // Putt Party's icon, roughly: a navy field, a yellow stripe, white letters that don't vote.
        let icon: Vec<Hsla> = [vec![hsl(0.64, 0.5, 0.3); 60], vec![hsl(0.15, 0.9, 0.55); 15], vec![hsl(0., 0., 0.95); 25]].concat();
        let (first, second) = leading_hues(&icon).expect("vivid pixels");
        assert!((first.h - 0.64).abs() < 0.02, "navy leads: {first:?}");
        assert!(second.is_some_and(|c| (c.h - 0.15).abs() < 0.02), "yellow follows: {second:?}");
        let mono: Vec<Hsla> = vec![hsl(0.64, 0.5, 0.3); 10];
        assert!(leading_hues(&mono).expect("vivid").1.is_none(), "one hue: the second is the first, darker");
        assert!(leading_hues(&[hsl(0., 0., 0.5)]).is_none(), "all grey");
    }
}
