// The profile popout, opened from a name or avatar: Discord's layout in the user's profile theme. The same
// card state opens the full profile dialog (`full.rs`) when its avatar, name or mutuals are clicked.
use std::time::Duration;

use gpui_kit::{
    AnyElement, App, Context, Div, Entity, MouseButton, Pixels, Point, ScrollHandle, SharedString, Window,
    component::{
        Icon,
        input::{InputState, TextareaState},
    },
    div, prelude::*, px,
};

use crate::{
    Waffle,
    discord::{Profile, Relation, User},
    icons::Glyph,
    layout::{TITLE_HEIGHT, decorated_avatar, playing_avatar},
    palette,
    ui::{self, RADIUS_XS, Squircle},
    settings::ProfileTab,
};

mod actions;
mod activity;
mod connections;
mod dm_panel;
mod full;
mod media;
mod parts;
mod popovers;
mod social;
use activity::game_collection;
use connections::*;
pub(crate) use media::{BANNER, banner};
use media::BANNER_FULL;
use media::{effect_overlay, frame_layers};
pub(crate) use parts::{Ink, bio_view, inner_radius, name_block, profile_key, themed_card};
use parts::*;
pub(crate) use popovers::{CardPop, Source};

pub(crate) const WIDTH: f32 = 300.;
const BORDER: f32 = 2.;
pub(crate) const AVATAR: f32 = 80.;
pub(crate) const AVATAR_RING: f32 = 6.;
// Only picks which corner the card rises from; `anchored` places it by its measured size.
const ESTIMATED_HEIGHT: f32 = 560.;
const MAX_GAMES: usize = 3;
// The card's side inset.
const CARD_INSET: f32 = 16.;
// An activity's art, a brand mark's glyph inside its tile, and the skeleton that stands for the badges.
const ACTIVITY_ART: f32 = 72.;
const BRAND_GLYPH: f32 = 13.;
const SKELETON_BADGES: f32 = 120.;

// A profile opened from an avatar or name: the popout at the click point, or the full profile dialog.
pub(crate) struct ProfileCard {
    pub(crate) user: User,
    pub(crate) guild_id: Option<String>,
    pub(crate) at: Point<Pixels>,
    pub(crate) profile: Option<Profile>,
    // Every activity card, after the peeking card under the first was clicked.
    pub(crate) all_activity: bool,
    // Their avatar full size over the window.
    pub(crate) avatar_full: bool,
    // The full profile dialog on this tab, in place of the popout.
    pub(crate) full: Option<FullTab>,
    // The note while it's being edited; saved when it loses focus.
    pub(crate) note: Option<Entity<TextareaState>>,
    // The popout's message field, once clicked into.
    pub(crate) message: Option<Entity<InputState>>,
    // Every role, after the popout's "+N" was clicked.
    pub(crate) all_roles: bool,
    // The popover open over the card, what an open emoji picker reacts to, and a passing toast.
    pub(crate) pop: Option<CardPop>,
    pub(crate) reacting: Option<Source>,
    pub(crate) toast: Option<&'static str>,
    // The full profile's Recent activity once fetched.
    pub(crate) recent: Option<Vec<crate::discord::RecentEntry>>,
    // A long status shown whole, after its chevron.
    pub(crate) status_expanded: bool,
    // The DM side panel's own card: it holds the panel's popovers and state, and draws no popout.
    pub(crate) panel: bool,
    // The full profile's card body and tab list.
    pub(crate) card_scroll: ScrollHandle,
    pub(crate) tab_scroll: ScrollHandle,
}

// The full profile's tabs, in Discord's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FullTab {
    Activity,
    MutualFriends,
    MutualServers,
}

