// The profile popout, opened from a name or avatar: Discord's layout in the user's profile theme.
use std::time::Duration;

use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, MouseButton, Pixels, Point, SharedString, Stateful, StyledText, Window,
    component::{ActiveTheme, Icon, text::TextView, tooltip::Tooltip},
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative, rgb,
};

use crate::{
    ProfileCard, Waffle,
    discord::{ActivityKind, Badge, Profile, Relation, RichPresence, User, perm},
    icons::Glyph,
    layout::{TITLE_HEIGHT, decorated_avatar},
    palette,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
    user_menu::Surface,
};

const WIDTH: f32 = 320.;
const BORDER: f32 = 2.;
// Discord banners are 5:2; at that height a cover fit crops nothing.
const BANNER: f32 = WIDTH * 0.4;
const BANNER_PLAIN: f32 = 64.;
pub(crate) const AVATAR: f32 = 80.;
pub(crate) const AVATAR_RING: f32 = 6.;
// Only picks which corner the card rises from; `anchored` places it by its measured size.
const ESTIMATED_HEIGHT: f32 = 560.;
const MAX_BADGES: usize = 6;
const MAX_GAMES: usize = 3;

impl Waffle {
    pub(crate) fn open_profile(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        let guild_id = self.guild_id.clone();
        // The last profile we fetched shows at once; a stale one refreshes underneath it.
        let key = profile_key(&user.id, guild_id.as_deref());
        let cached = self.profiles.cached(&key);
        self.profile_card = Some(ProfileCard { user: user.clone(), guild_id: guild_id.clone(), at, profile: cached, all_activity: false });
        cx.notify();
        if self.presences.get(&user.id).and_then(|p| p.rich.as_ref()).is_some_and(|r| r.started.is_some()) {
            self.tick_profile_timer(user.id.clone(), cx);
        }
        self.fetch_profile(user, guild_id, cx);
    }

