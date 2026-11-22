// The profile popout, opened from a name or avatar: Discord's layout in the user's profile theme.
use std::time::Duration;

use gpui_kit::{
    AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, MouseButton, Pixels, Point, SharedString, Stateful, StyledText, Window,
    component::{ActiveTheme, Icon, text::TextView, tooltip::Tooltip},
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative, rgb, svg,
};
use gpui_motion_kit::core::{Bezier, Easing, Motion, Target, Transition};

use crate::{
    Part, ProfileCard, Waffle,
    discord::{ActivityKind, Badge, Connection, EffectLayer, Profile, Relation, RichPresence, User, perm},
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
// Discord's activity peek: the card under the first shows PEEK below it, inset PEEK_INSET each side; on
// hover the first lifts PEEK_LIFT and the peek grows by as much again.
const PEEK: f32 = 8.;
const PEEK_INSET: f32 = 8.;
const PEEK_LIFT: f32 = 2.;
const PEEK_HOVER: &str = "profile-activity-peek";

impl Waffle {
    pub(crate) fn open_profile(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        let guild_id = self.guild_id.clone();
        // The last profile we fetched shows at once; a stale one refreshes underneath it.
        let key = profile_key(&user.id, guild_id.as_deref());
        let cached = self.profiles.cached(&key);
        let effect = cached.as_ref().and_then(|p| p.effect_sku.clone());
        self.profile_card = Some(ProfileCard { user: user.clone(), guild_id: guild_id.clone(), at, profile: cached, all_activity: false, avatar_full: false });
        self.fetch_profile_effect(effect, cx);
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
                this.fetch_profile_effect(profile.effect_sku.clone(), cx);
                if let Some(card) = this.profile_card.as_mut().filter(|c| c.user.id == user.id) {
                    card.profile = Some(profile);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // Once per sku: an effect that fails to resolve stays None and the card draws none.
    fn fetch_profile_effect(&mut self, sku: Option<String>, cx: &mut Context<Self>) {
        let Some(sku) = sku else { return };
        let Some(client) = self.client.clone() else { return };
        if self.profile_effects.contains_key(&sku) {
            return;
        }
        self.profile_effects.insert(sku.clone(), None);
        cx.spawn(async move |this, cx| {
            let Ok(layers) = client.profile_effect(&sku).await else { return };
            this.update(cx, |this, cx| {
                this.profile_effects.insert(sku, Some(layers));
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

    pub(crate) fn profile_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.profile_card.as_ref()?;
        let peek_hovered = self.hovered.as_deref() == Some(PEEK_HOVER);
        let lift = self.motions.frame_one(Part::ActivityPeek, Motion::new(Target::default().y(if peek_hovered { -PEEK_LIFT } else { 0. })).transition(peek_ease()), window, cx).target().y_px();
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
            .children(self.member_since(card, &profile, ink, cx))
            .children(self.friends_since(card, ink))
            .children(connections(&profile.connections, ink, cx))
            .children(game_collection(&profile.games, ink))
            .children(self.profile_activity(card, lift, ink, cx))
            .children(self.roles_row(card, &profile, ink, cx));
        let (outer, inner) = themed_card(ink, max_height, cx);
        let inner = inner
            .child(self.profile_header(card, &profile, is_me, inner_radius, ink, cx))
            .child(body)
            .when(!is_me, |c| c.child(message_field(&card.user, ink, cx)));
        let effect = profile.effect_sku.as_ref().and_then(|sku| self.profile_effects.get(sku)).and_then(|layers| layers.as_deref()).and_then(effect_overlay);
        let card_el = outer
            .id("profile-card")
            .occlude()
            .relative()
            .w(px(WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(inner)
            .children(effect);
        // It rises from the corner nearest where it was asked for: pushed in from an edge, from that side.
        let origin = ui::corner(x < f32::from(card.at.x), y < f32::from(card.at.y));
        let rise = ui::rise(card_el.transform_origin(origin), SharedString::from(format!("profile-{}", card.user.id)), Duration::ZERO);
        // Measured, not estimated: a short card opened low sits just above the window's foot, not 560px up.
        let placed = gpui_kit::anchored().position(card.at).snap_to_window_with_margin(gpui_kit::Edges { top: px(top), right: px(8.), bottom: px(8.), left: px(8.) }).child(rise);
        let avatar = card.avatar_full.then(|| avatar_viewer(&card.user, window, cx));
        Some(crate::overlays::backdrop("profile-backdrop", cx).child(placed).children(avatar).into_any_element())
    }

    // Discord's date from the account's id, then, opened in a server, the server's from when they joined.
    fn member_since(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &App) -> Option<Div> {
        card.profile.as_ref()?;
        let created = snowflake_time(&card.user.id)?;
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let joined = guild.zip(profile.joined.as_deref().and_then(parse_time));
        let discord_mark = div()
            .size(px(20.))
            .flex_none()
            .squircle(RADIUS_XS)
            .bg(palette::accent())
            .flex()
            .items_center()
            .justify_center()
            .children(crate::icons::brand_mark("discord").map(|path| svg().path(path).size(px(14.)).text_color(palette::on_accent())));
        let row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .child(since_date("profile-since-discord", discord_mark, "Discord".into(), &created, ink))
            .when_some(joined, |row, (guild, joined)| {
                let icon = ui::server_tile(guild.icon.as_deref(), &guild.name, 20., cx);
                row.child(div().size(px(4.)).rounded_full().bg(ink.muted)).child(since_date("profile-since-server", icon, guild.name.clone().into(), &joined, ink))
            });
        Some(section(if card.user.bot { "Created On" } else { "Member Since" }, row, ink))
    }

    fn friends_since(&self, card: &ProfileCard, ink: Ink) -> Option<Div> {
        let friend = self.relationships.iter().find(|r| r.user.id == card.user.id && r.kind == Relation::Friend)?;
        let since = parse_time(friend.since.as_deref()?)?;
        Some(section("Friends Since", div().text_size(px(14.)).text_color(ink.text).child(short_date(&since)), ink))
    }

    // Banner with its round actions, then the avatar overlapping its foot.
    fn profile_header(&self, card: &ProfileCard, profile: &Profile, is_me: bool, radius: f32, ink: Ink, cx: &mut Context<Self>) -> Div {
        let user = &card.user;
        let status = self.presences.get(&user.id).map(|p| p.mark()).or(is_me.then_some(self.my_status.into()));
        let (banner, _) = banner(profile, ink, radius, cx);
        let decoration = user.decoration.clone().or(profile.decoration.clone());
        // An open profile's avatar and decoration move, as Discord's.
        let decoration = decoration.as_deref().map(crate::discord::animated);
        let avatar = div()
            .id("profile-avatar")
            .p(px(AVATAR_RING))
            .rounded_full()
            .bg(ink.top)
            .cursor_pointer()
            .child(decorated_avatar(&crate::discord::animated(&user.avatar), decoration.as_deref(), AVATAR, status, ink.top))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(card) = &mut this.profile_card {
                    card.avatar_full = true;
                }
                cx.notify();
            }));
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

    // Discord's popout shows one activity card: their stream, else their first activity, else the voice
    // channel they're in. More than one peeks from under it; clicking the peek lists them all in place.
    fn profile_activity(&self, card: &ProfileCard, lift: f32, ink: Ink, cx: &mut Context<Self>) -> Option<AnyElement> {
        let place = self.guilds.iter().find_map(|g| {
            let state = g.voice_states.iter().find(|v| v.user_id == card.user.id)?;
            let channel = g.channels.iter().find(|c| Some(&c.id) == state.channel_id.as_ref())?;
            Some((g, channel, state.streaming))
        });
        // Discord's order: the stream, each activity, then voice when not streaming.
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
        if cards.len() <= 1 {
            return cards.pop();
        }
        if card.all_activity {
            let less = div()
                .id("profile-activity-less")
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(ink.muted)
                .cursor_pointer()
                .hover(move |s| s.text_color(ink.strong))
                .child("Show less")
                .on_click(cx.listener(|this, _, _, cx| this.show_all_activity(false, cx)));
            return Some(div().flex().flex_col().items_start().gap(px(8.)).children(cards.into_iter().map(|c| div().w_full().child(c))).child(less).into_any_element());
        }
        let first = cards.into_iter().next()?;
        Some(self.activity_peek(first, lift, ink, cx).into_any_element())
    }

    // The first card raised over a sliver of the next, which unfolds the rest ("View More").
    fn activity_peek(&self, first: AnyElement, lift: f32, ink: Ink, cx: &mut Context<Self>) -> Stateful<Div> {
        // The card's own tile is translucent: an opaque body under it keeps its shadow from showing through.
        let body = ui::mix(ink.top, ink.bottom, 0.5);
        let first = div().relative().top(px(lift)).squircle(RADIUS_MD).bg(body).shadow(vec![ui::shadow(gpui_kit::black().opacity(0.25), 0., 1., 3., 0., false)]).child(first);
        let sliver = ui::edge(div().absolute().left(px(PEEK_INSET)).right(px(PEEK_INSET)).bottom(px(lift)).h(px(PEEK - 2. * lift)).bg(ink.tile), ui::bottom(RADIUS_SM))
            .id("profile-activity-view-more")
            .cursor_pointer()
            .tooltip(|window, cx| Tooltip::new("View More").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.show_all_activity(true, cx)));
        div()
            .id("profile-activity-stack")
            .relative()
            .pb(px(PEEK))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(PEEK_HOVER, *hovered, cx)))
            .child(sliver)
            .child(first)
    }

    fn show_all_activity(&mut self, all: bool, cx: &mut Context<Self>) {
        if let Some(card) = &mut self.profile_card {
            card.all_activity = all;
        }
        cx.notify();
    }

    // Discord's rich presence card: heading and menu, the large image, then name, details, state with
    // the party size, the elapsed time, and Ask to Join when the party is open.

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

// The full avatar over the window at Discord's size: 70% of the window's short side, at most 1024.
// A click outside it or Esc closes it, back to the popout.
fn avatar_viewer(user: &User, window: &Window, cx: &mut Context<Waffle>) -> AnyElement {
    let viewport = window.viewport_size();
    let side = (0.7 * f32::from(viewport.width).min(f32::from(viewport.height))).round().min(1024.);
    let picture = crate::chat::animated_image(full_avatar(&user.avatar)).size(px(side)).squircle(RADIUS_MD).object_fit(gpui_kit::ObjectFit::Cover);
    div()
        .id("profile-avatar-full")
        .occlude()
        .absolute()
        .inset_0()
        .bg(gpui_kit::black().opacity(0.88))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.dismiss_overlay(cx);
            }),
        )
        .child(div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(picture))
        .with_animation("profile-avatar-full", gpui_kit::Animation::new(Duration::from_millis(160)), |el, t| el.opacity(t))
        .into_any_element()
}

fn full_avatar(url: &str) -> String {
    crate::discord::animated(url).replacen("size=128", "size=1024", 1)
}

// The effect's layers over the whole card, scaled so its widest reaches the card's edges. Drawn without a
// hitbox, so the card under it keeps the pointer.
fn effect_overlay(layers: &[EffectLayer]) -> Option<Div> {
    let span = layers.iter().map(|l| l.x + l.width).fold(0., f32::max);
    if span <= 0. {
        return None;
    }
    let k = WIDTH / span;
    let images = layers.iter().enumerate().map(|(i, l)| img(l.src.clone()).id(("profile-effect", i)).absolute().left(px(l.x * k)).top(px(l.y * k)).w(px(l.width * k)).h(px(l.height * k)));
    Some(div().absolute().inset_0().overflow_hidden().children(images))
}

// Discord's 200ms ease-in-out for the peek's lift.
fn peek_ease() -> Transition {
    Transition::Tween(Duration::from_millis(200), Easing::Bezier(Bezier::new(0.42, 0., 0.58, 1.).expect("monotonic bezier")))
}

fn section(title: &'static str, content: impl IntoElement, ink: Ink) -> Div {
    div().flex().flex_col().gap(px(6.)).child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(ink.strong).child(title)).child(content)
}

fn since_date(id: &'static str, mark: impl IntoElement, tip: SharedString, at: &chrono::DateTime<chrono::Utc>, ink: Ink) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(14.))
        .text_color(ink.text)
        .child(mark)
        .child(short_date(at))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}

// When a snowflake was minted: its top 42 bits are milliseconds since Discord's epoch (2015).
fn snowflake_time(id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    const DISCORD_EPOCH: i64 = 1_420_070_400_000;
    chrono::DateTime::from_timestamp_millis((id.parse::<u64>().ok()? >> 22) as i64 + DISCORD_EPOCH)
}

fn parse_time(iso: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|t| t.to_utc())
}