impl ProfileCard {
    fn new(user: User, guild_id: Option<String>, at: Point<Pixels>, profile: Option<Profile>, full: Option<FullTab>) -> Self {
        Self { user, guild_id, at, profile, all_activity: false, avatar_full: false, full, note: None, message: None, all_roles: false, pop: None, reacting: None, toast: None, recent: None, status_expanded: false, panel: false, card_scroll: ScrollHandle::new(), tab_scroll: ScrollHandle::new() }
    }
}

impl Waffle {
    pub(crate) fn open_profile(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_profile(user, at, None, cx);
    }

    // The full profile dialog, as Discord opens it from a context menu, a DM's header or the side panel.
    pub(crate) fn open_full_profile(&mut self, user: User, tab: FullTab, cx: &mut Context<Self>) {
        self.show_profile(user, Point::default(), Some(tab), cx);
    }

    fn show_profile(&mut self, user: User, at: Point<Pixels>, full: Option<FullTab>, cx: &mut Context<Self>) {
        let guild_id = self.guild_id.clone();
        // The last profile we fetched shows at once; a stale one refreshes underneath it.
        let key = profile_key(&user.id, guild_id.as_deref());
        let cached = self.profiles.cached(&key);
        let effect = cached.as_ref().and_then(|p| p.effect_sku.clone());
        self.profile_card = Some(ProfileCard::new(user.clone(), guild_id.clone(), at, cached, full));
        self.fetch_profile_effect(effect, cx);
        self.redraw_overlays(cx);
        if self.presences.get(&user.id).and_then(|p| p.rich.as_ref()).is_some_and(|r| r.started.is_some()) {
            self.tick_profile_timer(user.id.clone(), cx);
        }
        if full.is_some() {
            self.fetch_recent_activity(&user.id, cx);
        }
        self.fetch_profile(user, guild_id, cx);
    }

    // Closes an open popout or full profile; the DM panel's own card isn't open in that sense and stays.
    pub(crate) fn close_profile_card(&mut self) -> bool {
        if self.profile_card.as_ref().is_some_and(|c| c.panel) {
            return false;
        }
        self.profile_card.take().is_some()
    }

    // The full profile on `tab`: the open popout on this user grows into it, anywhere else it opens fresh.
    pub(crate) fn view_full_profile(&mut self, user: &User, tab: FullTab, cx: &mut Context<Self>) {
        let Some(card) = self.profile_card.as_mut().filter(|c| c.user.id == user.id) else { return self.open_full_profile(user.clone(), tab, cx) };
        card.full = Some(tab);
        card.avatar_full = false;
        let user_id = card.user.id.clone();
        self.fetch_recent_activity(&user_id, cx);
        self.redraw_overlays(cx);
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
                this.fetch_profile_frame(&key, &user.id, profile.frame_sku.clone(), cx);
                if let Some(card) = this.profile_card.as_mut().filter(|c| c.user.id == user.id) {
                    card.profile = Some(profile);
                }
                cx.notify(); // lint-allow(global-notify): the profile draws in the popout and in the DM panel
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the profile request returns, and holds only a WeakEntity.
        .detach();
    }

