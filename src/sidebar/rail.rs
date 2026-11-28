// The server rail: Home, Favorites, DM tiles, the servers and their folders.
use super::*;

// One tile's state, as the rail draws it.
pub(super) struct RailTile {
    pub(super) key: String,
    pub(super) name: SharedString,
    pub(super) selected: bool,
    pub(super) unread: bool,
    pub(super) mentions: u32,
    pub(super) muted: bool,
    // The top-right media badge: what's live there, and whether you're connected to it.
    pub(super) media: Option<Media>,
    pub(super) face: Face,
    pub(super) guild_id: Option<String>,
    // Badges shrink with a folder's trip (1 shown, 0 gone).
    pub(super) badge_scale: f32,
    // An open folder's head trades its minis for the folder glyph (0 closed, 1 open).
    pub(super) open: f32,
    // A folder head whose pill its servers carry off while it's open (their pills start from it).
    pub(super) handed: bool,
}

impl Waffle {
    pub fn render_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dm_mentions: u32 = self.dms.iter().map(|d| self.mention_count(&d.id)).sum();
        let home = RailTile {
            key: "dms".into(),
            name: "Direct messages".into(),
            selected: self.rail_selected(None),
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
        let favorites = self.favorites_item(window, cx);
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
        let separator = div().w_6().h_0p5().flex_none().rounded_full().bg(palette::line_strong(cx));
        // Under the lights the rail starts below a strip the channel header's height (so their edges meet in one line),
        // its first tile just under that edge.
        let top = if lights { 4. } else { HEADER_INSET };
        ui::col()
            .id("rail")
            .track_scroll(&self.rail_scroll)
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .pt(px(top))
            .pb_3()
            .items_center()
            .gap(px(rail_gap()))
            .overflow_y_scroll()
            .text_size(ui::TEXT_SM)
            // Without the lights, Home holds the slot over a drag strip; Home keeps its clicks from the strip,
            // and the rail still scrolls under it.
            .when(!lights, |rail| rail.child(div().absolute().top_0().left_0().w_full().h(px(crate::layout::HEADER_HEIGHT)).window_control_area(gpui_kit::WindowControlArea::Drag)))
            .child(if lights { home } else { div().block_mouse_except_scroll().child(home).into_any_element() })
            .children(favorites)
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
            .when(mentions, |b| b.child(ui::Badge::dot().absolute().top_1p5().right_1p5()))
            .on_click(cx.listener(|this, _, _, cx| this.open_inbox(cx)));
        // Under the traffic lights a strip sits above the tiles, a hard edge they scroll up to (ui::hard_edge).
        let lights = crate::layout::traffic_lights_shown(window);
        let strip = lights.then(|| ui::hard_edge(self.sidebar_scrolled(window), cx).h(px(crate::layout::HEADER_HEIGHT)).window_control_area(gpui_kit::WindowControlArea::Drag));
        ui::col()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .items_center()
            .children(strip)
            .child(div().relative().flex_1().min_h_0().w_full().child(self.render_rail(window, cx)))
            .child(div().flex_none().pt(px(rail_gap())).pb_2().child(inbox))
            .child(div().flex_none().h(self.user_panel_clearance()))
    }

