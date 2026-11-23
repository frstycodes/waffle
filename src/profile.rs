// The profile popout, opened from a name or avatar: Discord's layout in the user's profile theme.
use std::time::Duration;

use gpui_kit::{
    AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, MouseButton, Pixels, Point, SharedString, Stateful, StyledText, Window,
    component::{ActiveTheme, Icon, text::TextView},
    div, img, linear_color_stop, linear_gradient, prelude::*, px, relative, svg,
};

use crate::{
    ProfileCard, Waffle,
    discord::{ActivityKind, Badge, Connection, Profile, Relation, RichPresence, User, perm},
    icons::Glyph,
    layout::{TITLE_HEIGHT, decorated_avatar, playing_avatar},
    palette,
    ui::{self, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_XS, Squircle},
    user_menu::Surface,
};

mod dm_panel;
mod media;
pub(crate) use media::banner;
use media::{effect_overlay, full_avatar};

pub(crate) const WIDTH: f32 = 320.;
const BORDER: f32 = 2.;
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
        let card = self.profile_card.as_ref()?;
        // Animated pictures move only under the pointer, as Discord's.
        let (hovered, on_hover) = ui::hover_state(SharedString::from(format!("profile-hover-{}", card.user.id)), window, cx);
        let viewport = window.viewport_size();
        let profile = card.profile.clone().unwrap_or_default();
        let ink = Ink::new(profile.theme, cx);
        let x = f32::from(card.at.x).clamp(8., f32::from(viewport.width) - WIDTH - 8.);
        let top = TITLE_HEIGHT + 8.;
        let y = f32::from(card.at.y).clamp(top, (f32::from(viewport.height) - ESTIMATED_HEIGHT - 8.).max(top));
        let max_height = f32::from(viewport.height) - top - 8.;
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let body = self.profile_sections(card, &profile, ink, cx).id("profile-body").min_h_0().overflow_y_scroll();
        let (outer, inner) = themed_card(ink, max_height, cx);
        let inner = inner
            .child(self.profile_header(card, &profile, inner_radius(ink), hovered, ink, cx))
            .child(body)
            .when(!is_me, |c| c.child(message_field(&card.user, ink, cx)));
        let effect = profile.effect_sku.as_ref().and_then(|sku| self.profile_effects.get(sku)).and_then(|layers| layers.as_deref()).and_then(effect_overlay);
        let card_el = outer
            .id("profile-card")
            .occlude()
            .relative()
            .w(px(WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(on_hover)
            .child(inner)
            .children(effect);
        // It rises from the corner nearest where it was asked for: pushed in from an edge, from that side.
        let origin = ui::corner(x < f32::from(card.at.x), y < f32::from(card.at.y));
        let rise = ui::rise(card_el.transform_origin(origin), SharedString::from(format!("profile-{}", card.user.id)), Duration::ZERO);
        // Measured, not estimated: a short card opened low sits just above the window's foot, not 560px up.
        // lint-allow(raw-floating): placed inside the overlay backdrop, with a measured top margin floating_at can't take.
        let placed = gpui_kit::anchored().position(card.at).snap_to_window_with_margin(gpui_kit::Edges { top: px(top), right: px(8.), bottom: px(8.), left: px(8.) }).child(rise);
        let avatar = card.avatar_full.then(|| avatar_viewer(&card.user, window, cx));
        Some(crate::overlays::backdrop("profile-backdrop", cx).child(placed).children(avatar).into_any_element())
    }

    // The card's sections under its header: name, then whatever the profile has.
    fn profile_sections(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> Div {
        div()
            .px_4()
            .pb_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(name_block(&card.user, profile, ink))
            // Not fetched yet: where the badges, mutuals and bio will be.
            .when(card.profile.is_none(), |b| b.child(profile_skeleton(cx)))
            .children(badges(&profile.badges, ink))
            .children(mutuals(profile, ink))
            .children(profile.bio.as_ref().map(|bio| bio_view(&card.user.id, bio, ink)))
            .children(self.member_since(card, profile, ink, cx))
            .children(self.friends_since(card, ink))
            .children(connections(&profile.connections, ink, cx))
            .children(game_collection(&profile.games, ink))
            .children(self.profile_activity(card, ink, cx))
            .children(self.roles_row(card, profile, ink, cx))
    }

    // Discord's date from the account's id, then, opened in a server, the server's from when they joined.
    fn member_since(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &App) -> Option<Div> {
        card.profile.as_ref()?;
        let created = snowflake_time(&card.user.id)?;
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let joined = guild.zip(profile.joined.as_deref().and_then(parse_time));
        let discord_mark = div()
            .size_5()
            .flex_none()
            .squircle(RADIUS_XS)
            .bg(palette::accent())
            .flex()
            .items_center()
            .justify_center()
            .children(crate::icons::brand_mark("discord").map(|path| svg().path(path).size_3p5().text_color(palette::on_accent())));
        let row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2p5()
            .child(since_date("profile-since-discord", discord_mark, "Discord".into(), &created, ink))
            .when_some(joined, |row, (guild, joined)| {
                let icon = ui::server_tile(guild.icon.as_deref(), &guild.name, 20., cx);
                row.child(div().size_1().rounded_full().bg(ink.muted)).child(since_date("profile-since-server", icon, guild.name.clone().into(), &joined, ink))
            });
        Some(section(if card.user.bot { "Created On" } else { "Member Since" }, row, ink))
    }

    fn friends_since(&self, card: &ProfileCard, ink: Ink) -> Option<Div> {
        let friend = self.relationships.iter().find(|r| r.user.id == card.user.id && r.kind == Relation::Friend)?;
        let since = parse_time(friend.since.as_deref()?)?;
        Some(section("Friends Since", div().text_size(ui::TEXT_SM).text_color(ink.text).child(short_date(&since)), ink))
    }

    // Banner with its round actions, then the avatar overlapping its foot.
    // `radius` rounds the banner's top corners to what holds it: the card's inner radius, 0 flush in a panel.
    fn profile_header(&self, card: &ProfileCard, profile: &Profile, radius: f32, hovered: bool, ink: Ink, cx: &mut Context<Self>) -> Div {
        let user = &card.user;
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user.id);
        let status = self.presences.get(&user.id).map(|p| p.mark()).or(self.my_mark().filter(|_| is_me).map(Into::into));
        let (banner, _) = banner(profile, ink, radius, hovered, cx);
        let decoration = user.decoration.clone().or(profile.decoration.clone());
        // Still until the card is hovered, as Discord's.
        let face = match hovered {
            true => playing_avatar(&format!("profile-{}", user.id), &user.avatar, decoration.as_deref(), AVATAR, status, ink.top),
            false => decorated_avatar(&user.avatar, decoration.as_deref(), AVATAR, status, ink.top),
        };
        let avatar = div()
            .id("profile-avatar")
            .p(px(AVATAR_RING))
            .rounded_full()
            .bg(ink.top)
            .cursor_pointer()
            .child(face)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(card) = &mut this.profile_card {
                    card.avatar_full = true;
                }
                cx.notify();
            }));
        let edit = is_me.then(|| {
            ui::button("profile-edit", ui::Tone::Secondary, cx).small().child(Icon::new(Glyph::Pen).size(ui::ICON_MD)).child("Edit profile").on_click(cx.listener(|this, _, window, cx| {
                this.profile_card = None;
                this.open_settings(crate::settings::Page::Profiles, window, cx);
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
        let mod_view = card.guild_id.clone().filter(|g| self.can_mod_view(g)).map(|g| {
            let id = user.id.clone();
            banner_button("profile-mod-view", Glyph::Shield, "Open in Mod View").cursor_pointer().on_click(cx.listener(move |this, _, window, cx| this.open_mod_view(g.clone(), id.clone(), window, cx)))
        });
        let guild_id = card.guild_id.clone();
        let more = banner_button("profile-more", Glyph::More, "More")
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), menu_surface(guild_id.is_some()), event.position(), cx)));
        div().absolute().top_3().right_3().flex().gap_2().children(mod_view).children(friend).child(more)
    }

    // Discord's popout shows one activity card: their stream, else their first activity, else the voice
    // channel they're in. More than one peeks from under it; clicking the peek lists them all in place.
    fn profile_activity(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Option<AnyElement> {
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
                .text_size(ui::TEXT_SM)
                .font_weight(FontWeight::MEDIUM)
                .text_color(ink.muted)
                .cursor_pointer()
                .hover(move |s| s.text_color(ink.strong))
                .child("Show less")
                .on_click(cx.listener(|this, _, _, cx| this.show_all_activity(false, cx)));
            return Some(div().flex().flex_col().items_start().gap_2().children(cards.into_iter().map(|c| div().w_full().child(c))).child(less).into_any_element());
        }
        let more = cards.len() - 1;
        let first = cards.into_iter().next()?;
        Some(self.activity_more(first, more, ink, cx).into_any_element())
    }

    // The first card, then a button under it that unfolds the rest in place.
    fn activity_more(&self, first: AnyElement, more: usize, ink: Ink, cx: &mut Context<Self>) -> Div {
        let label = if more == 1 { "Show 1 more activity".to_owned() } else { format!("Show {more} more activities") };
        let button = div()
            .id("profile-activity-view-more")
            .h_7()
            .flex()
            .items_center()
            .justify_center()
            .squircle(RADIUS_SM)
            .bg(ink.tile)
            .text_size(ui::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(ink.muted)
            .cursor_pointer()
            .hover(move |s| s.text_color(ink.strong))
            .child(label)
            .on_click(cx.listener(|this, _, _, cx| this.show_all_activity(true, cx)));
        div().flex().flex_col().gap_1p5().child(first).child(button)
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
            .gap_2()
            .min_w_0()
            .child(div().truncate().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(heading))
            .child(ui::live_badge());
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_2p5()
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
            .gap_2p5()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("In voice"))
            .child(self.voice_place_row(guild, channel, ink))
            .children((!is_me && !connected).then(|| self.join_voice_button(guild, channel, cx)))
            .into_any_element()
    }

    // The server › channel breadcrumb, and who's in it.
    fn voice_place_row(&self, guild: &crate::discord::Guild, channel: &crate::discord::Channel, ink: Ink) -> Div {
        let people: Vec<ui::Entry> = guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .map(|v| match &v.user {
                Some(u) => ui::Entry::person(u.avatar.clone(), u.name.clone()),
                None => ui::Entry::person(crate::discord::avatar_url(&v.user_id, None), v.user_id.clone()),
            })
            .collect();
        let server = match &guild.icon {
            Some(icon) => ui::media(icon.clone(), ui::all(RADIUS_XS)).size_5().flex_none().into_any_element(),
            None => div().size_5().flex_none().squircle(RADIUS_XS).bg(ink.top).into_any_element(),
        };
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .text_size(ui::TEXT_SM)
            .text_color(ink.text)
            .child(server)
            .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_SM).flex_none().text_color(ink.muted))
            .child(crate::sidebar::channel_glyph(channel).size_4().flex_none().text_color(ink.muted))
            .child(div().flex_1().min_w_0().truncate().child(channel.name.clone()))
            .child(ui::listed_stack(SharedString::from(format!("profile-voice-{}", channel.id)), "In voice", people, 3, 20., ink.tile, ink.top, ink.strong))
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
            .size_6()
            .squircle(RADIUS_XS)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(ink.muted)
            .hover(move |s| s.bg(ink.tile).text_color(ink.strong))
            .child(Icon::new(Glyph::More).size(ui::ICON_MD))
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.open_user_menu(user.clone(), guild_id.clone(), menu_surface(guild_id.is_some()), event.position(), cx)));
        let art = match &rich.image {
            Some(url) => ui::media(url.clone(), ui::all(RADIUS_SM)).size(px(72.)).flex_none().object_fit(gpui_kit::ObjectFit::Cover).into_any_element(),
            None => div().size(px(72.)).flex_none().squircle(RADIUS_SM).bg(ink.tile).flex().items_center().justify_center().text_color(ink.muted).child(Icon::new(Glyph::Game).size_8()).into_any_element(),
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
            .gap_0p5()
            .text_size(ui::TEXT_SM)
            .child(div().truncate().text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(ink.strong).child(rich.name.clone()))
            .children(rich.details.clone().map(line))
            .children(state.map(|s| div().flex().items_center().gap_1().min_w_0().text_color(ink.text).when(rich.party.is_some(), |d| d.child(Icon::new(Glyph::Users).size(ui::ICON_SM).flex_none())).child(div().truncate().child(s))))
            .children(elapsed.map(|time| {
                // Status green is ~2:1 on a light card; a deeper green keeps it readable there.
                let green = if luminance(ink.strong) < 0.5 { palette::online_on_light() } else { palette::status(crate::discord::Status::Online) };
                div().flex().items_center().gap_1().text_color(green).child(Icon::new(Glyph::Game).size(ui::ICON_SM)).child(div().font_family(mono).child(time))
            }));
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_2p5()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().flex().items_center().justify_between().child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child(activity_heading(rich.kind))).child(more))
            .child(div().flex().items_center().gap_3().min_w_0().child(art).child(facts))
            .when(rich.joinable, |c| c.child(ask_to_join(ink))).children(self.activity_join_button(&card.user.id, rich, cx))
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
            let color = if r.color == 0 { palette::status_in(crate::discord::Status::Offline, cx) } else { palette::data_color(r.color) };
            ui::role_chip(r.name.clone(), color, cx).bg(ink.tile).text_color(ink.text)
        });
        let (user, guild_id) = (card.user.clone(), guild.id.clone());
        let add = can_manage.then(|| {
            div()
                .id("profile-add-role")
                .h_6()
                .px_1p5()
                .flex()
                .items_center()
                .gap_1()
                .squircle(RADIUS_XS)
                .cursor_pointer()
                .text_size(ui::TEXT_SM)
                .text_color(ink.muted)
                .hover(move |s| s.bg(ink.tile).text_color(ink.strong))
                .child(Icon::new(Glyph::Plus).size(ui::ICON_SM))
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
        let (from, to): (Hsla, Hsla) = (palette::data_color(from), palette::data_color(to));
        let dark = cx.theme().is_dark();
        let ground: Hsla = if dark { gpui_kit::black() } else { gpui_kit::white() };
        let (top, bottom) = (ui::mix(from, ground, 0.55), ui::mix(to, ground, 0.55));
        let strong: Hsla = if luminance(ui::mix(top, bottom, 0.5)) > 0.4 { palette::ink_on_light() } else { gpui_kit::white() };
        Ink { top, bottom, border: Some((from, to)), strong, text: strong.opacity(0.92), muted: strong.opacity(0.72), tile: strong.opacity(0.1) }
    }
}