    // The frame's layers, once the profile names one; kept on the cached profile so every surface draws it.
    fn fetch_profile_frame(&mut self, key: &str, user_id: &str, sku: Option<String>, cx: &mut Context<Self>) {
        let Some(sku) = sku else { return };
        let Some(client) = self.client.clone() else { return };
        let (key, user_id) = (key.to_owned(), user_id.to_owned());
        cx.spawn(async move |this, cx| {
            let Ok(Some(frame)) = client.profile_frame(&sku).await else { return };
            this.update(cx, |this, cx| {
                if let Some(mut profile) = this.profiles.peek(&key) {
                    profile.frame = Some(frame.clone());
                    this.profiles.seed(&key, profile);
                }
                if let Some(profile) = this.profile_card.as_mut().filter(|c| c.user.id == user_id).and_then(|c| c.profile.as_mut()) {
                    profile.frame = Some(frame);
                }
                cx.notify(); // lint-allow(global-notify): the frame draws on the popout and on the DM panel's card
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the product request returns, and holds only a WeakEntity.
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
                cx.notify(); // lint-allow(global-notify): the effect draws on the popout and on the DM panel's card
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the effect request returns, and holds only a WeakEntity.
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
                            this.redraw_overlays(cx);
                        }
                        open
                    })
                    .unwrap_or(false);
                if !open {
                    break;
                }
            }
        })
        // lint-allow(detached-task): ends itself when the card closes, and holds only a WeakEntity.
        .detach();
    }

    pub(crate) fn profile_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.profile_card.as_ref()?;
        if card.full.is_some() {
            return self.full_profile_overlay(window, cx);
        }
        if card.panel {
            let pop = self.card_popover(card, window, cx)?;
            return Some(crate::overlays::backdrop("profile-backdrop", cx).child(pop).into_any_element());
        }
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
        let body = self.popout_sections(card, &profile, ink, cx).id("profile-body").min_h_0().overflow_y_scroll();
        let (outer, inner) = themed_card(ink, max_height, cx);
        let footer = match is_me {
            true => self.edit_profile_button(card.guild_id.clone(), cx).into_any_element(),
            false => self.message_field(card, ink, cx).into_any_element(),
        };
        let inner = inner.child(self.profile_header(card, &profile, Header::Popout, hovered, ink, cx)).child(body).child(footer);
        let dressing = self.dressing(&profile, WIDTH, FRAME_MIN_POPOUT, window);
        let card_el = outer
            .id("profile-card")
            .group(CARD_GROUP)
            .occlude()
            .relative()
            .w(px(WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(on_hover)
            .child(inner)
            .children(dressing.effect);
        let card_el = div().relative().w(px(WIDTH)).children(dressing.back).child(card_el).children(dressing.front);
        // It rises from the corner nearest where it was asked for: pushed in from an edge, from that side.
        let origin = ui::corner(x < f32::from(card.at.x), y < f32::from(card.at.y));
        let rise = ui::rise(card_el.transform_origin(origin), SharedString::from(format!("profile-{}", card.user.id)), Duration::ZERO);
        // Measured, not estimated: a short card opened low sits just above the window's foot, not 560px up.
        // lint-allow(raw-floating): placed inside the overlay backdrop, with a measured top margin floating_at can't take.
        let placed = gpui_kit::anchored().position(card.at).snap_to_window_with_margin(ui::FLOAT_EDGES).child(rise);
        let pop = self.card_popover(card, window, cx);
        Some(crate::overlays::backdrop("profile-backdrop", cx).child(placed).children(pop).into_any_element())
    }

    // What a card wears around it, `width` wide: its profile effect over it, and its frame behind and in front.
    // Discord drops the frame from a window shorter than `frame_min`.
    pub(super) fn dressing(&self, profile: &Profile, width: f32, frame_min: f32, window: &Window) -> Dressing {
        let effect = profile.effect_sku.as_ref().and_then(|sku| self.profile_effects.get(sku)).and_then(|layers| layers.as_deref()).and_then(|layers| effect_overlay(layers, width));
        let frame = profile.frame.as_ref().filter(|_| f32::from(window.viewport_size().height) >= frame_min);
        Dressing { effect, back: frame.and_then(|f| frame_layers(f, width, false)), front: frame.and_then(|f| frame_layers(f, width, true)) }
    }

    // Discord's popout body: name, mutuals, bio, activity, then roles in a server.
    fn popout_sections(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> Div {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        ui::col()
            .px_4()
            .pt_1()
            .pb_2()
            .gap_3()
            .child(self.clickable_name(card, profile, ink, cx))
            // Not fetched yet: where the mutuals and bio will be.
            .when(card.profile.is_none(), |b| b.child(profile_skeleton(cx)))
            .children(self.relationship_notice(card, ink, cx))
            .children((!is_me).then(|| self.mutuals_row(&card.user, profile, ink, cx)).flatten())
            .children(profile.bio.as_ref().map(|bio| self.short_bio(&card.user, bio, ink, cx)))
            .children(self.profile_activity(card, ink, cx))
            .children(self.roles_row(card, profile, true, ink, cx).map(|roles| section("Roles", roles, ink)))
    }

    // The bio cut near three lines, with Discord's "View Full Bio" under it when some is hidden.
    pub(super) fn short_bio(&self, user: &User, bio: &str, ink: Ink, cx: &mut Context<Self>) -> Div {
        let (view, cut) = clamped_bio(&user.id, bio, ink);
        let user = user.clone();
        let more = cut.then(|| {
            ui::button("profile-full-bio", ui::Tone::Ghost, cx)
                .scoped(ink.tile, ink.strong.opacity(0.16), ink.text, ink.strong)
                .small()
                .text_size(ui::TEXT_XS)
                .child("View Full Bio")
                .on_click(cx.listener(move |this, _, _, cx| this.view_full_profile(&user, FullTab::Activity, cx)))
        });
        ui::col().items_start().gap_2().child(view).children(more)
    }

    // The name block whose display name opens the full profile, as a click on Discord's does.
    pub(super) fn clickable_name(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> Div {
        let user = card.user.clone();
        let open = card.full.is_none().then(|| std::rc::Rc::new(cx.listener(move |this, _: &gpui_kit::ClickEvent, _, cx| this.view_full_profile(&user, FullTab::Activity, cx))) as OnClick);
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let note = (card.full.is_none() && !is_me).then(|| self.note_button(card, profile, ink, cx).into_any_element());
        let tag = Some(std::rc::Rc::new(cx.listener(|this, event: &gpui_kit::ClickEvent, _, cx| this.open_tag_card(event.position(), cx))) as OnClick);
        name_block(&card.user, profile, ink, open, note, tag)
    }

    // Beside the name while the card is hovered: Add Note, or Edit Note when there is one. It opens the full
    // profile with the note being edited.
    // GAP: Discord edits the note in a small popover over the card; here it opens the full profile's field.
    fn note_button(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> ui::Control {
        let label = if profile.note.is_some() { "Edit Note (only visible to you)" } else { "Add Note (only visible to you)" };
        let (user, current) = (card.user.clone(), profile.note.clone().unwrap_or_default());
        ui::icon_button("profile-note", Glyph::Note, label, cx)
            .scoped(gpui_kit::transparent_black(), ink.tile, ink.muted, ink.strong)
            .small()
            .opacity(0.)
            .group_hover(CARD_GROUP, |s| s.opacity(1.))
            .on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                let _ = &user;
                this.open_note_popover(current.clone(), event.position(), window, cx);
            }))
    }

    // "N Mutual Friends • N Mutual Servers": each half opens the full profile on its tab.
    fn mutuals_row(&self, user: &User, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        let (to_friends, to_servers) = (user.clone(), user.clone());
        let friends = (profile.mutual_friend_count > 0).then(|| {
            let faces = profile.mutual_friends.iter().take(3).map(|u| u.avatar.clone());
            mutual_link("profile-mutual-friends", Some(ui::avatar_stack(faces, MUTUAL_FACE, ink.top)), mutual_count(profile.mutual_friend_count, "Mutual Friend"), ink)
                .on_click(cx.listener(move |this, _, _, cx| this.view_full_profile(&to_friends, FullTab::MutualFriends, cx)))
        });
        let servers = (profile.mutual_server_count > 0).then(|| {
            // Server icons stand in only when there are no friends' faces.
            let tiles = friends.is_none().then(|| {
                let tiles = profile.mutual_guilds.iter().take(3).filter_map(|m| self.guilds.iter().find(|g| g.id == m.id)).map(|g| ui::server_tile(g.icon.as_deref(), &g.name, MUTUAL_FACE, cx));
                ui::row().flex_none().gap_0p5().children(tiles)
            });
            mutual_link("profile-mutual-servers", tiles, mutual_count(profile.mutual_server_count, "Mutual Server"), ink)
                .on_click(cx.listener(move |this, _, _, cx| this.view_full_profile(&to_servers, FullTab::MutualServers, cx)))
        });
        if friends.is_none() && servers.is_none() {
            return None;
        }
        let both = friends.is_some() && servers.is_some();
        let text = if both { ui::TEXT_XS } else { ui::TEXT_SM };
        Some(ui::row().gap_2().text_size(text).children(friends).when(both, |r| r.child(div().size_1().flex_none().rounded_full().bg(ink.muted))).children(servers))
    }

    // Discord's date from the account's id, then, opened in a server, the server's from when they joined.
    pub(super) fn member_since(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &App) -> Option<Div> {
        card.profile.as_ref()?;
        let created = snowflake_time(&card.user.id)?;
        let guild = card.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id));
        let joined = guild.zip(profile.joined.as_deref().and_then(parse_time));
        let discord_mark = ui::row()
            .size_5()
            .flex_none()
            .squircle(RADIUS_XS)
            .bg(palette::accent())
            .justify_center()
            .children(crate::icons::brand_mark("discord").map(|path| gpui_kit::svg().path(path).size_3p5().text_color(palette::on_accent())));
        // With no server date it's the date alone, as Discord's; beside one, each carries its mark.
        if joined.is_none() {
            return Some(div().text_size(ui::TEXT_SM).text_color(ink.text).child(short_date(&created)));
        }
        let row = ui::row()
            .flex_wrap()
            .gap_2p5()
            .child(since_date("profile-since-discord", discord_mark, "Discord".into(), &created, ink))
            .when_some(joined, |row, (guild, joined)| {
                let icon = ui::server_tile(guild.icon.as_deref(), &guild.name, 20., cx);
                row.child(div().size_1().rounded_full().bg(ink.muted)).child(since_date("profile-since-server", icon, guild.name.clone().into(), &joined, ink))
            });
        Some(row)
    }

    pub(super) fn friends_since(&self, card: &ProfileCard) -> Option<String> {
        let friend = self.relationships.iter().find(|r| r.user.id == card.user.id && r.kind == Relation::Friend)?;
        Some(short_date(&parse_time(friend.since.as_deref()?)?))
    }

    // Banner with its round actions, then the avatar overlapping its foot and the custom status bubble beside it.
    fn profile_header(&self, card: &ProfileCard, profile: &Profile, kind: Header, hovered: bool, ink: Ink, cx: &mut Context<Self>) -> Div {
        let user = &card.user;
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user.id);
        let (banner, banner_height) = banner(profile, ink, kind.radius(ink), self.banner_plays(kind == Header::Panel, hovered), BANNER, cx);
        ui::col()
            .relative()
            .flex_none()
            .child(banner)
            .when(!is_me, |h| h.child(self.banner_actions(card, cx)))
            // lint-allow(raw-margin): the avatar row pulls up over the banner's foot, which padding can't do.
            .child(ui::row().px(px(CARD_INSET - AVATAR_RING)).mt(px(-(AVATAR / 2. + AVATAR_RING))).mb_2().child(self.profile_face(card, AVATAR, hovered, ink, cx)))
            .children(card_toast(card, banner_height, cx))
            .children(self.status_bubble_for(card, ink, cx).map(|bubble| bubble.absolute().top(px(banner_height + BUBBLE_DROP)).left(px(kind.bubble_left())).right_3()))
    }

    // Their avatar ringed in the card's colour; a click opens the full profile, or, in it, the picture itself.
    pub(super) fn profile_face(&self, card: &ProfileCard, size: f32, hovered: bool, ink: Ink, cx: &mut Context<Self>) -> gpui_kit::Stateful<Div> {
        let user = &card.user;
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user.id);
        let status = self.presences.get(&user.id).map(|p| p.mark()).or(self.my_mark().filter(|_| is_me).map(Into::into));
        let decoration = user.decoration.clone().or(card.profile.as_ref().and_then(|p| p.decoration.clone()));
        // Still until the card is hovered, as Discord's.
        let face = match hovered {
            true => playing_avatar(&format!("profile-{}", user.id), &user.avatar, decoration.as_deref(), size, status, ink.top),
            false => decorated_avatar(&user.avatar, decoration.as_deref(), size, status, ink.top),
        };
        let target = user.clone();
        let face = div().id("profile-avatar").p(px(AVATAR_RING)).rounded_full().bg(ink.top).cursor_pointer().child(face).on_click(cx.listener(move |this, _, _, cx| {
            match this.profile_card.as_mut().filter(|c| c.user.id == target.id && c.full.is_some()) {
                Some(card) => card.avatar_full = true,
                None => return this.view_full_profile(&target, FullTab::Activity, cx),
            }
            this.redraw_overlays(cx);
        }));
        let toolbar = self.interaction_toolbar(card, Source::Avatar, FACE_GROUP, ink, cx).map(|t| t.absolute().bottom_0().right_neg_6());
        div().id("profile-face").relative().group(FACE_GROUP).child(face).children(toolbar)
    }

    // The bubble beside the avatar: theirs when they've set a status, yours always (Add Status when unset).
    // Theirs carries React and Reply on hover, and a chevron that shows a long status whole.
    pub(super) fn status_bubble_for(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Option<Div> {
        if self.me.as_ref().is_some_and(|me| me.id == card.user.id) {
            return Some(self.own_status_bubble(ink, cx));
        }
        let text = self.custom_status(&card.user.id)?;
        let long = status_runs_long(&text);
        let toolbar = self.interaction_toolbar(card, Source::Status(text.clone()), BUBBLE_GROUP, ink, cx).map(|t| t.absolute().top_neg_4().right_neg_2());
        let chevron = long.then(|| {
            let (glyph, tip) = if card.status_expanded { (Glyph::ChevronUp, "Collapse custom status") } else { (Glyph::ChevronDown, "Expand custom status") };
            ui::icon_button("profile-status-expand", glyph, tip, cx)
                .scoped(ui::mix(ink.top, ink.strong, 0.08), ink.tile, ink.muted, ink.strong)
                .size_6()
                .rounded_full()
                .absolute()
                .bottom_neg_3()
                .right_2()
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(card) = this.profile_card.as_mut() {
                        card.status_expanded = !card.status_expanded;
                    }
                    this.redraw_overlays(cx);
                }))
        });
        let bubble = div().relative().min_w_0().group(BUBBLE_GROUP).child(status_bubble(text, card.status_expanded, ink, cx)).children(toolbar).children(chevron);
        Some(ui::row().items_start().child(bubble))
    }

    // Discord's rule for an animated banner: with GIFs autoplaying it plays whenever the popout or full profile
    // is open, and on the DM's side panel while the card is hovered; with autoplay off, only while hovered.
    pub(super) fn banner_plays(&self, panel: bool, hovered: bool) -> bool {
        let autoplay = self.account_settings.gif_auto_play && !self.prefs.reduces_motion();
        hovered || (autoplay && !panel)
    }

    // Their custom status, as the bubble beside the avatar shows it.
    pub(super) fn custom_status(&self, user_id: &str) -> Option<String> {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == user_id);
        match is_me {
            true => self.custom_status.as_ref().map(|_| self.status_line()).filter(|s| !s.is_empty()),
            false => self.presences.get(user_id).and_then(|p| p.custom.clone()),
        }
    }

    // Discord's round buttons over the banner: Mod View, the friend state, then More.
    fn banner_actions(&self, card: &ProfileCard, cx: &mut Context<Self>) -> Div {
        let user = card.user.clone();
        let friend = self.friend_action(&user).map(|(glyph, label, _)| {
            let target = user.clone();
            banner_button("profile-friend", glyph, label, cx).on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.friend_click(target.clone(), event.position(), cx)))
        });
        let mod_view = card.guild_id.clone().filter(|g| self.can_mod_view(g)).map(|g| {
            let id = user.id.clone();
            banner_button("profile-mod-view", Glyph::Shield, "Open in Mod View", cx).on_click(cx.listener(move |this, _, window, cx| this.open_mod_view(g.clone(), id.clone(), window, cx)))
        });
        let more = self.more_button(card, true, banner_button("profile-more", Glyph::More, "More", cx), cx);
        ui::row().items_stretch().absolute().top_2().right_3().gap_2().children(mod_view).children(friend).child(more)
    }

    // The friend button's look and what a click asks for: Add Friend, accept, or the state it's in.
    pub(super) fn friend_action(&self, user: &User) -> Option<(Glyph, &'static str, Option<Relation>)> {
        let relation = self.relationships.iter().find(|r| r.user.id == user.id).map(|r| r.kind);
        match relation {
            _ if user.bot => None,
            None => Some((Glyph::GroupPlus, "Add Friend", Some(Relation::Outgoing))),
            Some(Relation::Incoming) => Some((Glyph::GroupPlus, "Accept Friend Request", Some(Relation::Friend))),
            Some(Relation::Outgoing) => Some((Glyph::Clock, "Pending", None)),
            Some(Relation::Friend) => Some((Glyph::Users, "Friends", None)),
            Some(Relation::Blocked) => None,
        }
    }

    // More: the user's menu; from a popout or side panel it leads with "View Full Profile", as Discord's.
    pub(super) fn more_button(&self, card: &ProfileCard, view_full: bool, button: ui::Control, cx: &mut Context<Self>) -> ui::Control {
        let (user, guild_id) = (card.user.clone(), card.guild_id.clone());
        button.on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
            let (user, guild_id) = (user.clone(), guild_id.clone());
            this.open_live_menu(
                event.position(),
                move |this| {
                    let target = user.clone();
                    let lead = view_full.then(|| [crate::menu::item("View Full Profile", move |this, _, cx| this.view_full_profile(&target, FullTab::Activity, cx)).icon(Glyph::UserCircle).entry(), crate::menu::Entry::Separator]);
                    lead.into_iter().flatten().chain(this.user_menu(&user, guild_id.as_deref(), menu_surface(guild_id.is_some()))).collect()
                },
                cx,
            )
        }))
    }

    // Your own card's footer: Edit Profile; opened in a server it asks which profile, as Discord's does.
    pub(super) fn edit_profile_button(&self, guild_id: Option<String>, cx: &mut Context<Self>) -> Div {
        let button = ui::button("profile-edit", ui::Tone::Primary, cx).w_full().justify_center().child(Icon::new(Glyph::Pen).size(ui::ICON_MD)).child("Edit Profile").on_click(cx.listener(
            move |this, event: &gpui_kit::ClickEvent, window, cx| {
                if guild_id.is_none() {
                    return this.edit_own_profile(ProfileTab::Main, window, cx);
                }
                this.open_live_menu(
                    event.position(),
                    |_| {
                        let edit = |label: &'static str, hint: &'static str, tab| crate::menu::item(label, move |this, window, cx| this.edit_own_profile(tab, window, cx)).hint(hint).entry();
                        vec![edit("Edit Per-server Profile", "Visible only to those in this server", ProfileTab::Server), edit("Edit Main Profile", "Default profile and visible to everyone", ProfileTab::Main)]
                    },
                    cx,
                );
            },
        ));
        div().flex_none().px_4().pt_1().pb_3().child(button)
    }

    pub(super) fn edit_own_profile(&mut self, tab: ProfileTab, window: &mut Window, cx: &mut Context<Self>) {
        self.profile_card = None;
        self.open_settings(crate::settings::Page::Profiles, window, cx);
        self.ensure_profile_editor(window, cx);
        self.switch_profile_tab(tab, window, cx);
    }
}