// Discord's `toLocaleDateString(…, {month: "short", day: "numeric", year: "numeric"})` in English, on the
// local calendar: "Mar 4, 2021".
fn short_date(at: &chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string()
}

fn connections(list: &[Connection], ink: Ink, cx: &App) -> Option<Div> {
    if list.is_empty() {
        return None;
    }
    let chips = list.iter().enumerate().map(|(i, c)| connection_chip(i, c, ink, cx));
    Some(section("Connections", div().flex().flex_wrap().gap(px(6.)).children(chips), ink))
}

// The service's mark on its colour, the account name, Discord's verified seal, and a click to the account.
fn connection_chip(i: usize, c: &Connection, ink: Ink, cx: &App) -> Stateful<Div> {
    let (platform, color) = platform(&c.kind);
    let url = connection_url(c);
    let tip: SharedString = match url {
        Some(_) => format!("Open on {platform}").into(),
        None => platform.into(),
    };
    let name = if c.name.is_empty() { platform.to_owned() } else { c.name.clone() };
    // The seal takes the info ink on the plain card; on a themed one, the card's own ink reads better.
    let seal = if ink.border.is_some() { ink.strong } else { palette::info_ink(cx) };
    let hover = ink.strong.opacity(0.16);
    div()
        .id(("profile-connection", i))
        .h(px(28.))
        .max_w_full()
        .pl(px(4.))
        .pr(px(8.))
        .flex()
        .items_center()
        .gap(px(6.))
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink.strong)
        .child(brand_tile(&c.kind, platform, color))
        .child(div().min_w_0().truncate().child(name))
        .when(c.verified, |d| d.child(Icon::new(Glyph::CheckCircle).size(px(14.)).flex_none().text_color(seal)))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .when_some(url, |d, url| d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |_, _, cx| cx.open_url(&url)))
}