    // Fetches a profile unless a fresh one is cached; an open card on this user takes it when it lands.
    pub(crate) fn fetch_profile(&mut self, user: User, guild_id: Option<String>, cx: &mut Context<Self>) {
        let key = profile_key(&user.id, guild_id.as_deref());
        let Some(client) = self.client.clone() else { return };
        if !self.profiles.should_fetch(&key) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let fetched = client.profile(&user.id, guild_id.as_deref()).await;
            this.update(cx, |this, cx| {
                let Ok(profile) = fetched else { return this.profiles.failed(&key) };
                this.profiles.store(&key, profile.clone());
                if let Some(card) = this.profile_card.as_mut().filter(|c| c.user.id == user.id) {
                    card.profile = Some(profile);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // A changed member or user: their cached profiles refetch on next open, still shown meanwhile.
    pub(crate) fn profile_changed(&mut self, user_id: &str) {
        let prefix = format!("{user_id}-");
        self.profiles.changed(|key| key.starts_with(&prefix));
    }

    // The activity's elapsed time counts up each second while the card stays open on this user.
    fn tick_profile_timer(&self, user_id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let open = this
                    .update(cx, |this, cx| {
                        let open = this.profile_card.as_ref().is_some_and(|c| c.user.id == user_id);
                        if open {
                            cx.notify();
                        }
                        open
                    })
                    .unwrap_or(false);
                if !open {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn profile_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.profile_card.as_ref()?;
        let viewport = window.viewport_size();
        let profile = card.profile.clone().unwrap_or_default();
        let ink = Ink::new(profile.theme, cx);
        let x = f32::from(card.at.x).clamp(8., f32::from(viewport.width) - WIDTH - 8.);
        let top = TITLE_HEIGHT + 8.;
        let y = f32::from(card.at.y).clamp(top, (f32::from(viewport.height) - ESTIMATED_HEIGHT - 8.).max(top));
        let max_height = f32::from(viewport.height) - top - 8.;
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let inner_radius = inner_radius(ink);
        let body = div()
            .id("profile-body")
            .min_h_0()
            .overflow_y_scroll()
            .px_4()
            .pb_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(name_block(&card.user, &profile, ink))
            // Not fetched yet: where the badges, mutuals and bio will be.
            .when(card.profile.is_none(), |b| b.child(profile_skeleton(cx)))
            .children(badges(&profile.badges, ink))
            .children(mutuals(&profile, ink))
            .children(profile.bio.as_ref().map(|bio| bio_view(&card.user.id, bio, ink)))
            .children(game_collection(&profile.games, ink))
            .children(self.profile_activity(card, ink, cx))
            .children(self.roles_row(card, &profile, ink, cx));
        let (outer, inner) = themed_card(ink, max_height, cx);
        let inner = inner
            .child(self.profile_header(card, &profile, is_me, inner_radius, ink, cx))
            .child(body)
            .when(!is_me, |c| c.child(message_field(&card.user, ink, cx)));
        let card_el = outer
            .id("profile-card")
            .occlude()
            .w(px(WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(inner);
        // It rises from the corner nearest where it was asked for: pushed in from an edge, from that side.
        let origin = ui::corner(x < f32::from(card.at.x), y < f32::from(card.at.y));
        let rise = ui::rise(card_el.transform_origin(origin), SharedString::from(format!("profile-{}", card.user.id)), Duration::ZERO);
        // Measured, not estimated: a short card opened low sits just above the window's foot, not 560px up.
        let placed = gpui_kit::anchored().position(card.at).snap_to_window_with_margin(gpui_kit::Edges { top: px(top), right: px(8.), bottom: px(8.), left: px(8.) }).child(rise);
        Some(crate::overlays::backdrop("profile-backdrop", cx).child(placed).into_any_element())
    }

    // Banner with its round actions, then the avatar overlapping its foot.
    fn profile_header(&self, card: &ProfileCard, profile: &Profile, is_me: bool, radius: f32, ink: Ink, cx: &mut Context<Self>) -> Div {
        let user = &card.user;
        let status = self.presences.get(&user.id).map(|p| p.mark()).or(is_me.then_some(self.my_status.into()));
        let (banner, _) = banner(profile, ink, radius, cx);
        let decoration = user.decoration.clone().or(profile.decoration.clone());
        // An open profile's avatar and decoration move, as Discord's.
        let decoration = decoration.as_deref().map(crate::discord::animated);
        let avatar = div().p(px(AVATAR_RING)).rounded_full().bg(ink.top).child(decorated_avatar(&crate::discord::animated(&user.avatar), decoration.as_deref(), AVATAR, status, ink.top));
        let edit = is_me.then(|| {
            ui::button("profile-edit", ui::Tone::Secondary, cx).small().child(Icon::new(Glyph::Pen).size(px(16.))).child("Edit profile").on_click(cx.listener(|this, _, window, cx| {
                this.profile_card = None;
                this.open_settings(crate::settings::Page::Account, window, cx);
            }))
        });
        div()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .child(banner)
            .when(!is_me, |h| h.child(self.banner_actions(card, cx)))
            .child(div().px(px(16. - AVATAR_RING)).mt(px(-(AVATAR / 2. + AVATAR_RING))).mb_2().flex().items_end().justify_between().child(avatar).children(edit))
    }

    // Discord's round buttons over the banner: the friend state, then More.
    fn banner_actions(&self, card: &ProfileCard, cx: &mut Context<Self>) -> Div {
        let user = card.user.clone();
        let relation = self.relationships.iter().find(|r| r.user.id == user.id).map(|r| r.kind);
        let friend = match relation {
            _ if user.bot => None,
            None => Some((Glyph::GroupPlus, "Add Friend", Some(Relation::Outgoing))),
            Some(Relation::Incoming) => Some((Glyph::GroupPlus, "Accept Friend Request", Some(Relation::Friend))),
            Some(Relation::Outgoing) => Some((Glyph::Clock, "Friend Request Sent", None)),
            Some(Relation::Friend) => Some((Glyph::Users, "Friends", None)),
            Some(Relation::Blocked) => None,
        };
        let friend = friend.map(|(glyph, label, next)| {
            let target = user.clone();
            banner_button("profile-friend", glyph, label).when_some(next, |b, next| {
                b.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.relation_optimistic(&target.id, Some((target.clone(), next)), reqwest::Method::PUT, Some(serde_json::json!({})), cx)))
            })
        });
        let guild_id = card.guild_id.clone();
        let more = banner_button("profile-more", Glyph::More, "More")
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), menu_surface(guild_id.is_some()), event.position(), cx)));
        div().absolute().top(px(12.)).right(px(12.)).flex().gap_2().children(friend).child(more)
    }

    // Discord's rich presence card: heading and menu, the large image, then name, details, state with
    // the party size, the elapsed time, and Ask to Join when the party is open.
    // Discord's popout shows one activity card: their stream, else their first activity, else the voice
    // channel they're in.
    fn profile_activity(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Option<AnyElement> {
        let place = self.guilds.iter().find_map(|g| {
            let state = g.voice_states.iter().find(|v| v.user_id == card.user.id)?;
            let channel = g.channels.iter().find(|c| Some(&c.id) == state.channel_id.as_ref())?;
            Some((g, channel, state.streaming))
        });
        // Discord's order: the stream, each activity, then voice when not streaming. The popout shows
        // the first; the rest wait behind Show more.
        let mut cards: Vec<AnyElement> = Vec::new();
        if let Some((guild, channel, true)) = place {
            cards.push(self.stream_activity(card, guild, channel, ink, cx));
        }
        if let Some(rich) = self.presences.get(&card.user.id).and_then(|p| p.rich.as_ref()) {
            cards.push(self.activity_card(card, rich, ink, cx).into_any_element());
        }
        if let Some((guild, channel, false)) = place {
            cards.push(self.voice_activity(card, guild, channel, ink, cx));
        }
        if cards.is_empty() {
            return None;
        }
        let more = cards.len() - 1;
        let shown = if card.all_activity { cards.len() } else { 1 };
        let column = div().flex().flex_col().gap(px(8.)).children(cards.into_iter().take(shown));
        if more == 0 || card.all_activity {
            return Some(column.into_any_element());
        }
        let show_more = ui::button("profile-activity-more", ui::Tone::Secondary, cx).small().w_full().child("Show more").on_click(cx.listener(|this, _, _, cx| {
            if let Some(card) = &mut this.profile_card {
                card.all_activity = true;
            }
            cx.notify();
        }));
        Some(column.child(show_more).into_any_element())
    }

    // .profile-stream: "Streaming {game}" with LIVE, the stream's preview (a click watches it, joining the
    // call first if need be), where it is, and Join Voice.
    fn stream_activity(&self, card: &ProfileCard, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let user_id = card.user.id.clone();
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user_id);
        let game = self.presences.get(&user_id).and_then(|p| p.activity.as_ref()).filter(|a| a.kind == ActivityKind::Playing).map(|a| a.text.clone());
        let heading = game.map_or_else(|| "Streaming".to_owned(), |game| format!("Streaming {game}"));
        let connected = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        let watch_channel = channel.clone();
        let preview = div()
            .id("profile-stream-preview")
            .relative()
            .w_full()
            .aspect_ratio(16. / 9.)
            .child(crate::call::stream_fill(RADIUS_SM).absolute().inset_0())
            .children(self.stream_preview(&user_id).map(|url| ui::media(url, ui::all(RADIUS_SM)).absolute().size_full().object_fit(gpui_kit::ObjectFit::Cover)))
            .when(!is_me, |d| {
                d.cursor_pointer().on_click(cx.listener(move |this, _, window, cx| {
                    this.profile_card = None;
                    match connected {
                        true => this.watch_stream(user_id.clone(), cx),
                        false => this.watch_on_connect = Some(user_id.clone()),
                    }
                    this.show_call(watch_channel.clone(), window, cx);
                }))
            });
        let head = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(heading))
            .child(ui::live_badge());
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap(px(10.))
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(head)
            .child(preview)
            .child(self.voice_place_row(guild, channel, ink))
            .children((!is_me && !connected).then(|| self.join_voice_button(guild, channel, cx)))
            .into_any_element()
    }