// "Reply sent" or "Reaction sent", centred over the banner while it lasts.
pub(super) fn card_toast(card: &ProfileCard, banner_height: f32, cx: &App) -> Option<Div> {
    let text = card.toast?;
    Some(ui::row().absolute().left_0().right_0().top(px(banner_height / 2. - TOAST_HALF)).justify_center().child(ui::toast_text(SharedString::from(format!("profile-toast-{text}")), text, cx)))
}

// Half a one-line toast's height, to centre it on the banner.
const TOAST_HALF: f32 = 20.;

pub(super) struct Dressing {
    pub(super) effect: Option<Div>,
    pub(super) back: Option<Div>,
    pub(super) front: Option<Div>,
}

// The window heights below which Discord hides a frame: the popout's, the side panel's and the full profile's.
const FRAME_MIN_POPOUT: f32 = 480.;
pub(super) const FRAME_MIN_PANEL: f32 = 510.;
pub(super) const FRAME_MIN_FULL: f32 = 785.;

// Which card a header sits on: where its banner's corners round and where the status bubble starts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Header {
    Popout,
    // The DM's side panel, flush in its own rounded column.
    Panel,
}

impl Header {
    fn radius(self, ink: Ink) -> f32 {
        match self {
            Header::Popout => inner_radius(ink),
            Header::Panel => ui::RADIUS_LG,
        }
    }