// The mark tinted dark or light by its colour; a service without a mark gets its initial.
fn brand_tile(kind: &str, platform: &str, color: Option<u32>) -> Div {
    let fill: Hsla = color.map_or_else(|| palette::oklch(0.5, 0.1, ui::name_hue(kind)), |c| rgb(c).into());
    let ink: Hsla = if luminance(fill) > 0.5 { rgb(0x1A1A1E).into() } else { gpui_kit::white() };
    let tile = div().size(px(20.)).flex_none().squircle(RADIUS_XS).bg(fill).flex().items_center().justify_center().text_color(ink);
    match crate::icons::brand_mark(kind) {
        Some(path) => tile.child(svg().path(path).size(px(13.)).text_color(ink)),
        None => tile.text_size(px(12.)).font_weight(FontWeight::BOLD).child(platform.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()),
    }
}

// Discord's connection types: name and brand colour (Simple Icons' hex where it has the service).
// Not verified: the names of types outside the RE doc's URL table (Battle.net through Amazon Music).
fn platform(kind: &str) -> (&'static str, Option<u32>) {
    match kind {
        "twitch" => ("Twitch", Some(0x9146FF)),
        "youtube" => ("YouTube", Some(0xFF0000)),
        "bluesky" => ("Bluesky", Some(0x0285FF)),
        "steam" => ("Steam", Some(0x000000)),
        "reddit" => ("Reddit", Some(0xFF4500)),
        "twitter" => ("X", Some(0x000000)),
        "twitter_legacy" => ("Twitter", Some(0x000000)),
        "spotify" => ("Spotify", Some(0x1ED760)),
        "github" => ("GitHub", Some(0x181717)),
        "roblox" => ("Roblox", Some(0x000000)),
        "tiktok" => ("TikTok", Some(0x000000)),
        "instagram" => ("Instagram", Some(0xFF0069)),
        "battlenet" => ("Battle.net", Some(0x4381C3)),
        "playstation" | "playstation-stg" => ("PlayStation Network", Some(0x0070D1)),
        "epicgames" => ("Epic Games", Some(0x313131)),
        "riotgames" => ("Riot Games", Some(0xEB0029)),
        "crunchyroll" => ("Crunchyroll", Some(0xF47521)),
        "xbox" => ("Xbox", Some(0x107C10)),
        "ebay" => ("eBay", None),
        "letterboxd" => ("Letterboxd", None),
        "mastodon" => ("Mastodon", None),
        "domain" => ("Domain", None),
        "skype" => ("Skype", None),
        "bungie" => ("Bungie.net", None),
        "leagueoflegends" => ("League of Legends", None),
        "facebook" => ("Facebook", None),
        "samsung" => ("Samsung Galaxy", None),
        "paypal" => ("PayPal", None),
        "amazon-music" => ("Amazon Music", None),
        _ => ("Account", None),
    }
}

