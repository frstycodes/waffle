// The server rail and the channel list (design/css/navigation.css: .rail, .srv, .folder, .side, .ch).
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Div, FontWeight, Hsla, Radians, SharedString, Stateful, TooltipPlacement, Window,
    base::{Transition as Tween, transition},
    component::{ActiveTheme, Icon, tooltip::Tooltip},
    div, img, point, prelude::*, px, relative, rgb, svg,
};
use gpui_motion_kit::core::{Bezier, Easing, Motion, Target, Transition};

use crate::{
    Waffle, Edge, Part, RailEntry, Status,
    discord::{ActivityKind, Channel, ChannelKind, Folder, Guild, OnStage, Status as Status_},
    drag::{RowDrag, hole},
    icons::Glyph,
    layout::{RAIL_WIDTH, avatar_with_status},
    motion, palette,
    rail_drag::RailDrag,
    sortable::{Board, Place},
    stage::STAGE,
    ui::{self, Squircle},
};

// Rail geometry: 44px tiles 8px apart (at Default density), centred in the 68px rail, so the state pill
// sits on its edge.
pub(crate) const TILE: f32 = 44.;
// How far a header control sits below the window's top: a 34px control centred in the 48px header row.
const HEADER_INSET: f32 = 7.;
pub(crate) const TILE_X: f32 = (RAIL_WIDTH - TILE) / 2.;
const MINI: f32 = 16.;
const MINI_GAP: f32 = 4.;
const MINI_INSET: f32 = (TILE - 2. * MINI - MINI_GAP) / 2.;
// A folder opening is one trip: the servers travel from their mini tiles to their slots, the folder's
// pill splits into theirs and its count hands over to theirs on the way. Closing runs it backwards.
const TRIP: Duration = Duration::from_millis(260);

// Spacing that follows UI density (`ui::density_pad`).
fn rail_gap() -> f32 {
    ui::density_pad(8.)
}

// A channel row: its 16px of text plus padding, 32px at Default density.
fn row_height() -> f32 {
    16. + ui::density_pad(16.)
}

fn row_gap() -> f32 {
    ui::density_pad(2.)
}
// A live stage's row: its name with the topic under it.
const STAGE_ROW_HEIGHT: f32 = 44.;

#[derive(Clone)]
pub(crate) enum Face {
    Home,
    // A DM on the rail: the person's avatar, round like every person, or a group's.
    Person(crate::discord::DmFace),
    Icon(String),
    Letters(String),
    Folder { tint: Hsla, minis: Vec<Mini> },
}

// A server as a folder shows it: its icon, or its first letter.
#[derive(Clone)]
pub(crate) struct Mini {
    icon: Option<String>,
    letters: String,
}

// One tile's state, as the rail draws it.
struct RailTile {
    key: String,
    name: SharedString,
    selected: bool,
    unread: bool,
    mentions: u32,
    muted: bool,
    // The top-right media badge: what's live there, and whether you're connected to it.
    media: Option<Media>,
    face: Face,
    guild_id: Option<String>,
    // Badges shrink with a folder's trip (1 shown, 0 gone).
    badge_scale: f32,
    // An open folder's head trades its minis for the folder glyph (0 closed, 1 open).
    open: f32,
    // A folder head whose pill its servers carry off while it's open (their pills start from it).
    handed: bool,
}

impl Waffle {
    pub fn render_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dm_mentions: u32 = self.dms.iter().map(|d| self.mention_count(&d.id)).sum();
        let home = RailTile {
            key: "dms".into(),
            name: "Direct messages".into(),
            selected: self.guild_id.is_none(),
            unread: false,
            mentions: dm_mentions,
            muted: false,
            media: None,
            face: Face::Home,
            guild_id: None,
            badge_scale: 1.,
            open: 0.,
            handed: false,
        };
        let home = self.rail_item(home, window, cx);
        let dm_tiles = self.rail_dm_tiles(window, cx);
        let entries = self.rail_entries();
        let order: Vec<String> = entries.iter().map(|e| crate::channel_menu::rail_key(e, &self.guilds)).collect();
        self.note_left_folder();
        let top = self.rail_board.list(RAIL_LIST, order.clone(), window, cx);
        let mut items: Vec<AnyElement> = Vec::new();
        for (entry, key) in entries.into_iter().zip(&order) {
            let item = match entry {
                RailEntry::Guild(guild) => self.guild_item(guild, 1., window, cx),
                RailEntry::Folder(folder, guilds) => self.folder_item(folder, guilds, window, cx),
            };
            items.push(top.item(key, item));
        }
        // The rail's slot in the header row holds macOS's traffic lights while they show (Home then leads the
        // rail under the row); otherwise Home sits in it, inset like the header's other controls.
        let lights = crate::layout::traffic_lights_shown(window);
        let separator = div().w(px(24.)).h(px(2.)).flex_none().rounded(px(1.)).bg(palette::line_strong(cx));
        // Under the lights the rail starts below a strip the channel header's height (so their edges meet in one line),
        // its first tile just under that edge.
        let top = if lights { 4. } else { HEADER_INSET };
        div()
            .id("rail")
            .track_scroll(&self.rail_scroll)
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .pt(px(top))
            .pb(px(12.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(rail_gap()))
            .overflow_y_scroll()
            .text_size(px(14.))
            // Without the lights, Home holds the slot over a drag strip; Home keeps its clicks from the strip,
            // and the rail still scrolls under it.
            .when(!lights, |rail| rail.child(div().absolute().top_0().left_0().w_full().h(px(crate::layout::HEADER_HEIGHT)).window_control_area(gpui_kit::WindowControlArea::Drag)))
            .child(if lights { home } else { div().block_mouse_except_scroll().child(home).into_any_element() })
            .child(dm_tiles)
            .child(separator)
            .children(items)

            .child(rail_action("rail-add-server", Icon::new(Glyph::Plus), "Add a Server", cx).on_click(cx.listener(|this, _, window, cx| this.open_create_server(window, cx))))
            .child(rail_action("rail-discover", Nav::Compass.icon(), "Discover servers", cx).on_click(|_, _, cx| cx.open_url("https://discord.com/servers")))
    }

    /// The rail: Home and the servers scroll; the Inbox tile stays pinned at its foot, above the user
    /// area, however few servers there are.
    pub fn render_rail_column(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mentions = self.read_states.values().any(|r| r.mentions > 0);
        let open = self.inbox_open || self.inbox_page;
        let inbox = rail_action("rail-inbox", Icon::new(Glyph::Inbox), "Inbox", cx)
            .relative()
            .when(open, |b| b.bg(palette::selected(cx)).text_color(palette::strong(cx)))
            .when(mentions, |b| b.child(div().absolute().top(px(6.)).right(px(6.)).size(px(8.)).rounded_full().bg(palette::alert())))
            .on_click(cx.listener(|this, _, _, cx| this.open_inbox(cx)));
        // Under the traffic lights a strip sits above the tiles, a hard edge they scroll up to (ui::hard_edge).
        let lights = crate::layout::traffic_lights_shown(window);
        let strip = lights.then(|| ui::hard_edge(self.sidebar_scrolled(window), cx).h(px(crate::layout::HEADER_HEIGHT)).window_control_area(gpui_kit::WindowControlArea::Drag));
        div()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .children(strip)
            .child(div().relative().flex_1().min_h_0().w_full().child(self.render_rail(window, cx)))
            .child(div().flex_none().pt(px(rail_gap())).pb(px(8.)).child(inbox))
            .child(div().flex_none().h(self.user_panel_clearance()))
    }