fn luminance(color: Hsla) -> f32 {
    let c = gpui_kit::Rgba::from(color);
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
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

fn banner_button(id: &'static str, glyph: Glyph, label: &'static str) -> Stateful<Div> {
    let fill = gpui_kit::black().opacity(0.55);
    div()
        .id(id)
        .size_8()
        .rounded_full()
        .bg(fill)
        .hover(|s| s.bg(gpui_kit::black().opacity(0.75)))
        .flex()
        .items_center()
        .justify_center()
        .text_color(gpui_kit::white())
        .child(Icon::new(glyph).size(ui::ICON_MD))
        .tooltip(move |window, cx| ui::tip(label, window, cx))
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
            .px_1p5()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .squircle(RADIUS_XS)
            .bg(ink.tile)
            .text_color(ink.strong)
            .text_size(ui::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .children(tag.badge.clone().map(|badge| img(badge).size_3p5()))
            .child(tag.text.clone())
    });
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(crate::name_fonts::styled(div().truncate().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD), user.name_style.as_ref()).text_color(ink.strong).child(StyledText::new(user.name.clone()).with_highlights(name)))
        .when(!handle.is_empty() || tag.is_some(), |b| {
            b.child(div().flex().flex_wrap().items_center().gap_x_1p5().gap_y_1().text_size(ui::TEXT_SM).text_color(ink.text).when(!handle.is_empty(), |l| l.child(div().min_w_0().truncate().child(handle))).children(tag))
        })
}