    // .profile-voice: "In voice", who's there, where, and Join Voice.
    fn voice_activity(&self, card: &ProfileCard, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink, cx: &mut Context<Self>) -> AnyElement {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let connected = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id && c.connected);
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap(px(10.))
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("In voice"))
            .child(self.voice_place_row(guild, channel, ink))
            .children((!is_me && !connected).then(|| self.join_voice_button(guild, channel, cx)))
            .into_any_element()
    }

    // The server › channel breadcrumb, and who's in it.
    fn voice_place_row(&self, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink) -> Div {
        let faces: Vec<String> = guild.voice_states.iter().filter(|v| v.channel_id.as_ref() == Some(&channel.id)).map(|v| v.user.as_ref().map_or_else(|| crate::discord::avatar_url(&v.user_id, None), |u| u.avatar.clone())).collect();
        let server = match &guild.icon {
            Some(icon) => ui::media(icon.clone(), ui::all(RADIUS_XS)).size(px(20.)).flex_none().into_any_element(),
            None => div().size(px(20.)).flex_none().squircle(RADIUS_XS).bg(ink.top).into_any_element(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .min_w_0()
            .text_size(px(14.))
            .text_color(ink.text)
            .child(server)
            .child(Icon::new(Glyph::ChevronRight).size(px(14.)).flex_none().text_color(ink.muted))
            .child(crate::sidebar::channel_glyph(channel).size(px(16.)).flex_none().text_color(ink.muted))
            .child(div().flex_1().min_w_0().truncate().child(channel.name.clone()))
            .child(ui::capped_stack(&faces, 3, 20., ink.tile, ink.top, ink.strong))
    }

    fn join_voice_button(&self, guild: &crate::discord::Guild, channel: &crate::discord::Channel, cx: &mut Context<Self>) -> AnyElement {
        let (guild_id, channel) = (guild.id.clone(), channel.clone());
        ui::button("profile-join-voice", ui::Tone::Secondary, cx)
            .w_full()
            .justify_center()
            .child("Join Voice")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.profile_card = None;
                this.join_voice_in(Some(guild_id.clone()), channel.clone(), cx);
                cx.notify();
            }))
            .into_any_element()
    }

    fn activity_card(&self, card: &ProfileCard, rich: &RichPresence, ink: Ink, cx: &mut Context<Self>) -> Div {
        let (user, guild_id) = (card.user.clone(), card.guild_id.clone());
        let more = div()
            .id("profile-activity-more")
            .size(px(24.))
            .squircle(RADIUS_XS)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(ink.muted)
            .hover(move |s| s.bg(ink.tile).text_color(ink.strong))
            .child(Icon::new(Glyph::More).size(px(16.)))
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), menu_surface(guild_id.is_some()), event.position(), cx)));
        let art = match &rich.image {
            Some(url) => ui::media(url.clone(), ui::all(RADIUS_SM)).size(px(72.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => div().size(px(72.)).flex_none().squircle(RADIUS_SM).bg(ink.tile).flex().items_center().justify_center().text_color(ink.muted).child(Icon::new(Glyph::Game).size(px(32.))).into_any_element(),
        };
        let line = |text: String| div().truncate().text_color(ink.text).child(text);
        let state = rich.state.clone().map(|state| match rich.party {
            Some((current, max)) => format!("{state} ({current} of {max})"),
            None => state,
        });
        let elapsed = rich.started.map(|start| crate::call::clock(((chrono::Utc::now().timestamp_millis() - start).max(0) / 1000) as u64));
        let mono = cx.theme().mono_font_family.clone();
        let facts = div()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(2.))
            .text_size(px(14.))
            .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(ink.strong).child(rich.name.clone()))
            .children(rich.details.clone().map(line))
            .children(state.map(|s| div().flex().items_center().gap_1().min_w_0().text_color(ink.text).when(rich.party.is_some(), |d| d.child(Icon::new(Glyph::Users).size(px(14.)).flex_none())).child(div().truncate().child(s))))
            .children(elapsed.map(|time| {
                // Status green is ~2:1 on a light card; a deeper green keeps it readable there.
                let green = if luminance(ink.strong) < 0.5 { rgb(0x1E7A46).into() } else { palette::status(crate::discord::Status::Online) };
                div().flex().items_center().gap_1().text_color(green).child(Icon::new(Glyph::Game).size(px(14.))).child(div().font_family(mono).child(time))
            }));
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap(px(10.))
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().flex().items_center().justify_between().child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(activity_heading(rich.kind))).child(more))
            .child(div().flex().items_center().gap_3().min_w_0().child(art).child(facts))
            .when(rich.joinable, |c| c.child(ask_to_join(ink)))
    }

    // Their roles, then "Add role" for members who can manage roles.
    fn roles_row(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id))?;
        let mut roles: Vec<_> = guild.roles.iter().filter(|r| profile.roles.contains(&r.id)).collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.position));
        let can_manage = self.me.as_ref().is_some_and(|me| guild.can(&me.id, perm::MANAGE_ROLES));
        if roles.is_empty() && !can_manage {
            return None;
        }
        let chips = roles.into_iter().map(|r| {
            let color = if r.color == 0 { palette::status_in(crate::discord::Status::Offline, cx) } else { rgb(r.color).into() };
            ui::role_chip(r.name.clone(), color, cx).bg(ink.tile).text_color(ink.text)
        });
        let (user, guild_id) = (card.user.clone(), guild.id.clone());
        let add = can_manage.then(|| {
            div()
                .id("profile-add-role")
                .h(px(24.))
                .px(px(6.))
                .flex()
                .items_center()
                .gap_1()
                .squircle(RADIUS_XS)
                .cursor_pointer()
                .text_size(px(14.))
                .text_color(ink.muted)
                .hover(move |s| s.bg(ink.tile).text_color(ink.strong))
                .child(Icon::new(Glyph::Plus).size(px(14.)))
                .child("Add role")
                .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_user_menu(user.clone(), Some(guild_id.clone()), Surface::ServerChat, event.position(), cx)))
        });
        Some(div().flex().flex_wrap().items_center().gap_1().children(chips).children(add))
    }
}