    // Whether the rail or the channel list has scrolled under its header: one hairline then runs across both. Without
    // the traffic lights the rail has no strip to scroll under, so only the channel list counts.
    fn sidebar_scrolled(&self, window: &Window) -> impl Fn() -> bool + 'static {
        let rail = crate::layout::traffic_lights_shown(window).then(|| self.rail_scroll.clone());
        let channels = self.channels_scroll.clone();
        move || rail.as_ref().is_some_and(ui::scrolled) || ui::scrolled(&channels)
    }

    // DMs that want you, under Home as Discord stacks them: unread ones, newest first, and the one
    // you're in a call with.
    fn rail_dms(&self) -> Vec<Channel> {
        let in_call = |id: &str| self.call.as_ref().is_some_and(|c| c.guild_id.is_none() && c.channel.id == id);
        self.dms.iter().filter(|d| self.unread(&d.id) || in_call(&d.id)).cloned().collect()
    }

    // DM tiles grow in and fold away on one gently bouncing spring: the slot's height, the tile's fade and scale travel
    // together, so the servers below glide instead of jumping. The tile scales with its slot, so it never
    // reaches past it into Home or the next tile. A leaving tile keeps its last count and stays until its
    // exit settles. Each slot carries the gap above it, so the column cancels the rail's own gap.
    fn rail_dm_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let slot = TILE + rail_gap();
        let wanted: Vec<(Channel, u32)> = self.rail_dms().into_iter().map(|dm| {
            let count = self.mention_count(&dm.id);
            (dm, count)
        }).collect();
        let launch = self.rail_dms_shown.is_none();
        let before = self.rail_dms_shown.take().unwrap_or_default();
        let gone = Target::default().height(0.).opacity(0.);
        let mut kept = Vec::new();
        let mut slots = Vec::new();
        for (dm, count) in keep_leaving(wanted.clone(), before, |a, b| a.0.id == b.0.id) {
            let part = Part::RailDm(dm.id.clone());
            let present = wanted.iter().any(|(w, _)| w.id == dm.id);
            let motion = Motion::new(Target::default().height(slot).opacity(1.)).exit(gone).present(present).transition(motion::snappy());
            let motion = if launch { motion } else { motion.initial(gone) };
            let frame = self.motions.frame_one(part.clone(), motion, window, cx);
            if !frame.is_mounted() {
                self.motions.remove(&part);
                continue;
            }
            // Past 1 is the spring's overshoot, kept so the tile and its slot bounce together; below 0 is clamped.
            let shown = (frame.target().height_px().unwrap_or(slot) / slot).max(0.);
            let tile = self.dm_tile(dm.clone(), count, window, cx);
            slots.push(
                div()
                    .relative()
                    .w(px(RAIL_WIDTH))
                    .h(px(shown * slot))
                    .flex_none()
                    .child(div().absolute().left_0().top(px(rail_gap() * shown - (1. - shown) * TILE / 2.)).opacity(frame.target().alpha().clamp(0., 1.)).scale(shown).child(tile))
                    .into_any_element(),
            );
            kept.push((dm, count));
        }
        self.rail_dms_shown = (!launch || !self.dms.is_empty()).then_some(kept);
        div().flex_none().flex().flex_col().mt(px(-rail_gap())).children(slots)
    }

    fn dm_tile(&mut self, dm: Channel, mentions: u32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tile = RailTile {
            key: format!("dm-{}", dm.id),
            name: dm.recipient.as_ref().map_or_else(|| dm.name.clone(), |u| u.name.clone()).into(),
            selected: self.guild_id.is_none() && self.channel.as_ref().is_some_and(|c| c.id == dm.id),
            unread: false,
            // Every DM message counts as a mention, so this is its unread count.
            mentions,
            muted: false,
            media: self.call.as_ref().is_some_and(|c| c.guild_id.is_none() && c.channel.id == dm.id).then_some(Media { kind: MediaKind::Voice, connected: true }),
            face: Face::Person(dm.face()),
            guild_id: None,
            badge_scale: 1.,
            open: 0.,
            handed: false,
        };
        let item = self.rail_item(tile, window, cx);
        div()
            .id(SharedString::from(format!("rail-dm-hit-{}", dm.id)))
            .child(item)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_guild(None, window, cx);
                this.open_channel(dm.clone(), window, cx);
            }))
            .into_any_element()
    }

    // Who's in the server's voice channels and who's live there, by avatar, for its hover card.
    fn server_preview(&self, guild_id: &str, name: SharedString) -> ServerPreview {
        let muted = self.guild_settings.get(guild_id).is_some_and(|s| s.is_muted());
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return ServerPreview { name, voice: Vec::new(), live: Vec::new(), muted } };
        let members = self.members.get(guild_id);
        // Someone whose member hasn't loaded yet still shows, with Discord's default avatar for their id.
        let avatar = |state: &crate::discord::VoiceState| match state.user.as_ref().or_else(|| members.and_then(|m| m.get(&state.user_id))) {
            Some(u) => ui::Entry::person(u.avatar.clone(), u.name.clone()),
            None => ui::Entry::person(crate::discord::avatar_url(&state.user_id, None), state.user_id.clone()),
        };
        let present = guild.voice_states.iter().filter(|v| v.channel_id.is_some());
        ServerPreview {
            name,
            // Each person once: someone live shows on the stream row, not on both.
            voice: present.clone().filter(|v| !v.streaming).map(avatar).collect(),
            live: present.filter(|v| v.streaming).map(avatar).collect(),
            muted,
        }
    }

    // Discord's server icon badge (GuildMediaStateStore.getGuildMediaState): what's live in this server's
    // voice, from people you haven't blocked or ignored, in channels you can see that aren't a stage, the
    // AFK channel or muted. A muted server shows none unless you're in its voice.
    fn guild_media(&self, guild: &Guild) -> Option<Media> {
        let me = self.me.as_ref()?.id.as_str();
        let settings = self.guild_settings.get(&guild.id);
        let connected = self.call.as_ref().filter(|c| c.guild_id.as_ref() == Some(&guild.id)).map(|c| c.channel.id.as_str());
        let counts = |state: &crate::discord::VoiceState| {
            let Some(channel_id) = state.channel_id.as_deref() else { return false };
            let Some(channel) = guild.channels.iter().find(|c| c.id == channel_id) else { return false };
            channel.raw_type != 13 && guild.afk_channel_id.as_deref() != Some(channel_id) && guild.can_view(me, channel) && !settings.is_some_and(|s| s.channel_muted(channel_id)) && self.hidden_author(&state.user_id).is_none()
        };
        media_state(&guild.voice_states, me, connected, settings.is_some_and(|s| s.is_muted()), counts)
    }

    fn guild_tile(&self, guild: &Guild, badge_scale: f32) -> RailTile {
        let (unread, mentions) = self.guild_badge(guild);
        RailTile {
            key: guild.id.clone(),
            name: guild.name.clone().into(),
            selected: self.guild_id.as_ref() == Some(&guild.id),
            unread,
            mentions,
            muted: self.guild_settings.get(&guild.id).is_some_and(|s| s.is_muted()),
            media: self.guild_media(guild),
            face: server_face(guild),
            guild_id: Some(guild.id.clone()),
            badge_scale,
            open: 0.,
            handed: false,
        }
    }

    fn guild_item(&mut self, guild: usize, badge_scale: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tile = self.guild_tile(&self.guilds[guild], badge_scale);
        self.rail_item(tile, window, cx)
    }

    // A folder: closed, a tile of its first four servers; open, a tinted column holding them all.
    fn folder_item(&mut self, folder: Folder, indices: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let guilds: Vec<&Guild> = indices.iter().map(|&i| &self.guilds[i]).collect();
        let open = self.folder_shown_open(&folder);
        let key = format!("folder-{}", folder.id);
        let motion = FolderMotion::frame(&key, open, window, cx);
        let tint: Hsla = folder.color.map(|c| rgb(c).into()).unwrap_or(palette::faint(cx));
        let (unread, mentions) = guilds.iter().map(|g| self.guild_badge(g)).fold((false, 0), |(u, m), (gu, gm)| (u || gu, m + gm));
        let has_selected = guilds.iter().any(|g| self.guild_id.as_ref() == Some(&g.id));
        let name: SharedString = folder.name.clone().unwrap_or_else(|| guilds.iter().map(|g| g.name.as_str()).take(3).collect::<Vec<_>>().join(", ")).into();
        let minis: Vec<Mini> = guilds.iter().map(|g| mini(g)).collect();
        let badges: Vec<Badge> = guilds.iter().map(|g| {
            let (unread, mentions) = self.guild_badge(g);
            Badge { pill: pill_height(self.guild_id.as_ref() == Some(&g.id), false, unread), mentions }
        }).collect();
        // The height the servers' pills split from and merge back into: the head's own, hover included.
        let media = guilds.iter().filter_map(|g| self.guild_media(g)).max_by_key(|m| (m.kind, m.connected));
        let head_pill = pill_height(has_selected, self.hovered.as_deref() == Some(key.as_str()), unread);
        let handed = badges.iter().any(|b| b.pill > 0.);
        let rail = palette::surface(palette::Level::Rail, cx);
        let head = RailTile {
            key: key.clone(),
            name,
            selected: has_selected,
            unread,
            mentions,
            muted: false,
            // A closed folder carries its servers' strongest badge; open, each server shows its own.
            media: media.filter(|_| motion.trip <= 0.),
            face: Face::Folder { tint, minis: minis.iter().take(4).cloned().collect() },
            guild_id: None,
            // Its count hands over to the servers' over the trip, gone once they land.
            badge_scale: 1. - motion.trip,
            open: motion.trip,
            handed,
        };
        let head = self.rail_item(head, window, cx);
        let (toggle_id, menu_id) = (folder.id.clone(), folder.id.clone());
        let head = div()
            .id(SharedString::from(format!("folder-hit-{}", folder.id)))
            .absolute()
            .top_0()
            .left_0()
            .child(head)
            .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_folder_menu(menu_id.clone(), e.position, cx)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.open_folders.remove(&toggle_id) {
                    this.open_folders.insert(toggle_id.clone());
                }
                cx.notify();
            }));
        let count = indices.len();
        let landed = motion.trip >= 1.;
        let servers: Vec<AnyElement> = match (motion.trip > 0., landed) {
            (false, _) => Vec::new(),
            (true, true) => {
                let order: Vec<String> = indices.iter().map(|&g| self.guilds[g].id.clone()).collect();
                let inside = self.rail_board.list(folder_list(&folder.id), order.clone(), window, cx);
                indices.into_iter().zip(order).enumerate().map(|(i, (g, id))| div().absolute().left_0().top(px(slot_y(i))).child(inside.item(&id, self.guild_item(g, 1., window, cx))).into_any_element()).collect()
            }
            (true, false) => minis.iter().zip(&badges).enumerate().map(|(i, (m, badge))| flying_tile(m, badge, head_pill, i, minis.len(), motion.trip, rail, cx)).collect(),
        };
        div()
            .relative()
            .w(px(RAIL_WIDTH))
            .h(px(TILE + motion.trip * count as f32 * (TILE + rail_gap())))
            .flex_none()
            .child(div().absolute().top_0().bottom_0().left(px(TILE_X)).w(px(TILE)).squircle(ui::RADIUS_LG).bg(tint.opacity(0.18 * motion.trip)))
            // Servers over the head: in flight they cross its tinted face, which would shade them until
            // they land as its unshaded minis, a flash at the end of a close.
            .child(head)
            .children(servers)
            .into_any_element()
    }

    // A rail tile: the face, the state pill on the rail's edge (dot unread, longer on hover, full when
    // selected), and a count for mentions.
    fn rail_item(&mut self, tile: RailTile, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let RailTile { key, name, selected, unread, mentions, muted, media, face, guild_id, badge_scale, open, handed } = tile;
        let hovered = self.hovered.as_deref() == Some(&key);
        let pill_height = pill_height(selected, hovered, unread);
        let pill = self.motions.frame_one(Part::Pill(key.clone()), Motion::new(Target::default().height(pill_height)).transition(hover_ease()), window, cx);
        // A folder's head hands its pill to its servers for the trip (theirs start and end at its height);
        // with no server to take it (a hover alone), it eases away with the trip instead.
        let pill_height = pill.target().height_px().unwrap_or(pill_height).max(0.);
        let pill_height = match (open > 0., handed) {
            (false, _) => pill_height,
            (true, true) => 0.,
            (true, false) => pill_height * (1. - open),
        };
        let rail = palette::surface(palette::Level::Rail, cx);
        let look = crate::drag::Look::Tile(face.clone());
        // Reordering shows as the rail making room; going onto a tile (a folder made or joined) as a ring.
        let combine_ring = self.rail_board.hint().is_some_and(|h| h.place == Place::Into && h.target.as_deref() == Some(key.as_str()));
        let item_id = SharedString::from(format!("rail-{key}"));
        let carried = self.drag_motion.row(&item_id);
        let face = match carried {
            RowDrag::Rest => render_face(face, TILE, ui::RADIUS_LG, selected, open, cx),
            RowDrag::Hole => hole(div().size(px(TILE)).squircle(ui::RADIUS_LG), cx).into_any_element(),
            RowDrag::Hidden => div().size(px(TILE)).into_any_element(),
        };
        let hover_key = key.clone();
        let preview = guild_id.as_deref().map(|id| self.server_preview(id, name.clone()));
        let item = ui::pressable(div().id(item_id.clone()), 0.97)
            .children(self.drag_motion.spot(&item_id))
            .relative()
            .w(px(RAIL_WIDTH))
            .h(px(TILE))
            .flex_none()
            .cursor_pointer()
            .child(div().absolute().left_0().top(px((TILE - pill_height) / 2.)).w(px(4.)).h(px(pill_height)).rounded_r(px(4.)).bg(palette::strong(cx)))
            // GAP: muted tiles skip the design's saturate(0.35): gpui has no element filters.
            // The tooltip hangs off the tile rather than the rail-wide item, so it opens a fixed gap from the art.
            .child(
                div()
                    .id(SharedString::from(format!("rail-tile-{key}")))
                    .absolute()
                    .left(px(TILE_X))
                    .top_0()
                    .when(muted, |f| f.opacity(0.55))
                    .child(face)
                    // Hoverable, so the pointer can reach a row's "+n" and its full list.
                    .hoverable_tooltip({
                        let name = name.clone();
                        move |window, cx| match preview.clone().filter(ServerPreview::has_activity) {
                            Some(preview) => cx.new(|_| preview).into(),
                            None => Tooltip::new(name.clone()).build(window, cx),
                        }
                    })
                    .tooltip_show_delay(Duration::ZERO)
                    .tooltip_placement(TooltipPlacement::Side),
            )
            .when(mentions > 0 && badge_scale > 0.01, |item| item.child(rail_count(mentions, badge_scale, crate::material::behind_sidebar(rail))))
            .children(media.filter(|_| badge_scale > 0.99).map(|m| media_badge(m, crate::material::behind_sidebar(rail), cx)))
            .when(combine_ring, |item| item.child(combine_ring_mark()))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(&hover_key, *hovered, cx)))
            .when_some(guild_id.clone(), |item, guild_id| item.on_click(cx.listener(move |this, _, window, cx| this.open_guild(Some(guild_id.clone()), window, cx))))
            .when(key == "dms", |item| item.on_click(cx.listener(|this, _, window, cx| this.open_guild(None, window, cx))));
        let item = match key.as_str() {
            k if k == "dms" || k.starts_with("dm-") => item,
            _ => self.rail_draggable(item, RailDrag { key: key.clone() }, look, self.rail_list_of(&key), cx),
        };
        let Some(guild_id) = guild_id else { return item.into_any_element() };
        item.on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_server_menu(guild_id.clone(), e.position, false, cx)))
            .into_any_element()
    }

    // Carried, a tile swells to 104% and hangs at -4° (.srv.dragging), tilting further with speed. A server
    // can go onto another top-level tile (making or joining a folder); inside a folder it only reorders.
    fn rail_draggable(&self, item: ui::Pressable, drag: RailDrag, look: crate::drag::Look, list: SharedString, cx: &mut Context<Self>) -> ui::Pressable {
        let key = drag.key.clone();
        let ghost = crate::drag::pick_up(cx.weak_entity(), format!("rail-{key}").into(), look, point(px(RAIL_WIDTH), px(TILE)), 1.04, -4., true);
        let combines_here = list == RAIL_LIST;
        let item = item.on_drag(drag, Board::lift(cx.weak_entity(), rail_board, key.clone(), ghost));
        Board::target(item, rail_board, list, key, move |d: &RailDrag| combines_here && !d.key.starts_with("folder-"), cx)
    }

    // The rail list `key` sits in: its open folder's, or the rail's own.
    fn rail_list_of(&self, key: &str) -> SharedString {
        match self.folders.iter().find(|f| !f.id.is_empty() && f.id != "0" && f.guild_ids.iter().any(|g| g == key)) {
            Some(folder) => folder_list(&folder.id),
            None => RAIL_LIST.into(),
        }
    }

    // Sticky for the drag: once the dragged server is aimed outside the folder it came from, that folder
    // folds too.
    fn note_left_folder(&mut self) {
        let Some(dragged) = self.rail_board.dragged() else { return self.rail_left_folder = false };
        let Some(home) = self.folders.iter().find(|f| f.guild_ids.iter().any(|g| g == dragged)) else { return };
        let inside = self.rail_board.hint().is_none_or(|h| h.list == folder_list(&home.id) || h.target.as_deref() == Some(format!("folder-{}", home.id).as_str()));
        if !inside {
            self.rail_left_folder = true;
        }
    }

    // Open folders fold shut while a rail drag is on, so only top-level slots are in the way; the folder
    // the dragged server came from stays open until the server leaves it. They reopen on letting go.
    fn folder_shown_open(&self, folder: &Folder) -> bool {
        let open = self.open_folders.contains(&folder.id);
        match self.rail_board.dragged() {
            None => open,
            Some(dragged) => open && folder.guild_ids.iter().any(|g| g == dragged) && !self.rail_left_folder,
        }
    }

    // .preview, the hover card on someone live: the stream's thumbnail under LIVE, with a way in
    // over it on hover, then what they share and how many watch. It grows toward its row from 96%
    // with a fade, passing 100% before it settles.
    fn stream_card(&self, streamer: Streamer, channel: &Channel, stream_key: &str, connected: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Only once the bytes are in, so the fade below shows the picture rather than an empty box.
        let preview = self.stream_preview(streamer.id).filter(|url| matches!(window.use_asset::<gpui_kit::ImgResourceLoader>(&gpui_kit::Resource::Uri(url.clone().into()), cx), Some(Ok(_))));
        let app = self.presences.get(streamer.id).and_then(|p| p.activity.as_ref()).filter(|a| matches!(a.kind, ActivityKind::Playing | ActivityKind::Streaming)).map(|a| a.text.as_str());
        let viewer_ids: Vec<&String> = self.stream_viewers.get(stream_key).into_iter().flatten().filter(|id| *id != streamer.id).collect();
        let viewer_names: Vec<String> = viewer_ids.iter().map(|id| self.user_name(id)).collect();
        let watching = (!viewer_ids.is_empty()).then(|| format!("{} watching", viewer_ids.len()));
        let live = stream_meta(None, self.stream_live_since.get(stream_key).map(|at| at.elapsed()));
        let detail: Vec<String> = app.map(|_| streamer.name.to_owned()).into_iter().chain(watching).chain(live).collect();
        let who: Option<SharedString> = viewer_line(&viewer_names).map(Into::into);
        let hover_key = stream_hover_key(streamer.id);
        let (watch_id, watch_channel) = (streamer.id.to_owned(), channel.clone());
        let overlay = palette::surface(palette::Level::Overlay, cx);
        let thumb_group = SharedString::from(format!("stream-thumb-{}", streamer.id));
        let thumb = div()
            .id(SharedString::from(format!("stream-card-thumb-{}", streamer.id)))
            .group(thumb_group.clone())
            .relative()
            .w_full()
            .aspect_ratio(16. / 9.)
            .cursor_pointer()
            .child(crate::call::stream_fill(ui::RADIUS_MD).absolute().inset_0())
            .children(preview.map(|url| {
                ui::media(url, ui::all(ui::RADIUS_MD))
                    .absolute()
                    .size_full()
                    .object_fit(gpui_kit::ObjectFit::Cover)
                    .with_animation(SharedString::from(format!("stream-preview-{}", streamer.id)), Animation::new(ui::DUR_POP), |el, t| el.opacity(t))
            }))
            .child(ui::live_badge().absolute().top(px(8.)).left(px(8.)))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .squircle(ui::RADIUS_MD)
                    .bg(gpui_kit::black().opacity(0.45))
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.)
                    .group_hover(thumb_group, |s| s.opacity(1.))
                    .child(crate::call::watch_button(if connected { "Watch stream" } else { "Join and watch" })),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.stream_card_hover = None;
                match connected {
                    true => this.watch_stream(watch_id.clone(), cx),
                    // Not in that call yet: join it, then watch once connected.
                    false => this.watch_on_connect = Some(watch_id.clone()),
                }
                this.show_call(watch_channel.clone(), window, cx);
            }));
        let cap = div()
            .id(SharedString::from(format!("stream-card-cap-{}", streamer.id)))
            .px(px(6.))
            .pt(px(2.))
            .pb(px(4.))
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .child(ui::avatar(streamer.avatar.to_owned(), 24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(app.unwrap_or(streamer.name).to_owned()))
                    .child(div().truncate().text_size(px(12.)).text_color(palette::faint(cx)).child(detail.join(" · "))),
            )
            .when_some(who, |d, who| d.tooltip(move |window, cx| Tooltip::new(who.clone()).build(window, cx)));
        let record = self.stream_card_bounds.clone();
        div()
            .id(SharedString::from(format!("stream-card-{}", streamer.id)))
            // Blocks the rows under it; its own hover keeps it open once the pointer leaves the row.
            .occlude()
            .w(px(STREAM_CARD_WIDTH))
            .child(gpui_kit::canvas(move |bounds, _, _| record.set(bounds), |_, _, _, _| {}).absolute().inset_0())
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                this.stream_card_hover = hovered.then(|| hover_key.clone());
                cx.notify();
            }))
            .p(px(6.))
            .squircle(ui::RADIUS_LG)
            .bg(overlay)
            .shadow(ui::shadow_pop(cx))
            .flex()
            .flex_col()
            .gap(px(6.))
            .text_size(px(14.))
            .child(thumb)
            .child(cap)
            .transform_origin(point(relative(0.), relative(0.)))
            .with_animation(SharedString::from(format!("stream-card-open-{}", streamer.id)), Animation::new(Duration::from_millis(180)).with_easing(motion::back_out), |card, t| {
                card.scale(0.96 + 0.04 * t).opacity((t / 0.6).clamp(0., 1.))
            })
    }

    pub fn render_channels(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        match (&self.status, self.guild().cloned()) {
            // Until the gateway's first READY: the list's shape, shimmering.
            (Status::Connecting, guild) => rows.extend((0..8).map(|i| list_skeleton(i, guild.is_some(), cx))),
            (_, Some(guild)) => {
                let me = self.my_id().to_owned();
                // Beside the carried channel: "Move to Archive" while it's aimed into another category.
                let tag = self.channel_board.dragged().zip(self.channel_board.hint()).and_then(|(moved, hint)| self.category_move(moved, hint.target.as_deref()?));
                self.drag_motion.set_tag(tag);
                rows.extend(self.event_rows(&guild, cx));
                rows.extend(self.guild_entry_rows(&guild, window, cx));
                let sections = channel_sections(&guild, &me).into_iter().map(|(category, channels)| (category, channels.into_iter().filter(|c| self.shows_in_list(&guild, c)).collect::<Vec<_>>())).filter(|(_, channels)| !channels.is_empty()).collect::<Vec<_>>();
                for (i, (category, channels)) in sections.into_iter().enumerate() {
                    let order: Vec<String> = channels.iter().map(|c| c.id.clone()).collect();
                    let list = self.channel_board.list(channel_list(category.as_ref()), order, window, cx);
                    // A channel's first row is the item; its people and threads move with it.
                    let channel_rows = channels
                        .iter()
                        .flat_map(|c| {
                            let rows = self.channel_rows(c, &guild, window, cx);
                            let list = &list;
                            rows.into_iter().enumerate().map(move |(i, row)| ChannelRow {
                                keep: row.keep,
                                element: if i == 0 { list.item(&c.id, row.element) } else { list.follow(&c.id, row.element) },
                            })
                        })
                        .collect::<Vec<_>>();
                    match category {
                        Some(category) => rows.push(self.section(category, channel_rows, i > 0, window, cx)),
                        None => rows.extend(channel_rows.into_iter().map(|row: ChannelRow| row.element)),
                    }
                }
            }
            (_, None) => {
                rows.push(dm_find(cx).mb(px(8.)).on_click(cx.listener(|this, _, window, cx| this.open_switcher(window, cx))).into_any_element());
                rows.push(self.friends_row(cx));
                rows.extend(self.requests_row(cx));
                let start = cat_add("dm-start", None, "Start a conversation", cx).on_click(cx.listener(|this, _, window, cx| this.open_switcher(window, cx)));
                rows.push(div().mt(px(ui::density_pad(16.))).child(category_head("dm-head".into(), div().id("dm-head").child("Direct messages"), true, cx).child(start)).into_any_element());
                match self.dms.is_empty() {
                    true => rows.push(muted_line("No direct messages yet", cx)),
                    false => rows.extend(self.dms.clone().iter().map(|c| self.dm_row(c, cx))),
                }
            }
        }
        let reconnecting = match &self.status {
            Status::Reconnecting(reason) => Some(reason.clone()),
            _ => None,
        };
        let banner = self.motions.frame_one(
            Part::Banner,
            Motion::new(Target::default())
                .initial(Target::default().y(10.).opacity(0.))
                .exit(Target::default().y(10.).opacity(0.))
                .present(reconnecting.is_some())
                .transition(motion::spring()),
            window,
            cx,
        );
        let warning = cx.theme().warning;
        let banner = banner.render(|frame| {
            frame.apply(
                div()
                    .mx_2()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .squircle(ui::RADIUS_MD)
                    .bg(warning.opacity(0.12))
                    .text_size(px(12.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(warning))
                    .child(div().truncate().child(format!("Reconnecting… {}", reconnecting.clone().unwrap_or_default()))),
            )
        });
        // The header row: 8px in, like the list below, so the name lines up with the channel rows; below
        // 100% zoom the traffic lights outgrow the rail and push it further in.
        let header = div()
            .h(px(crate::layout::HEADER_HEIGHT))
            .px_2()
            .pl(px(8. + crate::layout::traffic_lights_overhang(window)))
            .flex()
            .items_center()
            .window_control_area(gpui_kit::WindowControlArea::Drag)
            .child(self.channel_list_head(cx));
        div()
            .relative()
            .w(px(self.channels_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(crate::material::sidebar_fill(cx))
            .text_color(palette::muted(cx))
            .text_size(px(14.))
            // The list stops under the header, a hard edge (ui::hard_edge).
            .child(ui::hard_edge(self.sidebar_scrolled(window), cx).child(header))
            .child(ui::scroll_area(
                div()
                    .id("channels")
                    .pt(px(8.))
                    .px_2()
                    .pb_2()
                    .flex()
                    .flex_col()
                    .gap(px(row_gap()))
                    .children(rows)
,
                &self.channels_scroll,
            ))
            .child(banner)
            .child(div().flex_none().h(self.user_panel_clearance()))
            .child(self.resize_handle(Edge::Channels, cx))
    }


    /// What heads the channel list: the open server's name, or "Home" (its search leads the list below).
    pub(crate) fn channel_list_head(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.guild() {
            Some(guild) => server_name_button(guild.name.clone().into(), cx).into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .px_2()
                .truncate()
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::strong(cx))
                .child("Home")
                .into_any_element(),
        }
    }

    // A category folds from its label: height and fade together on the fold curve, the chevron turns.
    // Folded, its unread and selected channels stay in view.
    fn section(&mut self, category: Channel, rows: Vec<ChannelRow>, spaced: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let folded = self.folded.contains(&category.id);
        let [fold, chevron] = self.motions.frame(
            [
                (Part::Fold(category.id.clone()), Motion::new(Target::default().opacity(if folded { 0. } else { 1. })).transition(motion::fold())),
                // The chevron's angle in degrees rides the x channel; it is read back, never applied as an offset.
                (Part::Chevron(category.id.clone()), Motion::new(Target::default().x(if folded { -90. } else { 0. })).transition(hover_ease())),
            ],
            window,
            cx,
        );
        let degrees = chevron.target().x_px();
        let target_ink = palette::accent_ink(cx);
        let id = category.id.clone();
        let label = div()
            .id(SharedString::from(format!("section-{}", category.id)))
            .child(Nav::Caret.icon().size(px(11.)).rotate(Radians(degrees.to_radians())))
            .child(ui::truncating(format!("category-name-{}", category.id), category.name.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.folded.remove(&id) {
                    this.folded.insert(id.clone());
                }
                cx.notify();
            }))
            .drag_over::<crate::drag::ChannelDrag>(move |s, _, _, _| s.text_color(target_ink))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let category = category.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| {
                    let Some(guild_id) = this.guild_id.clone() else { return };
                    this.open_channel_menu(category.clone(), guild_id, e.position, cx);
                })
            });
        let label = Board::head::<_, crate::drag::ChannelDrag, _>(label, channel_board, channel_list(Some(&category)), cx);
        let can_add = self.guild_id.as_ref().is_some_and(|g| self.can_arrange_channels(g));
        let group: SharedString = format!("cathead-{}", category.id).into();
        let head = category_head(group.clone(), label, false, cx).when(can_add, |head| {
            let category = category.clone();
            head.child(cat_add(SharedString::from(format!("section-add-{}", category.id)), Some(group), "Create channel", cx).on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    let Some(guild_id) = this.guild_id.clone() else { return };
                    cx.stop_propagation();
                    this.open_create_channel(guild_id, 0, Some(category.id.clone()), window, cx);
                }),
            ))
        });
        let alpha = fold.target().alpha();
        let gone = folded && alpha <= 0.05;
        let rows = rows.into_iter().filter(|row| !gone || row.keep).map(|row| match row.keep {
            true => row.element,
            false => div().opacity(alpha).child(row.element).into_any_element(),
        });
        div().when(spaced, |g| g.mt(px(ui::density_pad(16.)))).flex().flex_col().gap(px(row_gap())).child(head).children(rows).into_any_element()
    }

    fn channel_rows(&self, channel: &Channel, guild: &Guild, window: &mut Window, cx: &mut Context<Self>) -> Vec<ChannelRow> {
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let settings = self.guild_settings.get(&guild.id);
        let muted = settings.is_some_and(|s| s.channel_muted(&channel.id) || channel.parent_id.as_ref().is_some_and(|p| s.channel_muted(p)));
        // Discord's "Hide Muted Channels" keeps the open channel visible.
        if muted && !selected && settings.is_some_and(|s| s.hide_muted) {
            return Vec::new();
        }
        let is_voice = channel.kind == ChannelKind::Voice;
        let in_call = self.call.as_ref().is_some_and(|c| c.channel.id == channel.id);
        // A voice channel's chat marks it unread only while you're in it; mentions show always, as Discord's.
        let unread = self.unread(&channel.id) && !muted && (!is_voice || in_call);
        let mentions = self.mention_count(&channel.id);
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        // In the channel with no count beside them, its buttons stay out without hover.
        let actions_shown = selected || hovered || (is_voice && in_call && mentions == 0);
        let state = RowState { selected, unread, muted, strong: in_call };
        let elapsed = self.call.as_ref().filter(|c| c.channel.id == channel.id && c.connected).and_then(|_| self.voice_elapsed(&channel.id)).map(|secs| {
            div().flex_none().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(palette::positive_ink(cx)).child(crate::call::clock(secs))
        });
        let group: SharedString = format!("ch-{}", channel.id).into();
        // An unsent draft outranks the muted bell in the meta slot.
        let meta = match self.drafts.has(&channel.id) {
            true => Some(draft_mark()),
            false => muted.then(|| Nav::BellSlash.icon().size(px(16.))),
        };
        let faint = palette::faint(cx);
        let topic = guild.stage(&channel.id).map(|s| s.topic.clone());
        let carried = self.drag_motion.row(&channel.id);
        let welcomes = channel.kind == ChannelKind::Voice && self.welcomes_voice_drop(channel, guild);
        let row = channel_row(SharedString::from(channel.id.clone()), group.clone(), state, cx).children(self.drag_motion.spot(&channel.id));
        // A voice channel's status (or the way to set one) is a second line inside the row's fill and hit area.
        let status_line = self.voice_row_status(channel, cx);
        let row = match carried {
            RowDrag::Hole => hole(row.bg(gpui_kit::transparent_black()), cx),
            _ => {
                let line = div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(channel_glyph(channel).size(px(18.)).flex_none().text_color(match (welcomes, selected) {
                        (true, _) => palette::accent_ink(cx),
                        (false, true) => palette::muted(cx),
                        (false, false) => faint,
                    }))
                    .child(channel_label(channel, topic.clone(), cx))
                    .children(elapsed)
                    .children(meta.filter(|_| !actions_shown).map(|icon| div().flex_none().flex().text_color(faint).child(icon)))
                    .when(actions_shown, |r| r.child(self.row_actions(channel, guild, cx)))
                    .when(mentions > 0, |r| r.child(ui::count_badge(mentions)));
                let row = row.when(carried == RowDrag::Hidden, |r| r.invisible()).when(welcomes, |r| r.bg(palette::accent_soft(cx)).text_color(palette::strong(cx)));
                match status_line {
                    Some(status) => row.h_auto().py(px((row_height() - 20.) / 2.)).flex_col().items_stretch().gap(px(2.)).child(line.min_h(px(20.))).child(status),
                    None => row.when(topic.is_some(), |r| r.h(px(STAGE_ROW_HEIGHT))).child(line.h_full()),
                }
            }
        };
        let row = self.track_row_hover(self.channel_draggable(row, channel, guild, cx), &channel.id, !is_voice, cx);
        let keep = selected || unread;
        if !is_voice {
            let open = channel.clone();
            let row = row
                .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
                .on_mouse_down(gpui_kit::MouseButton::Right, {
                    let (channel, guild_id) = (channel.clone(), guild.id.clone());
                    cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_channel_menu(channel.clone(), guild_id.clone(), e.position, cx))
                });
            let mut rows = vec![ChannelRow { keep, element: row.into_any_element() }];
            rows.extend(self.thread_rows(channel, guild, cx));
            return rows;
        }
        let join = channel.clone();
        let row = self
            .voice_drop_zone(row, channel.id.clone().into(), channel, cx)
            .on_click(cx.listener(move |this, _, window, cx| {
                // With mentions waiting, Discord joins and opens the chat.
                if this.mention_count(&join.id) > 0 {
                    this.show_call(join.clone(), window, cx);
                    return this.open_call_chat(join.clone(), window, cx);
                }
                let in_call = this.call.as_ref().map(|c| c.channel.id.as_str());
                match voice_row_click(in_call, &join.id) {
                    VoiceRowClick::Join => this.join_voice(join.clone(), cx),
                    VoiceRowClick::OpenStage => this.show_call(join.clone(), window, cx),
                }
            }))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let (channel, guild_id) = (channel.clone(), guild.id.clone());
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_channel_menu(channel.clone(), guild_id.clone(), e.position, cx))
            });
        let mut rows = vec![ChannelRow { keep: keep || in_call, element: row.into_any_element() }];
        // A carried channel takes its people along in the ghost.
        if carried == RowDrag::Rest {
            rows.extend(self.voice_rows(channel, guild, in_call, window, cx).into_iter().map(|element| ChannelRow { keep: in_call, element }));
        }
        rows
    }

    // A voice channel takes people dropped anywhere on it, its row or the people under it. Each part
    // claims the target while the pointer is on it and lets go only of its own claim, so moving
    // between parts of one channel never drops it.
    fn voice_drop_zone(&self, zone: Stateful<Div>, part: SharedString, channel: &Channel, cx: &mut Context<Self>) -> Stateful<Div> {
        let (target, name, drop_target) = (channel.id.clone(), channel.name.clone(), channel.clone());
        zone.on_drag_move::<crate::drag::VoiceDrag>(cx.listener(move |this, e: &gpui_kit::DragMoveEvent<crate::drag::VoiceDrag>, _, cx| {
            let claim = Some((target.clone(), part.clone()));
            let inside = e.bounds.contains(&e.event.position) && this.can_move_voice(&e.drag(cx).guild_id, &e.drag(cx).user_id, &target);
            match (inside, this.drag_motion.voice_target == claim) {
                (true, false) => {
                    let joining = this.me.as_ref().is_some_and(|me| me.id == e.drag(cx).user_id);
                    let home = this.guilds.iter().flat_map(|g| &g.voice_states).any(|v| v.user_id == e.drag(cx).user_id && v.channel_id.as_ref() == Some(&target));
                    this.drag_motion.voice_target = claim;
                    this.drag_motion.set_tag((!home).then(|| format!("{} {name}", if joining { "Join" } else { "Move to" }).into()));
                }
                (false, true) => {
                    this.drag_motion.voice_target = None;
                    this.drag_motion.set_tag(None);
                }
                _ => return,
            }
            cx.notify();
        }))
        .on_drop(cx.listener(move |this, drag: &crate::drag::VoiceDrag, _, cx| this.drop_voice_user(drag, drop_target.clone(), cx)))
    }

    // Held over this voice channel by someone from elsewhere: it lights up and opens a slot.
    fn welcomes_voice_drop(&self, channel: &Channel, guild: &Guild) -> bool {
        let targeted = self.drag_motion.voice_target.as_ref().is_some_and(|(id, _)| *id == channel.id);
        targeted && !guild.voice_states.iter().any(|v| v.channel_id.as_ref() == Some(&channel.id) && self.drag_motion.row(&format!("voice-{}", v.user_id)) == RowDrag::Hole)
    }

    // Who a carried voice channel takes along, as its ghost shows them.
    fn voice_people(&self, channel: &Channel, guild: &Guild) -> Vec<(String, SharedString)> {
        guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .map(|v| {
                let avatar = v.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&v.user_id, None));
                let name = v.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&v.user_id));
                (avatar, name.into())
            })
            .collect()
    }

    // Leaving a streaming row toward its card holds the card open for the tunnel's grace.
    fn hover_stream_row(&mut self, key: &str, hovered: bool, at: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut Context<Self>) {
        if !hovered && self.stream_tunnel.heading_to(at, self.stream_card_bounds.get()) {
            self.stream_hold = Some(key.to_owned());
            let key = key.to_owned();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(ui::TUNNEL_GRACE).await;
                this.update(cx, |this, cx| {
                    if this.stream_hold.as_deref() == Some(key.as_str()) {
                        this.stream_hold = None;
                        this.set_hovered(&key, false, cx);
                    }
                })
                .ok();
            })
            .detach();
            return;
        }
        self.stream_hold = None;
        self.set_hovered(key, hovered, cx);
    }

    // Rows show their hover-only controls from this, not from `group_hover`: gpui resolves group hover
    // only while painting, so a `display` toggled by it paints children that were never prepainted.
    fn track_row_hover(&self, row: Stateful<Div>, id: &str, prefetch: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = id.to_owned();
        row.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            match *hovered {
                true if prefetch => this.prefetch(&id, cx),
                true => {}
                false if this.hovered_row.as_deref() != Some(id.as_str()) => return,
                false => {}
            }
            this.hovered_row = hovered.then(|| id.clone());
            cx.notify();
        }))
    }

    // Open Chat (voice channels you may join), invite and options, shown in the row's meta slot while
    // it's hovered or selected.
    fn row_actions(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Div {
        let can_chat = channel.kind == ChannelKind::Voice && self.can_in(&channel.id, crate::discord::perm::CONNECT);
        let open_chat = can_chat.then(|| {
            let open = channel.clone();
            row_action(SharedString::from(format!("open-chat-{}", channel.id)), Icon::new(Glyph::ChatRound), "Open Chat", cx).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_call_chat(open.clone(), window, cx);
            }))
        });
        let (gid, cid) = (guild.id.clone(), channel.id.clone());
        let (menu_channel, menu_guild) = (channel.clone(), guild.id.clone());
        let invite = row_action(SharedString::from(format!("invite-{}", channel.id)), Icon::new(Glyph::GroupPlus), "Invite people", cx)
            .on_click(cx.listener(move |this, _, window, cx| this.show_invite(gid.clone(), cid.clone(), window, cx)));
        let options = row_action(SharedString::from(format!("options-{}", channel.id)), Icon::new(Glyph::Settings), "Edit channel", cx).on_mouse_down(
            gpui_kit::MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_channel_settings(menu_channel.clone(), menu_guild.clone(), window, cx);
            }),
        );
        div()
            .flex_none()
            .flex()
            .gap(px(2.))
            .children(open_chat)
            .child(invite)
            .child(options)
    }

    // Who is in a voice channel: 30px rows under it, the mini wave for whoever speaks, then one mark.
    fn voice_rows(&self, channel: &Channel, guild: &Guild, in_call: bool, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let speaking = self.call.as_ref().filter(|_| in_call).map(|c| c.speaking.clone()).unwrap_or_default();
        let watching: Vec<String> = self.call.as_ref().map(|c| c.watching.iter().map(|w| w.streamer_id.clone()).collect()).unwrap_or_default();
        let connected = self.call.as_ref().is_some_and(|c| c.connected && in_call);
        let can_drag_people = self.can_in(&channel.id, crate::discord::perm::MOVE_MEMBERS);
        let (mut present, audience): (Vec<&crate::discord::VoiceState>, Vec<_>) = guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id.as_ref() == Some(&channel.id))
            .partition(|v| channel.raw_type != STAGE || v.on_stage() == OnStage::Speaker);
        // Whoever is streaming leads the list; the rest keep their order (a stable sort).
        present.sort_by_key(|v| !v.streaming);
        let avatar_of = |v: &crate::discord::VoiceState| v.user.as_ref().map(|u| u.avatar.clone()).unwrap_or_else(|| crate::discord::avatar_url(&v.user_id, None));
        if self.hidden_voice_names.contains(&channel.id) && !present.is_empty() {
            let sidebar = crate::material::sidebar_fill(cx);
            return vec![div().h(px(30.)).pl(px(32.)).flex().items_center().child(ui::avatar_stack(present.iter().map(|v| avatar_of(v)), 22., sidebar)).into_any_element()];
        }
        let (hover, selected, text) = (palette::hover(cx), palette::selected(cx), palette::text(cx));
        let mut rows = Vec::new();
        let me = self.me.as_ref().map(|m| m.id.clone());
        let elsewhere = |user_id: &str| me.as_deref() == Some(user_id) && !in_call;
        for state in present {
            let name = state.user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| self.user_name(&state.user_id));
            let avatar = avatar_of(state);
            let talking = speaking.contains(&state.user_id);
            let is_watched = watching.contains(&state.user_id);
            let hover_key = stream_hover_key(&state.user_id);
            // Open while the row or the card itself is hovered, so the card's button can be reached.
            let card_open = self.hovered.as_deref() == Some(hover_key.as_str()) || self.stream_card_hover.as_deref() == Some(hover_key.as_str());
            let stream_card = (state.streaming && card_open).then(|| {
                let stream_key = format!("guild:{}:{}:{}", guild.id, channel.id, state.user_id);
                self.stream_card(Streamer { id: &state.user_id, name: &name, avatar: &avatar }, channel, &stream_key, connected, window, cx).into_any_element()
            });
            let stream_hover = cx.listener(move |this, hovered: &bool, window, cx| this.hover_stream_row(&hover_key, *hovered, window.mouse_position(), cx));
            let end = voice_mark_for(state, cx);
            let game = self.playing(&state.user_id, cx).map(|game| {
                div()
                    .id(SharedString::from(format!("voice-game-{}", state.user_id)))
                    .flex_none()
                    .child(img(game.icon.clone()).size(px(16.)).squircle(4.))
                    .tooltip(move |window, cx| Tooltip::new(game.label()).build(window, cx))
            });
            let key = SharedString::from(format!("voice-{}", state.user_id));
            let carried = self.drag_motion.row(&key);
            let row = div().id(key.clone()).h(px(30.)).flex_none().squircle(ui::RADIUS_SM).children(self.drag_motion.spot(&key));
            let row = self.voice_drop_zone(row, key.clone(), channel, cx);
            if carried == RowDrag::Hole {
                rows.push(hole(row, cx).into_any_element());
                continue;
            }
            let row = match can_drag_people || self.me.as_ref().is_some_and(|m| m.id == state.user_id) {
                true => {
                    let drag = crate::drag::VoiceDrag { user_id: state.user_id.clone(), guild_id: guild.id.clone(), name: name.clone().into(), avatar: avatar.clone() };
                    let look = crate::drag::Look::Person { avatar: drag.avatar.clone(), name: drag.name.clone() };
                    row.on_drag(drag, crate::drag::pick_up(cx.weak_entity(), key, look, point(px(self.channels_width - 16.), px(30.)), 1.05, 0., false))
                }
                false => row,
            };
            rows.push(
                row.when(carried == RowDrag::Hidden, |r| r.invisible())
                    .pl(px(32.))
                    .pr_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if talking { palette::strong(cx) } else { palette::muted(cx) })
                    .hover(move |s| s.bg(hover).text_color(text))
                    // You, in this call from another client (not this app): avatar and name faded, as Discord does.
                    .child(speaking_avatar(&avatar, talking, cx).when(elsewhere(&state.user_id), |a| a.opacity(0.45)))
                    .child(div().flex_1().min_w_0().truncate().when(elsewhere(&state.user_id), |n| n.opacity(0.45)).child(name.clone()))
                    .children(game.map(|g| g.when(elsewhere(&state.user_id), |g| g.opacity(0.45))))
                    .children(end)
                    .on_mouse_down(gpui_kit::MouseButton::Right, {
                        let user_id = state.user_id.clone();
                        cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.voice_user_menu(&user_id, e.position, cx))
                    })
                    .when(state.streaming, |r| r.on_hover(stream_hover).on_mouse_move(cx.listener(|this, e: &gpui_kit::MouseMoveEvent, _, _| this.stream_tunnel.track(e.position))).children(stream_card.map(|card| div().absolute().top_0().right(px(4.)).child(gpui_kit::deferred(gpui_kit::anchored().snap_to_window_with_margin(px(8.)).child(card))))))
                    .when(is_watched, |r| r.bg(selected))
                    // A click opens their profile beside the channel list, as Discord's; a stream is watched from
                    // the preview its row shows on hover.
                    .cursor_pointer()
                    .on_click({
                        let (user_id, x) = (state.user_id.clone(), crate::layout::RAIL_WIDTH + self.channels_width + 8.);
                        cx.listener(move |this, e: &gpui_kit::ClickEvent, _, cx| this.open_voice_profile(&user_id, point(px(x), e.position().y - px(40.)), cx))
                    })
                    .into_any_element(),
            );
        }
        if !audience.is_empty() {
            rows.push(audience_row(audience.len(), cx).into_any_element());
        }
        // .vd-slot: where the person held over this channel will sit.
        if self.welcomes_voice_drop(channel, guild) {
            rows.push(div().h(px(30.)).ml(px(32.)).flex_none().squircle(ui::RADIUS_SM).border(px(1.5)).border_color(palette::accent()).bg(palette::accent_soft(cx)).into_any_element());
        }
        rows
    }

    // With Manage Channels, a channel can be dragged above or below another, or onto a category.
    fn channel_draggable(&self, row: Stateful<Div>, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Stateful<Div> {
        if !self.can_arrange_channels(&guild.id) {
            return row;
        }
        let drag = crate::drag::ChannelDrag { channel_id: channel.id.clone(), guild_id: guild.id.clone() };
        let people = match channel.kind {
            ChannelKind::Voice => self.voice_people(channel, guild),
            _ => Vec::new(),
        };
        // The ghost is the row and its people, 30px each under it 1px apart.
        let size = point(px(self.channels_width - 16.), px(row_height() + people.len() as f32 * 31.));
        let look = crate::drag::Look::Channel { channel: Box::new(channel.clone()), people };
        let ghost = crate::drag::pick_up(cx.weak_entity(), channel.id.clone().into(), look, size, 1.05, 0., true);
        let row = row.on_drag(drag, Board::lift(cx.weak_entity(), channel_board, channel.id.clone(), ghost));
        Board::target(row, channel_board, channel_list(self.guild().and_then(|g| g.channels.iter().find(|c| Some(&c.id) == channel.parent_id.as_ref()))), channel.id.clone(), |_: &crate::drag::ChannelDrag| false, cx)
    }

    // "Move to Archive": the tag a channel carries over a row in another category.
    fn category_move(&self, moved: &str, over: &str) -> Option<SharedString> {
        let channels = &self.guild()?.channels;
        let parent_of = |id: &str| channels.iter().find(|c| c.id == id).and_then(|c| c.parent_id.clone());
        let to = parent_of(over)?;
        if parent_of(moved).as_ref() == Some(&to) {
            return None;
        }
        let name = &channels.iter().find(|c| c.id == to)?.name;
        Some(format!("Move to {name}").into())
    }

    // The threads you've joined (and the one you have open), hung under their channel on a hairline
    // tree: each draws its branch from the row above, and all but the last carry the trunk on.
    fn thread_rows(&self, channel: &Channel, guild: &Guild, cx: &mut Context<Self>) -> Vec<ChannelRow> {
        let open = self.channel.as_ref().map(|c| c.id.clone());
        let threads: Vec<&Channel> = guild
            .threads
            .iter()
            .filter(|t| t.parent_id.as_ref() == Some(&channel.id))
            .filter(|t| open.as_ref() == Some(&t.id) || t.thread.as_ref().is_some_and(|m| m.joined && !m.archived))
            .collect();
        let count = threads.len();
        let line = match crate::material::active() {
            true => palette::faint(cx).opacity(0.5),
            false => ui::mix(palette::faint(cx), palette::surface(palette::Level::Sidebar, cx), 0.5),
        };
        threads
            .into_iter()
            .enumerate()
            .map(|(i, thread)| {
                let selected = open.as_ref() == Some(&thread.id);
                let unread = self.unread(&thread.id);
                let target = thread.clone();
                let (menu_thread, guild_id) = (thread.clone(), guild.id.clone());
                // The first branch starts just below its parent, never touching the parent's fill.
                let gap = if i == 0 { -4. } else { row_gap() };
                let branch = div().absolute().left(px(16.)).top(px(-gap)).w(px(12.)).h(px(THREAD_HEIGHT / 2. + gap)).border_l(px(1.5)).border_b(px(1.5)).rounded_bl(px(8.)).border_color(line);
                let trunk = (i + 1 < count).then(|| div().absolute().left(px(16.)).top(px(THREAD_HEIGHT / 2. - 8.)).bottom_0().w(px(1.5)).bg(line));
                let state = RowState { selected, unread, muted: false, strong: false };
                let row = channel_row(SharedString::from(format!("thread-{}", thread.id)), format!("thread-{}", thread.id).into(), state, cx)
                    .h(px(THREAD_HEIGHT))
                    .pl(px(34.))
                    .child(branch)
                    .children(trunk)
                    .child(ui::truncating(format!("thread-name-{}", thread.id), thread.name.clone()).flex_1())
                    .on_click(cx.listener(move |this, _, window, cx| this.open_channel(target.clone(), window, cx)))
                    .on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_thread_menu(menu_thread.clone(), guild_id.clone(), e.position, cx)));
                ChannelRow { keep: selected || unread, element: row.into_any_element() }
            })
            .collect()
    }

    // A conversation: the person, then one line on what they're doing.
    fn dm_row(&self, channel: &Channel, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.channel.as_ref().is_some_and(|c| c.id == channel.id);
        let unread = self.unread(&channel.id);
        let mentions = self.mention_count(&channel.id);
        let presence = channel.recipient.as_ref().map(|u| self.presences.get(&u.id).cloned().unwrap_or_default());
        let status = presence.as_ref().map(|p| p.status);
        let offline = status == Some(Status_::Offline);
        let ring = crate::material::sidebar_fill(cx);
        let face = match &channel.recipient {
            Some(user) => avatar_with_status(&user.avatar, 32., presence.as_ref().map(|p| p.mark()), ring),
            None => ui::dm_face(&channel.face(), 32., ring),
        };
        let typing = self.typing.get(&channel.id).is_some_and(|t| t.iter().any(|t| t.until > std::time::Instant::now()));
        let faint = palette::faint(cx);
        let call_line = self.dm_call_line(&channel.id, cx);
        let line: Option<Div> = match (typing, presence) {
            (true, _) => Some(div().child(ui::bars(SharedString::from(format!("dm-typing-{}", channel.id)), faint, 10.)).child("Typing…")),
            (false, Some(p)) => match (p.activity, p.status) {
                (Some(a), _) => Some(div().children(activity_glyph(a.kind).map(|i| i.size(px(13.)).flex_none())).child(div().truncate().child(a.text))),
                (None, Status_::Offline) => None,
                (None, s) => Some(div().child(s.label())),
            },
            // A group always counts us and at least one other, so never "1 member".
            (false, None) => (!channel.recipients.is_empty()).then(|| div().child(format!("{} members", channel.recipients.len() + 1))),
        };
        let line = call_line.map(|(glyph, label, ink)| div().child(Icon::new(glyph).size(px(14.)).flex_none().text_color(ink)).child(div().text_color(ink).child(label))).or(line);
        let state = RowState { selected, unread, muted: false, strong: false };
        let (open, close_id) = (channel.clone(), channel.id.clone());
        let group: SharedString = format!("dm-{}", channel.id).into();
        let hovered = self.hovered_row.as_deref() == Some(channel.id.as_str());
        // Hovered, the close button takes the count's place.
        let close = row_action(SharedString::from(format!("dm-close-{}", channel.id)), Nav::Close.icon(), "Close", cx)
            .flex_none()
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.close_dm(&close_id, cx);
            }));
        let row = channel_row(SharedString::from(channel.id.clone()), group.clone(), state, cx);
        self.track_row_hover(row, &channel.id, true, cx)
            .h(px(44.))
            .gap(px(10.))
            .font_weight(FontWeight::NORMAL)
            .child(div().when(offline, |a| a.opacity(0.5)).child(face))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().font_weight(if unread { FontWeight::MEDIUM } else { FontWeight::NORMAL }).child(channel.name.clone()))
                    .children(line.map(|l| l.flex().items_center().gap_1().min_w_0().text_size(px(12.)).text_color(faint))),
            )
            .when(mentions > 0 && !hovered, |r| r.child(ui::count_badge(mentions)))
            .when(mentions == 0 && !selected && !hovered && self.drafts.has(&channel.id), |r| r.child(div().flex_none().flex().text_color(faint).child(draft_mark())))
            .when(hovered, |r| r.child(close))
            .on_click(cx.listener(move |this, _, window, cx| this.open_channel(open.clone(), window, cx)))
            .on_mouse_down(gpui_kit::MouseButton::Right, {
                let channel = channel.clone();
                cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_dm_menu(channel.clone(), e.position, cx))
            })
            .into_any_element()
    }
}