pub(crate) fn badges(badges: &[Badge], ink: Ink) -> Option<Div> {
    if badges.is_empty() {
        return None;
    }
    let shown = badges.iter().take(MAX_BADGES).enumerate().map(|(i, badge)| {
        let label = SharedString::from(badge.description.clone());
        div().id(("profile-badge", i)).size(px(22.)).child(img(badge.icon.clone()).size_full()).tooltip(move |window, cx| ui::tip(label.clone(), window, cx))
    });
    let more = badges.len().saturating_sub(MAX_BADGES);
    let pill = div()
        .id("profile-badges")
        .h(px(30.))
        .px_1p5()
        .flex()
        .items_center()
        .gap_1()
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .children(shown)
        .when(more > 0, |p| p.child(div().pl_0p5().text_size(ui::TEXT_SM).text_color(ink.text).child(format!("+{more}"))));
    let pill = match more > 0 {
        true => ui::overflow_list(pill, "Badges", badges.iter().map(|b| ui::Entry::item(b.icon.clone(), b.description.clone())).collect()),
        false => pill,
    };
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
    Some(div().flex().items_center().gap_2().text_size(ui::TEXT_SM).text_color(ink.muted).child(ui::avatar_stack(faces, 20., ink.top)).child(div().truncate().child(parts.join(" • "))))
}