// The card's colours. Themed: the user's two colours washed toward the theme's ground for the body,
// raw for the border, with ink picked by the body's luminance. Otherwise the plain overlay surface.
#[derive(Clone, Copy)]
pub(crate) struct Ink {
    pub(crate) top: Hsla,
    pub(crate) bottom: Hsla,
    pub(crate) border: Option<(Hsla, Hsla)>,
    pub(crate) strong: Hsla,
    pub(crate) text: Hsla,
    pub(crate) muted: Hsla,
    pub(crate) tile: Hsla,
}

impl Ink {
    pub(crate) fn new(theme: Option<(u32, u32)>, cx: &App) -> Self {
        let Some((from, to)) = theme else {
            let surface = palette::surface(palette::Level::Overlay, cx);
            return Ink { top: surface, bottom: surface, border: None, strong: palette::strong(cx), text: palette::text(cx), muted: palette::muted(cx), tile: palette::selected(cx) };
        };
        let (from, to): (Hsla, Hsla) = (rgb(from).into(), rgb(to).into());
        let dark = cx.theme().is_dark();
        let ground: Hsla = if dark { gpui_kit::black() } else { gpui_kit::white() };
        let (top, bottom) = (ui::mix(from, ground, 0.55), ui::mix(to, ground, 0.55));
        let strong: Hsla = if luminance(ui::mix(top, bottom, 0.5)) > 0.4 { gpui_kit::rgb(0x1A1A1E).into() } else { gpui_kit::white() };
        Ink { top, bottom, border: Some((from, to)), strong, text: strong.opacity(0.92), muted: strong.opacity(0.72), tile: strong.opacity(0.1) }
    }
}