// CHANNEL LIST

const THREAD_HEIGHT: f32 = 28.;

// A row and whether it stays in view while its category is folded.
struct ChannelRow {
    keep: bool,
    element: AnyElement,
}

#[derive(Clone, Copy)]
pub(crate) struct RowState {
    pub selected: bool,
    pub unread: bool,
    pub muted: bool,
    // Full ink without the unread pill (the voice channel you're in).
    pub strong: bool,
}

// A sidebar row (.ch): muted ink at rest; selected is a fill, unread is bold with the pill on the
// list's edge, muted is faint. `group` names the row for its hover-revealed actions.
pub(crate) fn channel_row(id: SharedString, group: SharedString, state: RowState, cx: &App) -> Stateful<Div> {
    let RowState { selected, unread, muted, strong } = state;
    let (hover, text) = (palette::hover(cx), palette::text(cx));
    let ink = match (selected || unread || strong, muted) {
        (true, _) => palette::strong(cx),
        (false, true) => palette::faint(cx),
        (false, false) => palette::muted(cx),
    };
    div()
        .id(id)
        .group(group)
        .relative()
        .h(px(row_height()))
        .w_full()
        .flex_none()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .squircle(ui::RADIUS_SM)
        .cursor_pointer()
        .font_weight(if unread { FontWeight::MEDIUM } else { FontWeight::NORMAL })
        .text_color(ink)
        .when(selected, |r| r.bg(palette::selected(cx)))
        .when(!selected, |r| r.hover(move |s| s.bg(hover).text_color(text)))
        // Pinned to the name line's centre: a two-line row (a voice status under the name) would drop it to the top.
        .when(unread && !selected, |r| r.child(div().absolute().left(px(-8.)).top(px((row_height() - 8.) / 2.)).w(px(4.)).h(px(8.)).rounded_r(px(4.)).bg(palette::strong(cx))))
}