// Discord markdown with custom emoji as images, as a message renders it; Discord gives the bio no heading.
pub(crate) fn bio_view(user_id: &str, bio: &str, ink: Ink) -> Div {
    let view = TextView::markdown(SharedString::from(format!("bio-{user_id}")), crate::chat::inline_emoji(&crate::chat::discord_breaks(bio))).selectable(true);
    div().text_size(ui::TEXT_SM).text_color(ink.text).max_h_40().child(view)
}

fn game_collection(games: &[String], ink: Ink) -> Option<Div> {
    if games.is_empty() {
        return None;
    }
    let more = games.len().saturating_sub(MAX_GAMES);
    let tile = |el: Div| el.size_8().flex_none().squircle(RADIUS_XS);
    let icons = games.iter().take(MAX_GAMES).map(|url| ui::media(url.clone(), ui::all(RADIUS_XS)).size_8().flex_none().object_fit(gpui_kit::ObjectFit::Cover));
    let overflow = (more > 0).then(|| {
        tile(div()).bg(gpui_kit::black().opacity(0.6)).flex().items_center().justify_center().text_size(ui::TEXT_SM).text_color(gpui_kit::white()).child(format!("+{more}"))
    });
    Some(
        div()
            .px_3()
            .py_2p5()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .squircle(RADIUS_MD)
            .bg(ink.tile)
            .child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::MEDIUM).text_color(ink.strong).child("Game Collection"))
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
        .h_9()
        .flex()
        .items_center()
        .justify_center()
        .gap_2()
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .text_color(ink.muted)
        .text_size(ui::TEXT_SM)
        .opacity(0.7)
        .child(Icon::new(Glyph::Game).size(ui::ICON_MD))
        .child("Ask to Join")
        .tooltip(|window, cx| ui::tip("Joining from Waffle isn't supported yet", window, cx))
}