    // Discord's `inset-inline-start` for the bubble: 105px on the popout, 109px on the side panel.
    fn bubble_left(self) -> f32 {
        match self {
            Header::Popout => BUBBLE_LEFT,
            Header::Panel => BUBBLE_LEFT_PANEL,
        }
    }
}

// The status bubble's offsets from Discord's CSS: from the card's left, and below the banner's foot.
const BUBBLE_LEFT: f32 = 105.;
const BUBBLE_LEFT_PANEL: f32 = 109.;
const BUBBLE_DROP: f32 = -7.;
// What the hover reveals listen to: the whole card (the note button), the status bubble and the avatar (React
// and Reply).
const CARD_GROUP: &str = "profile-card";
const BUBBLE_GROUP: &str = "profile-bubble";
const FACE_GROUP: &str = "profile-face";
const ACTIVITY_GROUP: &str = "profile-activity";

// Whether a status runs past the bubble's two lines: about 22 characters to a 155px line of 14px text.
fn status_runs_long(text: &str) -> bool {
    text.chars().count() > 2 * STATUS_LINE_CHARS
}

const STATUS_LINE_CHARS: usize = 22;
// A face in the mutuals row.
const MUTUAL_FACE: f32 = 16.;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::Connection;

    #[test]
    fn snowflake_dates() {
        // Discord's own example id, documented as minted 2016-04-30 11:18:25.796 UTC.
        assert_eq!(snowflake_time("175928847299117063").map(|t| t.timestamp_millis()), Some(1_462_015_105_796));
        assert_eq!(snowflake_time("not a number"), None);
        let at = parse_time("2021-03-04T12:00:00+00:00").expect("valid time");
        assert_eq!(at.format("%b %-d, %Y").to_string(), "Mar 4, 2021");
    }

    #[test]
    fn bio_clamp() {
        assert!(!bio_overflows("LUCKY VICKY\n**gpui** fan, probably in voice"));
        assert!(bio_overflows("one\ntwo\nthree\nfour"));
        assert!(bio_overflows(&"word ".repeat(30)));
        assert!(!bio_overflows(&"word ".repeat(20)));
    }

    #[test]
    fn mutual_counts() {
        assert_eq!(mutual_count(1, "Mutual Server"), "1 Mutual Server");
        assert_eq!(mutual_count(12, "Mutual Friend"), "12 Mutual Friends");
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