// A category's header row (.cathead): the label, a button that folds the category unless `static_`,
// and room on its right for the add button. `group` names the row so the add can show on hover.
pub(crate) fn category_head(group: SharedString, label: Stateful<Div>, static_: bool, cx: &App) -> Div {
    let (hover, strong) = (palette::hover(cx), palette::strong(cx));
    div().group(group).relative().flex().items_center().child(
        label
            .flex_1()
            .min_w_0()
            .h(px(28.))
            .px(px(6.))
            .squircle(ui::RADIUS_SM)
            .flex()
            .items_center()
            .gap_1()
            .whitespace_nowrap()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::faint(cx))
            .when(!static_, |l| l.cursor_pointer().hover(move |s| s.bg(hover).text_color(strong))),
    )
}

// The + at a category's end: creates a channel there (or, on Direct messages, starts a conversation).
// With `reveal_on`, it shows only while that header group is hovered.
fn cat_add(id: impl Into<gpui_kit::ElementId>, reveal_on: Option<SharedString>, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    category_action(id, Nav::Plus.icon(), reveal_on, tooltip, cx)
}

// A category head's trailing action: a 24px target with a 14px glyph, so every head's action is one size.
pub(crate) fn category_action(id: impl Into<gpui_kit::ElementId>, icon: Icon, reveal_on: Option<SharedString>, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    div()
        .id(id)
        .absolute()
        .right(px(2.))
        .size(px(24.))
        .squircle(ui::RADIUS_XS)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(strong))
        .when_some(reveal_on, |add, group| add.opacity(0.).group_hover(group, |s| s.opacity(1.)))
        .child(icon.size(px(14.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}

// A 24px action inside a row (.ch .acts button).
fn row_action(id: SharedString, icon: Icon, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let strong = palette::strong(cx);
    div()
        .id(id)
        .size(px(24.))
        .squircle(ui::RADIUS_XS)
        .flex()
        .items_center()
        .justify_center()
        .text_color(palette::muted(cx))
        .hover(move |s| s.text_color(strong))
        .child(icon.size(px(16.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
}

// The pencil on a row whose channel holds an unsent draft.
fn draft_mark() -> Icon {
    Icon::new(Glyph::Pen).size(px(14.))
}

// The type glyph a channel leads with (Discord's raw types: 5 announcement, 13 stage, 16 media); a private
// channel's carries a padlock in its corner.
pub(crate) fn channel_glyph(channel: &Channel) -> Icon {
    if channel.private {
        return Icon::new(match (channel.kind, channel.raw_type) {
            (_, 5) => Glyph::AnnouncementLocked,
            (_, 13) => Glyph::StageLocked,
            (_, 16) => Glyph::MediaLocked,
            (ChannelKind::Voice, _) => Glyph::VoiceLocked,
            (ChannelKind::Forum, _) => Glyph::ForumLocked,
            _ => Glyph::TextLocked,
        });
    }
    match (channel.kind, channel.raw_type) {
        (_, 5) => Nav::Megaphone.icon(),
        (_, 13) => Nav::Stage.icon(),
        (_, 16) => Nav::Media.icon(),
        (ChannelKind::Voice, _) => Icon::new(Glyph::VolumeHigh),
        (ChannelKind::Forum, _) => Icon::new(Glyph::Forum),
        _ => Icon::new(Glyph::Hashtag),
    }
}

// The channel's name; a live stage's topic is its second line, as Discord's.
fn channel_label(channel: &Channel, topic: Option<String>, cx: &App) -> AnyElement {
    let name = ui::truncating(format!("channel-name-{}", channel.id), channel.name.clone()).flex_1();
    let Some(topic) = topic else { return name.into_any_element() };
    let topic = div().text_size(px(12.)).line_height(px(16.)).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).truncate().child(topic);
    div().flex_1().min_w_0().flex().flex_col().child(name).child(topic).into_any_element()
}

// A stage lists only its speakers; everyone else folds into one row, as Discord's does.
fn audience_row(count: usize, cx: &App) -> Div {
    div()
        .h(px(30.))
        .flex_none()
        .pl(px(35.))
        .pr_2()
        .flex()
        .items_center()
        .gap_2()
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::muted(cx))
        .child(Icon::new(Glyph::Headphones).size(px(16.)).flex_none())
        .child(div().flex_1().min_w_0().truncate().child(format!("{count} in audience")))
}

// A voice occupant's avatar; while they speak, a green ring sits 1px clear of it, as Discord's does.
fn speaking_avatar(avatar: &str, talking: bool, cx: &App) -> Div {
    div()
        .relative()
        .flex_none()
        .size(px(22.))
        .child(ui::avatar(avatar.to_owned(), 22.))
        .when(talking, |a| a.child(div().absolute().top(px(-3.)).left(px(-3.)).size(px(28.)).rounded_full().border(px(2.)).border_color(palette::status_in(crate::discord::Status::Online, cx))))
}

// What a voice occupant's row shows at its end, as Discord's: their mute and deafen (server ones red),
// then camera, then LIVE. Speaking shows as a ring on the avatar instead.
#[derive(Clone, Copy, Debug, PartialEq)]
enum VoiceMark {
    ServerMuted,
    ServerDeafened,
    Live,
    Camera,
    Muted,
    Deafened,
}

// Deafening yourself also mutes you, so a self-deafen shows both marks, as Discord's list does; a
// server deafen alone shows only its own, and a server mute and deafen together show both.
fn voice_marks(state: &crate::discord::VoiceState) -> Vec<VoiceMark> {
    // The mic slot and the headphones slot fill on their own, so a server mute shows beside a server
    // deafen. `muted` and `deafened` include the server's flags, so only a self-deafen fills the mic slot.
    let mic = match (state.server_muted, state.muted || (state.deafened && !state.server_deafened)) {
        (true, _) => Some(VoiceMark::ServerMuted),
        (false, true) => Some(VoiceMark::Muted),
        _ => None,
    };
    let ears = match (state.server_deafened, state.deafened) {
        (true, _) => Some(VoiceMark::ServerDeafened),
        (false, true) => Some(VoiceMark::Deafened),
        _ => None,
    };
    let mut marks: Vec<VoiceMark> = mic.into_iter().chain(ears).collect();
    if state.camera {
        marks.push(VoiceMark::Camera);
    }
    if state.streaming {
        marks.push(VoiceMark::Live);
    }
    marks
}

fn voice_mark_for(state: &crate::discord::VoiceState, cx: &App) -> Option<AnyElement> {
    let marks = voice_marks(state);
    if marks.is_empty() {
        return None;
    }
    let (faint, danger) = (palette::faint(cx), palette::danger_ink(cx));
    let mark = |icon: Icon, color: Hsla| div().flex_none().flex().text_color(color).child(icon.size(px(16.))).into_any_element();
    let elements = marks.into_iter().map(|m| match m {
        VoiceMark::ServerMuted => mark(Nav::MicOffFill.icon(), danger),
        VoiceMark::ServerDeafened => mark(Nav::DeafenedFill.icon(), danger),
        VoiceMark::Live => ui::live_badge().h(px(18.)).px(px(6.)).text_size(px(11.)).into_any_element(),
        VoiceMark::Camera => mark(Nav::CameraFill.icon(), faint),
        VoiceMark::Muted => mark(Nav::MicOffFill.icon(), faint),
        VoiceMark::Deafened => mark(Nav::DeafenedFill.icon(), faint),
    });
    Some(div().flex_none().flex().items_center().gap(px(6.)).children(elements).into_any_element())
}

// The DM list's header: a field-shaped button that opens the quick switcher.
fn dm_find(cx: &App) -> Stateful<Div> {
    ui::search_button("dm-find", "Find or start a conversation", cx).occlude()
}

pub(crate) fn channel_sections(guild: &Guild, me: &str) -> Vec<(Option<Channel>, Vec<Channel>)> {
    let visible = |c: &&Channel| matches!(c.kind, ChannelKind::Text | ChannelKind::Voice | ChannelKind::Forum) && guild.can_view(me, c);
    let uncategorised: Vec<Channel> = guild.channels.iter().filter(visible).filter(|c| c.parent_id.is_none()).cloned().collect();
    let mut sections = vec![(None, uncategorised)];
    for category in guild.channels.iter().filter(|c| c.kind == ChannelKind::Category) {
        let children = guild.channels.iter().filter(visible).filter(|c| c.parent_id.as_ref() == Some(&category.id)).cloned().collect();
        sections.push((Some(category.clone()), children));
    }
    sections.retain(|(_, channels)| !channels.is_empty());
    sections
}

pub fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|w| w.chars().next()).take(3).collect()
}

// RAIL

// A folder's trip this frame: 0 closed, 1 open.
struct FolderMotion {
    trip: f32,
}

impl FolderMotion {
    fn frame(key: &str, open: bool, window: &mut Window, cx: &mut App) -> Self {
        let id = SharedString::from(key.to_owned());
        let trip = transition((id, "trip"), if open { 1. } else { 0. }, Tween::new(TRIP).ease(ease_out()), window, cx);
        Self { trip }
    }
}

// A rail tile's state pill: full when selected, longer on hover, a dot when unread.
fn pill_height(selected: bool, hovered: bool, unread: bool) -> f32 {
    match (selected, hovered, unread) {
        (true, _, _) => 36.,
        (_, true, _) => 20.,
        (_, _, true) => 8.,
        _ => 0.,
    }
}

// A folder server's marks, which ride its tile on the trip.
struct Badge {
    pill: f32,
    mentions: u32,
}

// --ease-out: cubic-bezier(0.23, 1, 0.32, 1).
fn ease_out() -> impl Fn(f32) -> f32 + 'static {
    gpui_kit::base::animation::cubic_bezier(0.23, 1., 0.32, 1.)
}

