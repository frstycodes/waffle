// Discord's live activity card: the header with its brand mark and menu, the art, three lines, the party and
// timer badges or a track's progress, then the buttons the activity offers.
use gpui_kit::{FontWeight, Hsla, component::ActiveTheme};

use super::activity::{Fit, art, card_button, head, shell};
use super::activity_text as text;
use super::*;
use crate::discord::{ActivityKind, RichPresence};

impl Waffle {
    pub(super) fn activity_card(&self, card: &ProfileCard, at: usize, rich: &RichPresence, fit: Fit, ink: Ink, cx: &mut Context<Self>) -> Div {
        let (user, menu_rich) = (card.user.clone(), rich.clone());
        let more = (!card.user.bot && !self.activity_menu(&card.user, rich).is_empty()).then(|| {
            ui::icon_button(("profile-activity-more", at), Glyph::More, "More", cx)
                .scoped(gpui_kit::transparent_black(), ink.tile, ink.muted, ink.strong)
                .small()
                .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_activity_menu(user.clone(), menu_rich.clone(), event.position(), cx)))
        });
        let toolbar = self.interaction_toolbar(card, Source::Activity(rich.name.clone()), ACTIVITY_GROUP, ink, cx);
        let menu = ui::row().flex_none().gap_1().children(toolbar).children(more).into_any_element();
        let large = rich.image.clone().or_else(|| rich.app_id.as_deref().and_then(|id| self.app_icon(id, cx)));
        let actions = self.activity_actions(card, at, rich, fit, ink, cx);
        let details = details(rich, fit, ink, cx);
        let (details, foot) = match fit {
            Fit::Wide => (details.children(actions), None),
            Fit::Compact => (details, actions),
        };
        let body = ui::row().map(|r| if fit == Fit::Wide { r.items_start().gap_3() } else { r.gap_2() }).child(art(large, rich.small_image.clone(), fit, ink)).child(details);
        shell(fit, ink).group(ACTIVITY_GROUP).child(head(text::header(rich), text::platform(rich), None, Some(menu), ink)).child(body).children(foot)
    }

    // What the activity lets you do: Spotify's Play and Listen Along, a stream's Watch, the activity's own
    // link buttons, and Join or Ask to Join. Nothing on your own card.
    fn activity_actions(&self, card: &ProfileCard, at: usize, rich: &RichPresence, fit: Fit, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        if self.me.as_ref().is_some_and(|me| me.id == card.user.id) {
            return None;
        }
        let mut buttons: Vec<AnyElement> = Vec::new();
        let link = |id: SharedString, label: String, url: Option<String>, cx: &mut Context<Self>| {
            let missing = url.is_none();
            card_button(id, ink, cx)
                .flex_1()
                .justify_center()
                .disabled(missing)
                .child(ui::line(label))
                .when(missing, |b| b.tooltip(|window, cx| ui::tip("Discord didn't share this link", window, cx)))
                .on_click(move |_, _, cx| {
                    if let Some(url) = &url {
                        cx.open_url(url);
                    }
                })
                .into_any_element()
        };
        if text::is_spotify(rich) {
            buttons.extend(rich.sync_id.clone().map(|track| self.spotify_play(at, track, ink, cx)));
            // GAP: listening along needs Spotify's player connection, which Waffle doesn't make yet.
            buttons.push(card_button(("profile-listen-along", at), ink, cx).disabled(true).child(Icon::new(Glyph::Headphones).size(ui::ICON_MD)).tooltip(|window, cx| ui::tip("Listen Along", window, cx)).into_any_element());
        } else if rich.kind == ActivityKind::Streaming && rich.url.is_some() {
            buttons.push(link(format!("profile-watch-{at}").into(), "Watch".into(), rich.url.clone(), cx));
        } else {
            for (i, label) in rich.buttons.iter().enumerate() {
                buttons.push(link(format!("profile-link-{at}-{i}").into(), label.clone(), rich.button_urls.get(i).cloned(), cx));
            }
        }
        match self.activity_join_button(&card.user.id, rich, cx) {
            Some(join) => buttons.push(join),
            None if rich.joinable && rich.kind == ActivityKind::Playing => buttons.push(ask_to_join(at, ink, cx)),
            None => {}
        }
        if buttons.is_empty() {
            return None;
        }
        let row = ui::row().flex_wrap().gap_2().children(buttons);
        Some(if fit == Fit::Compact { row.w_full().pt_1() } else { row.pt_1() })
    }

    fn spotify_play(&self, at: usize, track: String, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let mark = crate::icons::brand_mark("spotify").map(|path| gpui_kit::svg().path(path).size(ui::ICON_MD).flex_none().text_color(ink.strong));
        card_button(("profile-spotify-play", at), ink, cx)
            .flex_1()
            .justify_center()
            .gap_1()
            .children(mark)
            .child("Play on Spotify")
            .on_click(move |_, _, cx| cx.open_url(&format!("https://open.spotify.com/track/{track}")))
            .into_any_element()
    }
}