fn message_field(user: &User, ink: Ink, cx: &mut Context<Waffle>) -> Div {
    let dm_user = user.id.clone();
    let field = div()
        .id("profile-message")
        .h_11()
        .px_3p5()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .squircle(RADIUS_MD)
        .bg(ink.tile)
        .cursor_text()
        .child(div().truncate().text_size(ui::TEXT_SM).text_color(ink.muted).child(format!("Message @{}", user.name)))
        .child(Icon::new(Glyph::Emoji).size(ui::ICON_MD).flex_none().text_color(ink.muted))
        .on_click(cx.listener(move |this, _, window, cx| this.message_user(dm_user.clone(), window, cx)));
    div().flex_none().px_4().pt_1().pb_4().child(field)
}

// A profile differs per server (nickname, roles, server avatar), so the server is part of its key.
pub(crate) fn profile_key(user_id: &str, guild_id: Option<&str>) -> String {
    format!("{user_id}-{}", guild_id.unwrap_or("dm"))
}

fn profile_skeleton(cx: &App) -> Div {
    let line = |id: &'static str, width: f32| ui::skeleton(id, 4., cx).h_3().w(relative(width));
    div()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(ui::skeleton("profile-skel-badges", 8., cx).h_7().w(px(120.)))
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


fn section(title: &'static str, content: impl IntoElement, ink: Ink) -> Div {
    div().flex().flex_col().gap_1p5().child(div().text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(ink.strong).child(title)).child(content)
}

fn since_date(id: &'static str, mark: impl IntoElement, tip: SharedString, at: &chrono::DateTime<chrono::Utc>, ink: Ink) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(ui::TEXT_SM)
        .text_color(ink.text)
        .child(mark)
        .child(short_date(at))
        .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
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
    Some(section("Connections", div().flex().flex_wrap().gap_1p5().children(chips), ink))
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
        .h_7()
        .max_w_full()
        .pl_1()
        .pr_2()
        .flex()
        .items_center()
        .gap_1p5()
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink.strong)
        .child(brand_tile(&c.kind, platform, color))
        .child(div().min_w_0().truncate().child(name))
        .when(c.verified, |d| d.child(Icon::new(Glyph::CheckCircle).size(ui::ICON_SM).flex_none().text_color(seal)))
        .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
        .when_some(url, |d, url| d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |_, _, cx| cx.open_url(&url)))
}

// The mark tinted dark or light by its colour; a service without a mark gets its initial.
fn brand_tile(kind: &str, platform: &str, color: Option<u32>) -> Div {
    let fill: Hsla = color.map_or_else(|| palette::oklch(0.5, 0.1, ui::name_hue(kind)), palette::data_color);
    let ink: Hsla = if luminance(fill) > 0.5 { palette::ink_on_light() } else { gpui_kit::white() };
    let tile = div().size_5().flex_none().squircle(RADIUS_XS).bg(fill).flex().items_center().justify_center().text_color(ink);
    match crate::icons::brand_mark(kind) {
        Some(path) => tile.child(svg().path(path).size(px(13.)).text_color(ink)),
        None => tile.text_size(ui::TEXT_XS).font_weight(FontWeight::BOLD).child(platform.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()),
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
}