// Hover and state changes: --dur-hover on --ease-out.
pub(crate) fn hover_ease() -> Transition {
    Transition::Tween(ui::DUR_HOVER, Easing::Bezier(Bezier::new(0.23, 1., 0.32, 1.).expect("monotonic bezier")))
}

// Where an open folder's `i`th server sits, from the folder's top.
fn slot_y(i: usize) -> f32 {
    (i + 1) as f32 * (TILE + rail_gap())
}

// A server on its way between its mini tile and its slot. Past the fourth, servers have no mini and
// grow out of the last one. Its pill starts as the folder's (`head_pill`, on the folder's row) and slides
// with it to its own height; its count grows with the tile, so both land together.
#[allow(clippy::too_many_arguments)]
fn flying_tile(mini: &Mini, badge: &Badge, head_pill: f32, i: usize, total: usize, t: f32, rail: Hsla, cx: &App) -> AnyElement {
    let (x, y) = mini_slot(i.min(3), total.min(4));
    let from = (TILE_X + x, y, if i < 4 { MINI } else { 0. });
    let to = (TILE_X, slot_y(i), TILE);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let size = lerp(from.2, to.2);
    let face = match &mini.icon {
        Some(url) => img(url.clone()).size(px(size)).squircle(lerp(ui::RADIUS_XS, ui::RADIUS_LG)).into_any_element(),
        None => letters_tile(&mini.letters, size, lerp(ui::RADIUS_XS, ui::RADIUS_LG), cx).into_any_element(),
    };
    let tile = div()
        .absolute()
        .left(px(lerp(from.0, to.0)))
        .top(px(lerp(from.1, to.1)))
        .size(px(size))
        .child(face)
        .when(badge.mentions > 0, |tile| tile.child(count_chip(badge.mentions, t, rail).absolute().right(px(-5. - COUNT_RING)).bottom(px(-4. - COUNT_RING))));
    let pill = (badge.pill > 0.).then(|| {
        let height = lerp(head_pill, badge.pill);
        let center = lerp(TILE / 2., slot_y(i) + TILE / 2.);
        div().absolute().left_0().top(px(center - height / 2.)).w(px(4.)).h(px(height)).rounded_r(px(4.)).bg(palette::strong(cx))
    });
    div().absolute().inset_0().children(pill).child(tile).into_any_element()
}