// Discord's `getPlatformUserUrl`; None for the services it shows as plain text.
// GAP: Discord warns before opening a `domain` link (it's untrusted); this opens it directly.
fn connection_url(c: &Connection) -> Option<String> {
    let (id, name) = (encode_component(&c.id), encode_component(&c.name));
    Some(match c.kind.as_str() {
        "twitch" => format!("https://www.twitch.tv/{name}"),
        "youtube" => format!("https://www.youtube.com/channel/{id}"),
        "bluesky" => format!("https://bsky.app/profile/{}", id.replace("%3A", ":")),
        "steam" => format!("https://steamcommunity.com/profiles/{id}"),
        "reddit" => format!("https://www.reddit.com/u/{name}"),
        "twitter" => format!("https://x.com/{name}"),
        "twitter_legacy" => format!("https://twitter.com/{name}"),
        "spotify" => format!("https://open.spotify.com/user/{id}"),
        "github" => format!("https://github.com/{name}"),
        "roblox" => format!("https://roblox.com/users/{id}/profile"),
        "ebay" => format!("https://www.ebay.com/usr/{name}"),
        "tiktok" => format!("https://www.tiktok.com/@{name}"),
        "instagram" => format!("https://www.instagram.com/{name}"),
        "letterboxd" => format!("https://letterboxd.com/{name}/"),
        "mastodon" if c.id.starts_with("https://") => c.id.clone(),
        "domain" => format!("https://{id}/"),
        _ => return None,
    })
}