    // Whether the rail or the channel list has scrolled under its header: one hairline then runs across both. Without
    // the traffic lights the rail has no strip to scroll under, so only the channel list counts.
    pub(super) fn sidebar_scrolled(&self, window: &Window) -> impl Fn() -> bool + 'static {
        let rail = crate::layout::traffic_lights_shown(window).then(|| self.rail_scroll.clone());
        let channels = self.channels_scroll.clone();
        move || rail.as_ref().is_some_and(ui::scrolled) || ui::scrolled(&channels)
    }

    // DMs that want you, under Home as Discord stacks them: unread ones, newest first, and the one
    // you're in a call with.
    pub(super) fn rail_dms(&self) -> Vec<Channel> {
        let in_call = |id: &str| self.call.as_ref().is_some_and(|c| c.guild_id.is_none() && c.channel.id == id);
        self.dms.iter().filter(|d| self.unread(&d.id) || in_call(&d.id)).cloned().collect()
    }

    // DM tiles grow in and fold away on one gently bouncing spring: the slot's height, the tile's fade and scale travel
    // together, so the servers below glide instead of jumping. The tile scales with its slot, so it never
    // reaches past it into Home or the next tile. A leaving tile keeps its last count and stays until its
    // exit settles. Each slot carries the gap above it, so the column cancels the rail's own gap.
    pub(super) fn rail_dm_tiles(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
            let tile = self.dm_rail_item(dm.clone(), count, window, cx);
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
        // lint-allow(raw-margin): cancels the rail's own gap above the first slot, as each slot carries the gap above it; padding can't be negative.
        ui::col().flex_none().mt(px(-rail_gap())).children(slots)
    }

    pub(super) fn dm_rail_item(&mut self, dm: Channel, mentions: u32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tile = RailTile {
            key: format!("dm-{}", dm.id),
            name: dm.recipient.as_ref().map_or_else(|| dm.name.clone(), |u| u.name.clone()).into(),
            selected: self.rail_selected(None) && self.channel.as_ref().is_some_and(|c| c.id == dm.id),
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

    // Whether the rail marks this server (None: Home) as open; Favorites holds the mark while it shows.
    pub(super) fn rail_selected(&self, guild_id: Option<&String>) -> bool {
        !self.in_favorites() && self.guild_id.as_ref() == guild_id
    }

    // The star tile after Home, while something is favourited.
    pub(super) fn favorites_item(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.favorites.shown() {
            return None;
        }
        let (unread, mentions) = self.favorites_badge();
        let tile = RailTile {
            key: crate::favorites::RAIL_KEY.into(),
            name: "Favorites".into(),
            selected: self.in_favorites(),
            unread,
            mentions,
            muted: self.favorites.muted,
            media: None,
            face: Face::Favorites,
            guild_id: None,
            badge_scale: 1.,
            open: 0.,
            handed: false,
        };
        let item = self.rail_item(tile, window, cx);
        Some(div().id("rail-favorites-hit").child(item).on_click(cx.listener(|this, _, window, cx| this.open_favorites(window, cx))).into_any_element())
    }

    // Who's in the server's voice channels and who's live there, by avatar, for its hover card.
    pub(super) fn server_preview(&self, guild_id: &str, name: SharedString) -> ServerPreview {
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
    pub(super) fn guild_media(&self, guild: &Guild) -> Option<Media> {
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

    pub(super) fn guild_tile(&self, guild: &Guild, badge_scale: f32) -> RailTile {
        let (unread, mentions) = self.guild_badge(guild);
        RailTile {
            key: guild.id.clone(),
            name: guild.name.clone().into(),
            selected: self.rail_selected(Some(&guild.id)),
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

    pub(super) fn guild_item(&mut self, guild: usize, badge_scale: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let tile = self.guild_tile(&self.guilds[guild], badge_scale);
        self.rail_item(tile, window, cx)
    }

    // A folder: closed, a tile of its first four servers; open, a tinted column holding them all.
    pub(super) fn folder_item(&mut self, folder: Folder, indices: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let guilds: Vec<&Guild> = indices.iter().map(|&i| &self.guilds[i]).collect();
        let open = self.folder_shown_open(&folder);
        let key = format!("folder-{}", folder.id);
        let motion = FolderMotion::frame(&key, open, window, cx);
        let tint: Hsla = folder.color.map(palette::data_color).unwrap_or(palette::faint(cx));
        let (unread, mentions) = guilds.iter().map(|g| self.guild_badge(g)).fold((false, 0), |(u, m), (gu, gm)| (u || gu, m + gm));
        let has_selected = guilds.iter().any(|g| self.rail_selected(Some(&g.id)));
        let name: SharedString = folder.name.clone().unwrap_or_else(|| guilds.iter().map(|g| g.name.as_str()).take(3).collect::<Vec<_>>().join(", ")).into();
        let minis: Vec<Mini> = guilds.iter().map(|g| mini(g)).collect();
        let badges: Vec<Badge> = guilds.iter().map(|g| {
            let (unread, mentions) = self.guild_badge(g);
            Badge { pill: pill_height(self.rail_selected(Some(&g.id)), false, unread), mentions }
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
                this.redraw(crate::part::Column::Nav, cx);
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
            // lint-allow(custom-mark): ui::badged marks a face's corner; this is the folder's tinted backdrop behind its servers.
            .child(div().absolute().top_0().bottom_0().left(px(TILE_X)).w(px(TILE)).squircle(ui::RADIUS_LG).bg(tint.opacity(0.18 * motion.trip)))
            // Servers over the head: in flight they cross its tinted face, which would shade them until
            // they land as its unshaded minis, a flash at the end of a close.
            .child(head)
            .children(servers)
            .into_any_element()
    }

    // A rail tile: the face, the state pill on the rail's edge (dot unread, longer on hover, full when
    // selected), and a count for mentions.
    pub(super) fn rail_item(&mut self, tile: RailTile, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            RowDrag::Rest => {
                let behind = crate::material::behind_sidebar(rail);
                let media = media.filter(|_| badge_scale > 0.99).map(|m| media_mark(m, cx));
                let tile = ui::badged(render_face(face, TILE, ui::RADIUS_LG, selected, open, cx), TILE, ui::Shape::Tile, behind).count_scaled(mentions, badge_scale);
                match media {
                    Some((icon, fill)) => tile.icon(ui::Spot::TopRight, icon, fill).into_any_element(),
                    None => tile.into_any_element(),
                }
            }
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
            .child(div().absolute().left_0().top(px((TILE - pill_height) / 2.)).w_1().h(px(pill_height)).rounded_r_sm().bg(palette::strong(cx)))
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
                            None => ui::Tooltip::new(name.clone()).build(window, cx),
                        }
                    })
                    .tooltip_placement(TooltipPlacement::Side),
            )
            .when(combine_ring, |item| item.child(combine_ring_mark()))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(&hover_key, *hovered, cx)))
            .when_some(guild_id.clone(), |item, guild_id| item.on_click(cx.listener(move |this, _, window, cx| this.open_guild(Some(guild_id.clone()), window, cx))))
            .when(key == "dms", |item| item.on_click(cx.listener(|this, _, window, cx| this.open_guild(None, window, cx))));
        let item = match key.as_str() {
            k if k == "dms" || k == crate::favorites::RAIL_KEY || k.starts_with("dm-") => item,
            _ => self.rail_draggable(item, RailDrag { key: key.clone() }, look, self.rail_list_of(&key), cx),
        };
        let Some(guild_id) = guild_id else { return item.into_any_element() };
        item.on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, e: &gpui_kit::MouseDownEvent, _, cx| this.open_server_menu(guild_id.clone(), e.position, false, cx)))
            .into_any_element()
    }

    // Carried, a tile swells to 104% and hangs at -4° (.srv.dragging), tilting further with speed. A server
    // can go onto another top-level tile (making or joining a folder); inside a folder it only reorders.
    pub(super) fn rail_draggable(&self, item: ui::Pressable, drag: RailDrag, look: crate::drag::Look, list: SharedString, cx: &mut Context<Self>) -> ui::Pressable {
        let key = drag.key.clone();
        let ghost = crate::drag::pick_up(cx.weak_entity(), format!("rail-{key}").into(), look, point(px(RAIL_WIDTH), px(TILE)), 1.04, -4., true);
        let combines_here = list == RAIL_LIST;
        let item = item.on_drag(drag, Board::lift(cx.weak_entity(), rail_board, key.clone(), ghost));
        Board::target(item, rail_board, list, key, move |d: &RailDrag| combines_here && !d.key.starts_with("folder-"), cx)
    }

    // The rail list `key` sits in: its open folder's, or the rail's own.
    pub(super) fn rail_list_of(&self, key: &str) -> SharedString {
        match self.folders.iter().find(|f| !f.id.is_empty() && f.id != "0" && f.guild_ids.iter().any(|g| g == key)) {
            Some(folder) => folder_list(&folder.id),
            None => RAIL_LIST.into(),
        }
    }

    // Sticky for the drag: once the dragged server is aimed outside the folder it came from, that folder
    // folds too.
    pub(super) fn note_left_folder(&mut self) {
        let Some(dragged) = self.rail_board.dragged() else { return self.rail_left_folder = false };
        let Some(home) = self.folders.iter().find(|f| f.guild_ids.iter().any(|g| g == dragged)) else { return };
        let inside = self.rail_board.hint().is_none_or(|h| h.list == folder_list(&home.id) || h.target.as_deref() == Some(format!("folder-{}", home.id).as_str()));
        if !inside {
            self.rail_left_folder = true;
        }
    }

    // Open folders fold shut while a rail drag is on, so only top-level slots are in the way; the folder
    // the dragged server came from stays open until the server leaves it. They reopen on letting go.
    pub(super) fn folder_shown_open(&self, folder: &Folder) -> bool {
        let open = self.open_folders.contains(&folder.id);
        match self.rail_board.dragged() {
            None => open,
            Some(dragged) => open && folder.guild_ids.iter().any(|g| g == dragged) && !self.rail_left_folder,
        }
    }
}