pub(crate) fn server_face(guild: &Guild) -> Face {
    match &guild.icon {
        Some(url) => Face::Icon(url.clone()),
        None => Face::Letters(initials(&guild.name)),
    }
}

fn mini(guild: &Guild) -> Mini {
    Mini { icon: guild.icon.clone(), letters: initials(&guild.name) }
}

// A tile's face at `size`. `open` trades a folder's minis for the folder glyph.
pub(crate) fn render_face(face: Face, size: f32, radius: f32, selected: bool, open: f32, cx: &App) -> AnyElement {
    let tile = || div().size(px(size)).flex_none().squircle(radius).flex().items_center().justify_center();
    match face {
        Face::Icon(url) => img(url).size(px(size)).flex_none().squircle(radius).into_any_element(),
        Face::Person(face) => ui::dm_face(&face, size, crate::material::sidebar_fill(cx)),
        Face::Letters(label) => letters_tile(&label, size, radius, cx).into_any_element(),
        Face::Home => tile()
            .bg(if selected { palette::accent() } else { crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx) })
            .child(svg().path(crate::icons::MARK).size(px(size / 2.)).text_color(if selected { palette::on_accent() } else { palette::logo_ink(cx) }))
            .into_any_element(),
        Face::Folder { tint, minis } => tile()
            .relative()
            .bg(tint.opacity(0.24 * (1. - open)))
            .child(div().absolute().inset_0().flex().items_center().justify_center().opacity(open).text_color(tint).child(Icon::new(Glyph::Folder).size(px(22.))))
            .when(open <= 0., |t| t.child(minis_grid(minis, cx)))
            .into_any_element(),
    }
}