fn luminance(color: Hsla) -> f32 {
    let c = gpui_kit::Rgba::from(color);
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

// The banner with its height: the picture at 5:2, else a strip of the theme or accent colour.
pub(crate) fn banner(profile: &Profile, ink: Ink, radius: f32, cx: &App) -> (AnyElement, f32) {
    match (&profile.banner, ink.border, profile.accent) {
        (Some(url), _, _) => (ui::media(url.clone(), ui::top(radius)).w_full().h(px(BANNER)).object_fit(gpui_kit::ObjectFit::Cover).into_any_element(), BANNER),
        (None, Some((color, _)), _) => (banner_fill(color, radius).into_any_element(), BANNER_PLAIN),
        (None, None, Some(accent)) => (banner_fill(rgb(accent).into(), radius).into_any_element(), BANNER_PLAIN),
        (None, None, None) => (banner_fill(palette::selected(cx), radius).into_any_element(), BANNER_PLAIN),
    }
}

// The card in the profile's colours: its gradient body inside the gradient border when the profile has a
// theme. Returns (outer, inner); content goes in the inner.
pub(crate) fn themed_card(ink: Ink, max_height: f32, cx: &App) -> (Div, Div) {
    let inner = div()
        .flex()
        .flex_col()
        .max_h(px(max_height - if ink.border.is_some() { 2. * BORDER } else { 0. }))
        .squircle(inner_radius(ink))
        .bg(linear_gradient(180., linear_color_stop(ink.top, 0.), linear_color_stop(ink.bottom, 1.)));
    let outer = div()
        .squircle(RADIUS_LG)
        .shadow(ui::shadow_pop(cx))
        .when_some(ink.border, |c, (from, to)| c.p(px(BORDER)).bg(linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.))));
    (outer, inner)
}