// JavaScript's encodeURIComponent: everything but letters, digits and -_.!~*'() is percent-encoded.
fn encode_component(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snowflake_dates() {
        // Discord's own example id, documented as minted 2016-04-30 11:18:25.796 UTC.
        assert_eq!(snowflake_time("175928847299117063").map(|t| t.timestamp_millis()), Some(1_462_015_105_796));
        assert_eq!(snowflake_time("not a number"), None);
        let at = parse_time("2021-03-04T12:00:00+00:00").expect("valid time");
        assert_eq!(at.format("%b %-d, %Y").to_string(), "Mar 4, 2021");
    }

    #[test]
    fn connection_urls() {
        let c = |kind: &str, id: &str, name: &str| Connection { kind: kind.into(), id: id.into(), name: name.into(), verified: false };
        assert_eq!(connection_url(&c("github", "1", "wren lark")).as_deref(), Some("https://github.com/wren%20lark"));
        assert_eq!(connection_url(&c("bluesky", "did:plc:abc", "wren.bsky.social")).as_deref(), Some("https://bsky.app/profile/did:plc:abc"));
        assert_eq!(connection_url(&c("spotify", "31x", "Wren")).as_deref(), Some("https://open.spotify.com/user/31x"));
        assert_eq!(connection_url(&c("mastodon", "javascript:alert(1)", "w")), None);
        assert_eq!(connection_url(&c("xbox", "1", "wren")), None);
        assert_eq!(encode_component("a/b?c=d&e"), "a%2Fb%3Fc%3Dd%26e");
    }

    #[test]
    fn effect_fills_the_card_width() {
        let layer = |x: f32, width: f32| EffectLayer { src: "a".into(), x, y: 0., width, height: 10., z: 0 };
        assert!(effect_overlay(&[]).is_none());
        assert!(effect_overlay(&[layer(0., 0.)]).is_none());
        assert!(effect_overlay(&[layer(0., 450.), layer(100., 50.)]).is_some());
    }
}