// A closed folder's first four servers, laid out for how many there are (no empty slots).
fn minis_grid(minis: Vec<Mini>, cx: &App) -> Div {
    let shown = minis.len().min(4);
    let cells = minis.into_iter().take(4).enumerate().map(|(i, mini)| {
        let (x, y) = mini_slot(i, shown);
        let face = match mini {
            Mini { icon: Some(url), .. } => img(url).size(px(MINI)).squircle(ui::RADIUS_XS).into_any_element(),
            Mini { letters, .. } => letters_tile(&letters, MINI, ui::RADIUS_XS, cx).into_any_element(),
        };
        div().absolute().left(px(x)).top(px(y)).child(face)
    });
    div().absolute().inset_0().children(cells)
}

// Where mini `i` of `shown` (1..=4) sits inside the folder tile: one centred, two side by side,
// three as two over one, four as a 2x2 grid.
fn mini_slot(i: usize, shown: usize) -> (f32, f32) {
    let (centre, step) = ((TILE - MINI) / 2., MINI + MINI_GAP);
    match (shown, i) {
        (0 | 1, _) => (centre, centre),
        (2, i) => (MINI_INSET + i as f32 * step, centre),
        (3, 2) => (centre, MINI_INSET + step),
        (_, i) => (MINI_INSET + (i % 2) as f32 * step, MINI_INSET + (i / 2) as f32 * step),
    }
}

// A server with no icon: its initials on the sidebar tone. Minis keep only the first letter.
fn letters_tile(label: &str, size: f32, radius: f32, cx: &App) -> Div {
    let label: String = if size < 24. { label.chars().take(1).collect() } else { label.to_owned() };
    div()
        .size(px(size))
        .flex_none()
        .squircle(radius)
        .bg(crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx))
        .text_color(palette::strong(cx))
        // Minis keep a readable letter rather than scaling the tile's 14px down to nothing.
        .text_size(px((14. * size / TILE).max(8.)))
        .font_weight(FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(label)
}

// The hairline between home, the servers and the actions.
// Add and Discover: a quiet tile that lifts to the raised tone on hover.
// A server's hover card, as Discord's: its name, then who's talking in voice and who's live.
const PREVIEW_FACES: usize = 6;

#[derive(Clone)]
struct ServerPreview {
    name: SharedString,
    voice: Vec<ui::Entry>,
    live: Vec<ui::Entry>,
    muted: bool,
}

impl ServerPreview {
    fn has_activity(&self) -> bool {
        !self.voice.is_empty() || !self.live.is_empty()
    }
}

impl Render for ServerPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = palette::surface(palette::Level::Overlay, cx);
        let muted = palette::muted(cx);
        let (wash, strong) = (palette::selected(cx), palette::strong(cx));
        // Six slots at most per row, the last a "+n" when they don't all fit; the card stays a glance, not a roster.
        let row = |key: &'static str, title: &'static str, glyph: Glyph, people: &[ui::Entry]| {
            div().flex().items_center().gap(px(8.)).child(Icon::new(glyph).size(px(16.)).text_color(muted)).child(ui::listed_stack(key, title, people.to_vec(), PREVIEW_FACES, 24., surface, wash, strong))
        };
        // Anchored flush to the tile; plain tooltips get their 12px gap from Tooltip's own margin.
        div().pl(px(12.)).child(ui::surface(cx)
            .max_w(px(280.))
            .p(px(12.))
            .gap(px(8.))
            .child(div().truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(self.name.clone()))
            .when(!self.voice.is_empty(), |c| c.child(row("server-preview-voice", "In voice", Glyph::VolumeHigh, &self.voice)))
            .when(!self.live.is_empty(), |c| c.child(row("server-preview-live", "Live", Glyph::ShareScreen, &self.live)))
            .when(self.muted, |c| c.child(div().text_size(px(12.)).text_color(muted).child("Muted"))))
    }
}

fn rail_action(id: &'static str, icon: Icon, tooltip: &'static str, cx: &App) -> Stateful<Div> {
    let (raised, strong) = (crate::material::lifted(palette::surface(palette::Level::Raised, cx), cx), palette::strong(cx));
    div()
        .id(id)
        .size(px(TILE))
        .flex_none()
        .squircle(ui::RADIUS_LG)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(raised)
        .text_color(palette::muted(cx))
        .hover(move |s| s.bg(raised).text_color(strong))
        .child(icon.size(px(20.)))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .tooltip_show_delay(Duration::ZERO)
        .tooltip_placement(TooltipPlacement::Side)
}

// The mention count on a tile's corner, ringed in the rail so it reads apart from the art.
fn rail_count(count: u32, scale: f32, rail: Hsla) -> Div {
    count_chip(count, scale, rail).absolute().right(px(RAIL_WIDTH - TILE_X - TILE - 5. - COUNT_RING)).bottom(px(-4. - COUNT_RING))
}

const COUNT_RING: f32 = 3.;

// The count itself, unplaced; `scale` grows it in with a folder's trip.
fn count_chip(count: u32, scale: f32, rail: Hsla) -> Div {
    let chip = div().p(px(COUNT_RING)).rounded_full();
    // The ring's cutout erases the glass behind it at full size whatever the scale or opacity, so while the
    // count grows or shrinks it goes without, or an empty hole is left where it was.
    let chip = if scale >= 0.999 { ui::cut_out(chip, rail) } else { chip };
    chip.scale(scale.max(0.))
        .opacity(scale.clamp(0., 1.))
        .child(ui::count_badge(count).min_w(px(18.)).h(px(18.)).px(px(5.)).text_size(px(12.)))
}

// What a server's icon badge shows, as Discord's GuildMediaStateStore decides it; the highest wins.
// GAP: Discord's stage, event and activity badges outrank these; we don't track those yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MediaKind {
    Voice,
    Video,
    Screen,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Media {
    kind: MediaKind,
    // You're in this voice: Discord's connected (green) style.
    connected: bool,
}

// The badge for one server's voice states. `connected` is the channel you're in there, if any.
fn media_state(states: &[crate::discord::VoiceState], me: &str, connected: Option<&str>, muted_server: bool, counts: impl Fn(&crate::discord::VoiceState) -> bool) -> Option<Media> {
    let pick = |screen: bool, video: bool, audio: bool| match (screen, video, audio) {
        (true, ..) => Some(MediaKind::Screen),
        (_, true, _) => Some(MediaKind::Video),
        (.., true) => Some(MediaKind::Voice),
        _ => None,
    };
    if let Some(channel) = connected {
        // Yours: your own stream, anyone's camera in your channel, or just the call.
        let screen = states.iter().any(|s| s.user_id == me && s.streaming);
        let video = states.iter().any(|s| s.channel_id.as_deref() == Some(channel) && s.camera);
        return pick(screen, video, true).map(|kind| Media { kind, connected: true });
    }
    if muted_server {
        return None;
    }
    let live: Vec<_> = states.iter().filter(|s| counts(s)).collect();
    pick(live.iter().any(|s| s.streaming), live.iter().any(|s| s.camera), !live.is_empty()).map(|kind| Media { kind, connected: false })
}

// The badge on a tile's top-right corner, ringed in the rail like the count below it.
fn media_badge(media: Media, rail: Hsla, cx: &App) -> Div {
    const RING: f32 = 3.;
    let icon = match media.kind {
        MediaKind::Voice => Icon::new(Glyph::VolumeHigh),
        MediaKind::Video => Nav::CameraFill.icon(),
        MediaKind::Screen => Icon::new(Glyph::ShareScreen),
    };
    // Dark with white ink in both themes, as Discord draws it; green while you're connected.
    let fill = if media.connected { palette::status_in(Status_::Online, cx) } else { gpui_kit::hsla(0., 0., 0.08, 1.) };
    ui::cut_out(div().absolute().right(px(RAIL_WIDTH - TILE_X - TILE - 4. - RING)).top(px(-4. - RING)).p(px(RING)).rounded_full(), rail).child(
        div().size(px(20.)).rounded_full().bg(fill).flex().items_center().justify_center().text_color(gpui_kit::white()).child(icon.size(px(12.))),
    )
}

// Going onto a tile (making or joining a folder): a ring around it. Reordering shows as the rail making room.
fn combine_ring_mark() -> Div {
    div().absolute().left(px(TILE_X - 4.)).top(px(-4.)).size(px(TILE + 8.)).squircle(ui::RADIUS_LG + 4.).border_2().border_color(palette::accent())
}

// The rail's own sortable list, and a folder's.
const RAIL_LIST: &str = "rail";

fn folder_list(id: &str) -> SharedString {
    format!("folder-list-{id}").into()
}

fn rail_board(waffle: &mut Waffle) -> &mut Board {
    &mut waffle.rail_board
}

fn channel_board(waffle: &mut Waffle) -> &mut Board {
    &mut waffle.channel_board
}

// A category's sortable list; channels outside any category share one.
pub(crate) fn channel_list(category: Option<&Channel>) -> SharedString {
    category.map_or_else(|| "channels".into(), |c| format!("category-{}", c.id).into())
}

// ICONS

// MOVE(icons): glyphs the navigation chrome needs that `icons::Glyph` lacks yet (window controls and
// voice marks included); promote each into `Glyph` and drop this.
#[derive(Clone, Copy)]
pub(crate) enum Nav {
    Megaphone,
    Media,
    Stage,
    BellSlash,
    Compass,
    Plus,
    Caret,
    MoreVertical,
    Close,
    MicOffFill,
    DeafenedFill,
    CameraFill,
    GameFill,
    MusicFill,
    TvFill,
    Mic,
    Headphones,
    HeadphonesOff,
    MicOff,
    Settings,
    Minimize,
    Maximize,
    Restore,
}

impl Nav {
    pub(crate) fn icon(self) -> Icon {
        Icon::default().data(self.svg())
    }