// GAP: the join request endpoint isn't wired, so the button shows the party is open but can't send yet.
fn ask_to_join(at: usize, ink: Ink, cx: &App) -> AnyElement {
    card_button(("profile-ask-to-join", at), ink, cx)
        .flex_1()
        .justify_center()
        .gap_1()
        .disabled(true)
        .child(Icon::new(Glyph::Game).size(ui::ICON_MD))
        .child("Ask to Join")
        .tooltip(|window, cx| ui::tip("Joining from Waffle isn't supported yet", window, cx))
        .into_any_element()
}

// The lines, then the badges or the track's bar. Compact cards keep each line to one; wide ones wrap.
fn details(rich: &RichPresence, fit: Fit, ink: Ink, cx: &App) -> Div {
    let [first, second, third] = text::lines(rich);
    let line = |text: String| match fit {
        Fit::Compact => ui::line(text),
        Fit::Wide => div().child(text),
    };
    let lines = ui::col()
        .min_w_0()
        .text_color(ink.text)
        .children(first.map(|t| line(t).text_size(ui::TEXT_SM).line_height(ui::LINE_DETAIL).font_weight(FontWeight::SEMIBOLD)))
        .children([second, third].into_iter().flatten().map(|t| line(t).text_size(ui::TEXT_XS).line_height(ui::LINE_XS)));
    ui::col().flex_1().min_w_0().gap_1().justify_center().child(lines).children(badges(rich, ink)).children(progress(rich, ink, cx))
}

// The party, the green clock, and a show's episode, in Discord's order for the kind.
fn badges(rich: &RichPresence, ink: Ink) -> Option<Div> {
    let party = text::party(rich).map(|t| fact(Icon::new(Glyph::Users), t, ink.muted));
    let timer = rich.started.filter(|_| text::has_timer(rich)).map(|start| {
        let icon = crate::sidebar::activity_glyph(rich.kind).unwrap_or_else(|| Icon::new(Glyph::Game));
        fact(icon, crate::call::clock(((chrono::Utc::now().timestamp_millis() - start).max(0) / 1000) as u64), timer_green(ink))
    });
    let episode = text::episode(rich).map(|t| fact(Icon::new(Glyph::Screen), t, ink.muted));
    let all: Vec<Div> = [party, timer, episode].into_iter().flatten().collect();
    (!all.is_empty()).then(|| ui::row().flex_wrap().gap_2().children(all))
}

fn fact(icon: Icon, text: String, color: Hsla) -> Div {
    ui::row().gap_1().text_size(ui::TEXT_XS).text_color(color).child(icon.size(ui::ICON_XS).flex_none()).child(ui::line(text))
}

// Status green is ~2:1 on a light card; a deeper green keeps it readable there.
fn timer_green(ink: Ink) -> Hsla {
    if luminance(ink.strong) < 0.5 { palette::online_on_light() } else { palette::status(crate::discord::Status::Online) }
}

// A track's bar with where it is and how long it runs at its ends, in the code font.
fn progress(rich: &RichPresence, ink: Ink, cx: &App) -> Option<Div> {
    let (elapsed, length) = text::track_position(rich, chrono::Utc::now().timestamp_millis()).filter(|_| text::has_progress(rich))?;
    let done = if length == 0 { 0. } else { elapsed as f32 / length as f32 };
    let mono = cx.theme().mono_font_family.clone();
    let time = |secs: u64| div().flex_none().font_family(mono.clone()).text_size(ui::TEXT_XS).text_color(ink.text).child(text::track_time(secs));
    let bar = div().flex_1().child(ui::progress_bar(done, cx).fill(ink.strong).track(ink.strong.opacity(0.16)).thin());
    Some(ui::row().gap_2().child(time(elapsed)).child(bar).child(time(length)))
}