pub(crate) fn inner_radius(ink: Ink) -> f32 {
    if ink.border.is_some() { RADIUS_LG - BORDER } else { RADIUS_LG }
}

fn banner_fill(color: Hsla, radius: f32) -> Div {
    ui::edge(div().w_full().h(px(BANNER_PLAIN)).bg(color), ui::top(radius))
}

fn banner_button(id: &'static str, glyph: Glyph, label: &'static str) -> Stateful<Div> {
    let fill = gpui_kit::black().opacity(0.55);
    div()
        .id(id)
        .size(px(32.))
        .rounded_full()
        .bg(fill)
        .hover(|s| s.bg(gpui_kit::black().opacity(0.75)))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .child(Icon::new(glyph).size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
}

fn menu_surface(in_guild: bool) -> Surface {
    if in_guild { Surface::ServerChat } else { Surface::Dm }
}

// Display name, then "username • pronouns" with the guild tag chip on that line.
pub(crate) fn name_block(user: &User, profile: &Profile, ink: Ink) -> Div {
    let name = user.name_style.as_ref().map(|style| crate::chat::name_runs(&user.name, style, ui::mix(ink.top, ink.bottom, 0.5))).unwrap_or_default();
    let handle = [Some(profile.username.clone()).filter(|u| !u.is_empty()), profile.pronouns.clone()].into_iter().flatten().collect::<Vec<_>>().join(" • ");
    let tag = user.tag.as_ref().map(|tag| {
        div()
            .h(px(22.))
            .px(px(6.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .squircle(RADIUS_XS)
            .bg(ink.tile)
            .text_color(ink.strong)
            .text_size(px(14.))
            .font_weight(FontWeight::MEDIUM)
            .children(tag.badge.clone().map(|badge| img(badge).size(px(14.))))
            .child(tag.text.clone())
    });
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(crate::name_fonts::styled(div().truncate().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD), user.name_style.as_ref()).text_color(ink.strong).child(StyledText::new(user.name.clone()).with_highlights(name)))
        .when(!handle.is_empty() || tag.is_some(), |b| {
            b.child(div().flex().flex_wrap().items_center().gap_x(px(6.)).gap_y(px(4.)).text_size(px(14.)).text_color(ink.text).when(!handle.is_empty(), |l| l.child(div().min_w_0().truncate().child(handle))).children(tag))
        })
}

fn badges(badges: &[Badge], ink: Ink) -> Option<Div> {
    if badges.is_empty() {
        return None;
    }
    let shown = badges.iter().take(MAX_BADGES).enumerate().map(|(i, badge)| {
        let label = SharedString::from(badge.description.clone());
        div().id(("profile-badge", i)).size(px(22.)).child(img(badge.icon.clone()).size_full()).tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
    });
    let more = badges.len().saturating_sub(MAX_BADGES);
    let pill = div()
        .h(px(30.))
        .px(px(6.))
        .flex()
        .items_center()
        .gap(px(4.))
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .children(shown)
        .when(more > 0, |p| p.child(div().pl(px(2.)).text_size(px(14.)).text_color(ink.text).child(format!("+{more}"))));
    Some(div().flex().child(pill))
}

fn mutuals(profile: &Profile, ink: Ink) -> Option<Div> {
    let parts: Vec<String> = [(profile.mutual_friend_count, "Mutual Friend"), (profile.mutual_server_count, "Mutual Server")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, noun)| if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") })
        .collect();
    if parts.is_empty() {
        return None;
    }
    let faces = profile.mutual_friends.iter().take(3).map(|u| u.avatar.clone());
    Some(div().flex().items_center().gap_2().text_size(px(14.)).text_color(ink.muted).child(ui::avatar_stack(faces, 20., ink.top)).child(div().truncate().child(parts.join(" • "))))
}

// Discord markdown with custom emoji as images, as a message renders it; Discord gives the bio no heading.
fn bio_view(user_id: &str, bio: &str, ink: Ink) -> Div {
    let view = TextView::markdown(SharedString::from(format!("bio-{user_id}")), crate::chat::inline_emoji(&crate::chat::discord_breaks(bio))).selectable(true);
    div().text_size(px(14.)).text_color(ink.text).max_h(px(160.)).child(view)
}

fn game_collection(games: &[String], ink: Ink) -> Option<Div> {
    if games.is_empty() {
        return None;
    }
    let more = games.len().saturating_sub(MAX_GAMES);
    let tile = |el: Div| el.size(px(32.)).flex_none().squircle(RADIUS_XS);
    let icons = games.iter().take(MAX_GAMES).map(|url| ui::media(url.clone(), ui::all(RADIUS_XS)).size(px(32.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover));
    let overflow = (more > 0).then(|| {
        tile(div()).bg(gpui_kit::black().opacity(0.6)).flex().items_center().justify_center().text_size(px(14.)).text_color(gpui_kit::white()).child(format!("+{more}"))
    });
    Some(
        div()
            .px_3()
            .py(px(10.))
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("Game Collection"))
            .child(div().flex().gap_1().children(icons).children(overflow)),
    )
}

fn activity_heading(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Streaming => "Streaming",
        ActivityKind::Listening => "Listening to",
        ActivityKind::Watching => "Watching",
        ActivityKind::Competing => "Competing in",
        ActivityKind::Playing | ActivityKind::Custom => "Playing",
    }
}

// GAP: the join request endpoint isn't wired, so the button shows the party is open but can't send yet.
fn ask_to_join(ink: Ink) -> Stateful<Div> {
    div()
        .id("profile-ask-to-join")
        .h(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .text_color(ink.muted)
        .text_size(px(14.))
        .opacity(0.7)
        .child(Icon::new(Glyph::Game).size(px(16.)))
        .child("Ask to Join")
        .tooltip(|window, cx| Tooltip::new("Joining from Waffle isn't supported yet").build(window, cx))
}

fn message_field(user: &User, ink: Ink, cx: &mut Context<Waffle>) -> Div {
    let dm_user = user.id.clone();
    let field = div()
        .id("profile-message")
        .h(px(44.))
        .px(px(14.))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .squircle(RADIUS_MD)
        .bg(ink.tile)
        .cursor_text()
        .child(div().truncate().text_size(px(14.)).text_color(ink.muted).child(format!("Message @{}", user.name)))
        .child(Icon::new(Glyph::Emoji).size(px(16.)).flex_none().text_color(ink.muted))
        .on_click(cx.listener(move |this, _, window, cx| this.message_user(dm_user.clone(), window, cx)));
    div().flex_none().px_4().pt_1().pb_4().child(field)
}

// A profile differs per server (nickname, roles, server avatar), so the server is part of its key.
pub(crate) fn profile_key(user_id: &str, guild_id: Option<&str>) -> String {
    format!("{user_id}-{}", guild_id.unwrap_or("dm"))
}

fn profile_skeleton(cx: &App) -> Div {
    let line = |id: &'static str, width: f32| ui::skeleton(id, 4., cx).h(px(12.)).w(relative(width));
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(ui::skeleton("profile-skel-badges", 8., cx).h(px(28.)).w(px(120.)))
        .child(line("profile-skel-mutuals", 0.7))
        .child(line("profile-skel-bio-1", 0.9))
        .child(line("profile-skel-bio-2", 0.55))
}