    fn svg(self) -> &'static [u8] {
        match self {
            Nav::Megaphone => include_bytes!("../assets/icons/reicon/bullhorn.svg"),
            Nav::Media => include_bytes!("../assets/icons/reicon/gallery.svg"),
            Nav::Stage => include_bytes!("../assets/icons/reicon/podcast.svg"),
            Nav::BellSlash => include_bytes!("../assets/icons/reicon/bell-off2.svg"),
            Nav::Compass => include_bytes!("../assets/icons/reicon/compass.svg"),
            Nav::Plus => include_bytes!("../assets/icons/reicon/add.svg"),
            Nav::Caret => include_bytes!("../assets/icons/reicon/chevron-down.svg"),
            Nav::MoreVertical => include_bytes!("../assets/icons/reicon/more-v.svg"),
            Nav::Close => include_bytes!("../assets/icons/reicon/xmark.svg"),
            Nav::MicOffFill => include_bytes!("../assets/icons/reicon/muted.svg"),
            Nav::DeafenedFill => include_bytes!("../assets/icons/reicon/deafened.svg"),
            Nav::CameraFill => include_bytes!("../assets/icons/reicon/video.svg"),
            Nav::GameFill => include_bytes!("../assets/icons/reicon/gamepad.svg"),
            Nav::MusicFill => include_bytes!("../assets/icons/reicon/music-notes.svg"),
            Nav::TvFill => include_bytes!("../assets/icons/reicon/tv.svg"),
            Nav::Mic => include_bytes!("../assets/icons/reicon/microphone2.svg"),
            Nav::Headphones => include_bytes!("../assets/icons/reicon/headphone.svg"),
            Nav::HeadphonesOff => include_bytes!("../assets/icons/reicon/deafened.svg"),
            Nav::MicOff => include_bytes!("../assets/icons/reicon/muted.svg"),
            Nav::Settings => include_bytes!("../assets/icons/reicon/gear2.svg"),
            Nav::Minimize => include_bytes!("../assets/icons/reicon/minus.svg"),
            Nav::Maximize => include_bytes!("../assets/icons/reicon/win-max.svg"),
            Nav::Restore => include_bytes!("../assets/icons/reicon/win-restore.svg"),
        }
    }
}

// An activity's kind as its filled glyph: what someone is playing, hearing, streaming or watching.
pub(crate) fn activity_glyph(kind: ActivityKind) -> Option<Icon> {
    match kind {
        ActivityKind::Playing | ActivityKind::Competing => Some(Nav::GameFill.icon()),
        ActivityKind::Listening => Some(Nav::MusicFill.icon()),
        // Discord draws a TV for both.
        ActivityKind::Streaming | ActivityKind::Watching => Some(Nav::TvFill.icon()),
        ActivityKind::Custom => None,
    }
}

// A row still loading, names of varying length: a DM's avatar and name, or a channel's icon and name.
fn list_skeleton(i: usize, channels: bool, cx: &App) -> AnyElement {
    let width = [0.62, 0.45, 0.7, 0.52, 0.38, 0.58, 0.48, 0.66][i % 8];
    let mark = match channels {
        true => ui::skeleton(SharedString::from(format!("list-skel-mark-{i}")), 4., cx).size(px(18.)).flex_none(),
        false => ui::skeleton_circle(SharedString::from(format!("list-skel-mark-{i}")), 32., cx),
    };
    div()
        .h(px(if channels { row_height() } else { 42. }))
        .px_2()
        .flex()
        .items_center()
        .gap(px(if channels { 8. } else { 12. }))
        .child(mark)
        .child(ui::skeleton(SharedString::from(format!("list-skel-name-{i}")), 4., cx).h(px(12.)).w(relative(width)))
        .into_any_element()
}

fn muted_line(text: &'static str, cx: &App) -> AnyElement {
    div().px_2().py_2().text_color(cx.theme().muted_foreground).child(text).into_any_element()
}

const STREAM_CARD_WIDTH: f32 = 260.;

struct Streamer<'a> {
    id: &'a str,
    name: &'a str,
    avatar: &'a str,
}

// The row and its card share one hover key, so moving from one onto the other keeps the card open.
fn stream_hover_key(user_id: &str) -> String {
    format!("stream-{user_id}")
}

// "Theo watching", "Theo and Ivy watching", "Theo, Ivy and Sam watching", "Theo, Ivy and 2 others watching".
fn viewer_line(names: &[String]) -> Option<String> {
    let who = match names {
        [] => return None,
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} others", rest.len()),
    };
    Some(format!("{who} watching"))
}

// The shared app and how long it's been live, "Blender · 24 min"; either half may be missing.
fn stream_meta(app: Option<&str>, live: Option<Duration>) -> Option<String> {
    let parts: Vec<String> = app.map(str::to_owned).into_iter().chain(live.map(live_for)).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn live_for(live: Duration) -> String {
    let minutes = live.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "Just started".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

// Unlike Discord, joining leaves the view where it is; the stage opens from a second click.
#[derive(Debug, PartialEq)]
enum VoiceRowClick {
    Join,
    OpenStage,
}

fn voice_row_click(in_call: Option<&str>, clicked: &str) -> VoiceRowClick {
    match in_call == Some(clicked) {
        true => VoiceRowClick::OpenStage,
        false => VoiceRowClick::Join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_minis_fill_the_tile_for_how_many_there_are() {
        let centre = (TILE - MINI) / 2.;
        assert_eq!(mini_slot(0, 1), (centre, centre), "one sits in the middle");
        assert_eq!([mini_slot(0, 2).1, mini_slot(1, 2).1], [centre, centre], "two share the middle row");
        assert_eq!(mini_slot(2, 3).0, centre, "the third of three centres under the pair");
        assert_eq!(mini_slot(3, 4), (MINI_INSET + MINI + MINI_GAP, MINI_INSET + MINI + MINI_GAP), "four fill the grid");
    }

    #[test]
    fn server_badge_follows_discords_media_state() {
        let state = |user: &str, channel: &str, streaming, camera| crate::discord::VoiceState { user_id: user.into(), channel_id: Some(channel.into()), streaming, camera, ..Default::default() };
        let all = |_: &crate::discord::VoiceState| true;
        let kind = |m: Option<Media>| m.map(|m| (m.kind, m.connected));
        assert_eq!(kind(media_state(&[], "me", None, false, all)), None, "an empty server has no badge");
        assert_eq!(kind(media_state(&[state("a", "c", false, false)], "me", None, false, all)), Some((MediaKind::Voice, false)), "people in voice: speaker");
        let busy = [state("a", "c", false, true), state("b", "c", true, false), state("d", "c", false, false)];
        assert_eq!(kind(media_state(&busy, "me", None, false, all)), Some((MediaKind::Screen, false)), "a stream outranks cameras and voice");
        assert_eq!(kind(media_state(&busy[..1], "me", None, false, all)), Some((MediaKind::Video, false)), "a camera outranks voice");
        assert_eq!(kind(media_state(&busy, "me", None, true, all)), None, "a muted server stays quiet");
        assert_eq!(kind(media_state(&busy, "me", None, false, |s| s.user_id == "d")), Some((MediaKind::Voice, false)), "uncounted people (blocked, AFK, hidden) don't light it");
        let mine = [state("me", "c", false, false), state("b", "other", true, false)];
        assert_eq!(kind(media_state(&mine, "me", Some("c"), true, all)), Some((MediaKind::Voice, true)), "connected: green speaker, even muted, and others' streams don't count");
        let streaming = [state("me", "c", true, false)];
        assert_eq!(kind(media_state(&streaming, "me", Some("c"), false, all)), Some((MediaKind::Screen, true)), "your own stream");
    }

    #[test]
    fn mute_and_deafen_marks_fill_their_own_slots() {
        let state = |muted, deafened, server_muted, server_deafened| crate::discord::VoiceState { muted, deafened, server_muted, server_deafened, ..Default::default() };
        assert_eq!(voice_marks(&state(true, true, false, false)), [VoiceMark::Muted, VoiceMark::Deafened]);
        assert_eq!(voice_marks(&state(false, true, false, false)), [VoiceMark::Muted, VoiceMark::Deafened]);
        assert_eq!(voice_marks(&state(true, false, false, false)), [VoiceMark::Muted]);
        // A server mute and deafen arrive as mute and deaf, which also read as muted and deafened here.
        assert_eq!(voice_marks(&state(true, true, true, true)), [VoiceMark::ServerMuted, VoiceMark::ServerDeafened], "both server marks show");
        assert_eq!(voice_marks(&state(false, true, false, true)), [VoiceMark::ServerDeafened], "a server deafen alone isn't a self-mute");
        assert_eq!(voice_marks(&state(true, true, true, false)), [VoiceMark::ServerMuted, VoiceMark::Deafened], "server mute beside a self-deafen");
        assert_eq!(voice_marks(&state(true, false, true, false)), [VoiceMark::ServerMuted]);
        assert!(voice_marks(&state(false, false, false, false)).is_empty());
        let live = crate::discord::VoiceState { muted: true, deafened: true, camera: true, streaming: true, ..Default::default() };
        assert_eq!(voice_marks(&live), [VoiceMark::Muted, VoiceMark::Deafened, VoiceMark::Camera, VoiceMark::Live], "live keeps the mute and deafen marks");
    }

    #[test]
    fn voice_row_joins_first_then_opens_the_stage() {
        assert_eq!(voice_row_click(None, "lounge"), VoiceRowClick::Join);
        assert_eq!(voice_row_click(Some("gaming"), "lounge"), VoiceRowClick::Join);
        assert_eq!(voice_row_click(Some("lounge"), "lounge"), VoiceRowClick::OpenStage);
    }

    #[test]
    fn viewer_line_names_two_then_counts() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(viewer_line(&[]), None);
        assert_eq!(viewer_line(&names(&["Theo"])).as_deref(), Some("Theo watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy"])).as_deref(), Some("Theo and Ivy watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy", "Sam"])).as_deref(), Some("Theo, Ivy and Sam watching"));
        assert_eq!(viewer_line(&names(&["Theo", "Ivy", "Sam", "Ada"])).as_deref(), Some("Theo, Ivy and 2 others watching"));
    }

    #[test]
    fn live_for_drops_zero_units() {
        assert_eq!(live_for(Duration::from_secs(59)), "Just started");
        assert_eq!(live_for(Duration::from_secs(24 * 60 + 59)), "24 min");
        assert_eq!(live_for(Duration::from_secs(2 * 3600)), "2 hr");
        assert_eq!(live_for(Duration::from_secs(3600 + 5 * 60)), "1 hr 5 min");
    }

    #[test]
    fn stream_meta_joins_what_it_has() {
        assert_eq!(stream_meta(Some("Blender"), Some(Duration::from_secs(24 * 60))).as_deref(), Some("Blender · 24 min"));
        assert_eq!(stream_meta(None, Some(Duration::from_secs(24 * 60))).as_deref(), Some("24 min"));
        assert_eq!(stream_meta(Some("Blender"), None).as_deref(), Some("Blender"));
        assert_eq!(stream_meta(None, None), None);
    }
}

// The open server's name as one button that drops the server menu, heading the channel list.
pub(crate) fn server_name_button(name: SharedString, cx: &mut Context<Waffle>) -> Stateful<Div> {
    let hover = palette::hover(cx);
    div()
        .id("server-header")
        .occlude()
        .flex_1()
        .min_w_0()
        .h(px(34.))
        .px_2()
        .squircle(ui::RADIUS_SM)
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .text_size(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::strong(cx))
        .hover(move |s| s.bg(hover))
        .child(ui::truncating("server-name", name).flex_1())
        .child(Nav::Caret.icon().size(px(16.)).text_color(palette::muted(cx)))
        .on_mouse_down(gpui_kit::MouseButton::Left, cx.listener(|this, _: &gpui_kit::MouseDownEvent, _, cx| {
            let Some(guild_id) = this.guild_id.clone() else { return };
            // Drops from the button's bottom edge, at its left.
            let at = gpui_kit::point(px(RAIL_WIDTH + 8.), px(crate::layout::HEADER_HEIGHT - 4.));
            this.open_server_menu(guild_id, at, true, cx);
        }))
}

// `wanted` in its order, with each item of `before` that left put back right after the one it followed.
fn keep_leaving<T>(mut wanted: Vec<T>, before: Vec<T>, same: impl Fn(&T, &T) -> bool) -> Vec<T> {
    let mut at = 0;
    for old in before {
        match wanted.iter().position(|w| same(w, &old)) {
            Some(i) => at = i + 1,
            None => {
                wanted.insert(at, old);
                at += 1;
            }
        }
    }
    wanted
}

#[cfg(test)]
mod rail_dm_tests {
    #[test]
    fn leaving_tiles_hold_their_place() {
        let same = |a: &&str, b: &&str| a == b;
        assert_eq!(super::keep_leaving(vec!["a", "c"], vec!["a", "b", "c"], same), ["a", "b", "c"]);
        assert_eq!(super::keep_leaving(vec!["n", "a"], vec!["a", "b"], same), ["n", "a", "b"]);
        assert_eq!(super::keep_leaving(vec![], vec!["a", "b"], same), ["a", "b"]);
    }
}
