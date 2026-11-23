use crate::ui;
use super::*;

// The width of the Watch-alongside button, and the side padding of a mark with no words.
const ALONGSIDE_WIDTH: f32 = 30.;
const MARK_PAD: f32 = 3.;

impl Waffle {
    // A watched stream that ended, in its card's place: that it ended and why, and Close stream to clear
    // it. While it's up, the streamer going live again brings us back in as a viewer, as Discord's does.
    pub(super) fn ended_tile(&self, streamer: &str, size: TileSize, cx: &mut Context<Self>) -> AnyElement {
        let name = self.user_name(streamer);
        let line = match (self.participants().iter().any(|s| s.user_id == streamer), self.activity_name(streamer)) {
            (false, _) => format!("{name} left the call"),
            (true, Some(app)) => format!("{name} stopped sharing {app}"),
            (true, None) => format!("{name} stopped streaming"),
        };
        let close = streamer.to_owned();
        self.stage_notice(format!("ended-{streamer}"), size, ("Stream ended", line, "Close stream"), move |this, cx| this.close_ended_stream(&close, cx), cx)
    }

    // Discord's failed-stream screen: what went wrong, its error code, and Close, which stops watching.
    pub(super) fn failed_tile(&self, streamer: &str, code: u16, size: TileSize, cx: &mut Context<Self>) -> AnyElement {
        let close = streamer.to_owned();
        let line = format!("{} Error code: {code}", failure_line(code));
        self.stage_notice(format!("failed-{streamer}"), size, ("The stream couldn't start", line, "Close stream"), move |this, cx| {
            this.unwatch_stream(&close, cx);
            if this.focused_stream().as_deref() == Some(close.as_str()) {
                this.call_focus = None;
            }
        }, cx)
    }

    // `text`: the heading, the line under it, and the button's label.
    pub(super) fn stage_notice(&self, key: String, size: TileSize, text: (&'static str, String, &'static str), close: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        let (heading, line, button) = text;
        let (fill, w) = match size {
            TileSize::Card(w) => (false, w),
            TileSize::Fill => (true, f32::INFINITY),
        };
        let radius = if fill { 0. } else { RADIUS_LG };
        let small = w <= STRIP_TILE + 0.5;
        let card = ui::col()
            .id(SharedString::from(key.clone()))
            .relative()
            .map(|d| match size {
                TileSize::Card(w) => d.w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(radius).overflow_hidden(),
                TileSize::Fill => d.size_full(),
            })
            .bg(stage::ended())
            .items_center()
            .justify_center()
            .gap_1()
            .p_3()
            .text_center();
        if small {
            return card.child(Icon::new(Glyph::ScreenOff).size_6().text_color(stage::muted())).child(tile_ring(false, false, false, radius)).into_any_element();
        }
        card.child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::MEDIUM).text_color(stage::strong()).child(heading))
            .child(div().text_size(ui::TEXT_XS).text_color(stage::muted()).child(line))
            .child(div().mt_1p5().child(stage_button(SharedString::from(format!("{key}-close")), button).on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                close(this, cx);
            }))))
            .when(!fill, |d| d.child(tile_ring(false, false, false, radius)))
            .with_animation(SharedString::from(format!("{key}-in")), Animation::new(ui::DUR_PANEL), |d, t| d.opacity(t.min(1.)))
            .into_any_element()
    }

    pub(super) fn close_ended_stream(&mut self, streamer: &str, cx: &mut Context<Self>) {
        if let Some(call) = &mut self.call {
            call.ended.retain(|id| id != streamer);
        }
        if self.focused_stream().as_deref() == Some(streamer) {
            self.call_focus = None;
        }
        cx.notify(); // lint-allow(global-notify): dropping the focus swaps the stage layout (main) and the activity frame
    }

    // Over a stream not yet watched: Watch stream switches to it alone; while others play, the button
    // beside it adds it to them, as Discord's.
    pub(super) fn watch_actions(&self, tile: &Tile, alongside: bool, cx: &mut Context<Self>) -> Div {
        let (only, also) = (tile.clone(), tile.clone());
        let switch = watch_button("Watch stream").id("watch-only").cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.stop_watching(cx);
            this.call_focus = None;
            this.toggle_card(&only, cx);
        }));
        ui::row().gap_1p5().child(switch).when(alongside, |d| {
            d.child(
                watch_button_look()
                    .id("watch-alongside")
                    .w(px(ALONGSIDE_WIDTH))
                    .justify_center()
                    .cursor_pointer()
                    .child(Icon::new(Glyph::Plus).size(ui::ICON_MD))
                    .tooltip(|window, cx| ui::tip("Watch alongside", window, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if let Tile::Stream(state) = &also {
                            this.watch_stream(state.user_id.clone(), cx);
                        }
                    })),
            )
        })
    }

    // .tile: a person (camera, or their avatar on their own tint) or someone's Go Live. Every card
    // carries a name chip (a stream's led by the screen glyph); only our own stream says LIVE, with its
    // quality. A stream not yet opened shows Watch at rest. Speaking draws the green ring above
    // everything. `TileSize::Fill` is the focused card: the whole stage, no corners or edge line.
    pub(super) fn tile(&self, tile: &Tile, size: TileSize, cx: &mut Context<Self>) -> AnyElement {
        let call = self.call.as_ref().expect("in a call");
        let (fill, w) = match size {
            TileSize::Card(w) => (false, w),
            TileSize::Fill => (true, f32::INFINITY),
        };
        let radius = if fill { 0. } else { RADIUS_LG };
        let state = match tile {
            Tile::User(state) | Tile::Stream(state) => state,
            Tile::Ended(streamer) => return self.ended_tile(streamer, size, cx),
            Tile::Activity(app_id) => return self.activity_tile(app_id, (!fill).then_some(w), cx),
        };
        let user = state.user.clone().unwrap_or_else(|| User {
            id: state.user_id.clone(),
            name: self.user_name(&state.user_id),
            avatar: discord::avatar_url(&state.user_id, None),
            ..Default::default()
        });
        let me = self.me.as_ref().is_some_and(|m| m.id == state.user_id);
        let (muted, deafened) = if me { (self.muted || self.deafened, self.deafened) } else { (state.muted, state.deafened) };
        let local_muted = !me && self.prefs.local_mutes.contains(&state.user_id);
        let is_stream = matches!(tile, Tile::Stream(_));
        let watch_state = call.watched(&state.user_id).map(|w| w.state);
        if is_stream
            && let Some(crate::WatchState::Failed(code)) = watch_state
        {
            return self.failed_tile(&state.user_id, code, size, cx);
        }
        let waiting = call.issues.contains(&(format!("stream:{}", state.user_id), voice::issues::RECEIVER_READY_TIMEOUT));
        if is_stream && waiting {
            let streamer = state.user_id.clone();
            let line = format!("Connected, but no picture has come through. Error code: {}", voice::issues::RECEIVER_READY_TIMEOUT);
            return self.stage_notice(format!("no-video-{}", state.user_id), size, ("No video yet", line, "Try again"), move |this, cx| this.retry_watch(&streamer, cx), cx);
        }
        let me_hidden = self.me.as_ref().is_some_and(|m| m.id == state.user_id) && self.self_stream_hidden();
        if is_stream && me_hidden {
            return self.stage_notice(format!("hidden-{}", state.user_id), size, ("Stream preview hidden", "You're still live; only your own preview is off.".into(), "Show preview"), |this, cx| this.toggle_self_stream_hidden(cx), cx);
        }
        let talking = !is_stream && self.speaking(&state.user_id);
        let sounding = !is_stream && self.soundboard_playing(&state.user_id);
        let camera = (!is_stream).then(|| call.cameras.get(&state.user_id)).flatten();
        let media = is_stream || camera.is_some();
        // A stream card plays the stream once it's watched (our own from the local capture), and shows
        // Discord's preview still until then.
        // A watched stream that stopped delivering goes back to the loader instead of freezing on its
        // last frame; a paused one keeps it.
        let stalled = !me && call.watched(&state.user_id).is_some_and(|w| w.arrivals.back().is_some_and(|at| at.elapsed() > STALL_AFTER)) && !self.stream_paused(&state.user_id);
        let live_frame = match me {
            true => call.self_view.as_ref().map(|(frame, _)| frame.clone()),
            false => call.watched(&state.user_id).and_then(|w| w.frame.clone()).filter(|_| !stalled),
        };
        let loading = is_stream && live_frame.is_none() && (me || call.watched(&state.user_id).is_some());
        // Focused, the picture keeps its whole frame (bars on the axis that doesn't fit); a card is
        // cropped to fill.
        let fit = if fill { gpui_kit::ObjectFit::Contain } else { gpui_kit::ObjectFit::Cover };
        // A strip card under a focused one: no name at rest, as Discord's.
        let small = w <= STRIP_TILE + 0.5;
        let face: AnyElement = match (is_stream, camera) {
            // A stream that isn't playing, small: Discord's screen glyph on black.
            (true, _) if small && live_frame.is_none() => {
                ui::row().size_full().squircle(radius).bg(gpui_kit::black()).justify_center().child(Icon::new(Glyph::Screen).size_7().text_color(gpui_kit::white())).into_any_element()
            }
            // Playing, the picture letterboxes on black; the gradient only holds the place before it.
            (true, _) => match fill || live_frame.is_some() || loading {
                true => div().relative().size_full().when(!fill, |d| d.squircle(radius)).bg(gpui_kit::black()),
                false => stream_fill(radius).size_full(),
            }
            .map(|f| match live_frame {
                Some(frame) => f.child(ui::media(ImageSource::Render(frame), ui::all(radius)).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Contain)),
                // Discord's preview is a stale, soft still: once we're waiting on the real picture, say so.
                None if loading => f
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(mascot::loader(SharedString::from(format!("stream-loading-{}", state.user_id)), if fill { 64. } else { 40. }, palette::logo(), cx))
                    // Discord's reconnecting overlay, with its error code.
                    .when(watch_state == Some(crate::WatchState::Reconnecting) && !small, |f| {
                        f.child(div().text_size(ui::TEXT_XS).text_color(stage::muted()).child(format!("Reconnecting… Error code: {}", crate::STREAM_RECONNECTING)))
                    }),
                None => f.children(self.stream_preview(&state.user_id).map(|url| ui::media(url, ui::all(radius)).absolute().inset_0().size_full().object_fit(fit))),
            })
            .into_any_element(),
            // Our own camera reads as a mirror, as every call app shows it; others see it unflipped.
            (false, Some(frame)) => ui::media(ImageSource::Render(frame.clone()), ui::all(radius)).size_full().object_fit(fit).when(me, mirrored).into_any_element(),
            (false, None) => ui::row()
                .size_full()
                .when(!fill, |d| d.squircle(radius))
                .bg(self.tile_fill(&state.user_id))
                .justify_center()
                .child(ui::avatar(user.avatar.clone(), if fill { 96. } else if w < SMALL_TILE { 40. } else { 64. }).when(local_muted, |a| a.opacity(0.5)))
                .into_any_element(),
        };
        let watching = call.watched(&state.user_id).is_some();
        let watchable = is_stream && !watching && !me;
        let label = match (is_stream, self.activity_name(&state.user_id)) {
            (true, Some(app)) => format!("{} · {app}", user.name),
            _ => user.name.clone(),
        };
        let group = SharedString::from(format!("tile-{}-{is_stream}", state.user_id));
        let name = tile_name(
            div()
                .when(is_stream, |d| d.child(Icon::new(Glyph::Screen).size(ui::ICON_SM).text_color(gpui_kit::white())))
                .when(!is_stream && self.stage_moderator(&state.user_id), |d| d.child(crate::stage_view::moderator_mark(&state.user_id)))
                .when(!is_stream && state.server_muted, |d| d.child(vmark(Glyph::MicOff, None, MarkKind::Server)))
                .when(!is_stream && !state.server_muted && muted && !deafened, |d| d.child(Icon::new(Glyph::MicOff).size(ui::ICON_SM).text_color(stage::off_ink())))
                .when(!is_stream && deafened, |d| d.child(Icon::new(Glyph::HeadphonesOff).size(ui::ICON_SM).text_color(stage::off_ink())))
                .child(ui::line(label))
                .when(local_muted, |d| d.child(vmark(Glyph::VolumeOff, Some("Muted for you"), MarkKind::Local))),
        )
        .max_w(if fill { relative(0.5) } else { px(w - 16.).into() })
        .when(small || fill, |n| n.opacity(0.).group_hover(STAGE_GROUP, |s| s.opacity(1.)));
        // With the name hidden on a strip card, a mute or deafen still shows, as a chip in its corner.
        let off_chip = (small && !is_stream && (muted || deafened || state.server_muted)).then(|| {
            ui::row()
                .absolute()
                .left_1p5()
                .bottom_1p5()
                .size_6()
                .squircle(RADIUS_XS)
                .justify_center()
                .bg(gpui_kit::black().opacity(0.6))
                .child(Icon::new(if deafened { Glyph::HeadphonesOff } else { Glyph::MicOff }).size(ui::ICON_SM).text_color(stage::off_ink()))
        });
        let issues = self.tile_issues(&state.user_id, is_stream, me);
        let chip_id = SharedString::from(format!("issues-{}-{is_stream}", state.user_id));
        // Our stream's sits beside LIVE: the viewers chip holds the corner.
        let own_issues = (is_stream && me && !issues.is_empty()).then(|| issue_chip(chip_id.clone(), issues.clone()));
        let issue_chip = (!(is_stream && me) && !issues.is_empty() && !small).then(|| issue_chip(chip_id, issues).absolute().top_2().right_2());
        // Our own stream: its quality beside LIVE, as Discord's "720P 30FPS LIVE".
        let own_live = (is_stream && me && !small).then(|| {
            let quality = format!("{} FPS", quality_label(self.prefs.stream_resolution, self.prefs.stream_fps));
            ui::row().absolute().left_2().top_2().gap_1().child(ui::badge(gpui_kit::black().opacity(0.6), gpui_kit::white()).font_features(tabular()).child(quality)).child(ui::live_badge()).children(own_issues)
        });
        let streamer = state.user_id.clone();
        let menu_user = state.user_id.clone();
        let card = tile.clone();
        let viewers = if is_stream { self.viewers(&state.user_id) } else { Vec::new() };
        let alongside = call.watching.iter().any(|w| w.streamer_id != state.user_id);
        div()
            .id(group.clone())
            .group(group.clone())
            .relative()
            .map(|d| match size {
                TileSize::Card(w) => d.w(px(w)).h(px(w * 9. / 16.)).flex_none().squircle(radius).overflow_hidden(),
                TileSize::Fill => d.size_full(),
            })
            .child(face)
            // Scrims only while hovered, so pictures aren't dimmed at rest; the chips carry their own fill.
            .when(media && !fill, |d| d.child(scrim_top(radius).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))).child(scrim_bottom(radius).opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))))
            .children(own_live)
            .children(issue_chip)
            .when(!viewers.is_empty(), |d| d.child(viewer_chip(viewers, &state.user_id)))
            .when(watchable && !small, |d| {
                d.child(ui::row().absolute().inset_0().when(!fill, |d| d.squircle(radius)).bg(gpui_kit::black().opacity(0.45)).justify_center().child(self.watch_actions(tile, alongside, cx)))
            })
            .child(name)
            .children(off_chip)
            .children((!is_stream).then(|| self.soundboard_tile_effect(&state.user_id, effect_area(w), cx)).flatten())
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_card(&card, cx)))
            .when(is_stream && me && !small, |d| {
                d.child(
                    div()
                        .absolute()
                        .right_2()
                        .bottom_1p5()
                        .opacity(0.)
                        .group_hover(group.clone(), |s| s.opacity(1.))
                        .child(ui::round_button("card-hide-preview", Glyph::EyeSlash, "Hide preview", 28.).on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.toggle_self_stream_hidden(cx);
                        }))),
                )
            })
            .when(is_stream && watching && !me && !small && !fill, |d| {
                let streamer = streamer.clone();
                let volume_id = streamer.clone();
                d.child(
                    ui::row()
                        .absolute()
                        .right_2()
                        .bottom_1p5()
                        .gap_1p5()
                        .opacity(0.)
                        .group_hover(group.clone(), |s| s.opacity(1.))
                        .child(ui::round_button(SharedString::from(format!("card-volume-{streamer}")), Glyph::VolumeHigh, "Stream volume", 28.).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.open_stream_volume(volume_id.clone(), e.position, cx);
                            }),
                        ))
                        .child(ui::button(SharedString::from(format!("card-unwatch-{streamer}")), ui::Tone::OnMedia, cx).small().child("Stop watching").on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.unwatch_stream(&streamer, cx);
                        }))),
                )
            })
            // Every card answers the pointer with a brighter edge; the focused one has no edge at all.
            .when(!fill || talking || sounding, |d| d.child(tile_ring(talking, sounding, local_muted, radius).when(!talking && !sounding, |r| r.group_hover(group.clone(), |s| s.border_color(stage::wash(0.2))))))
            .on_mouse_down(MouseButton::Right, cx.listener(move |this, e: &MouseDownEvent, _, cx| this.voice_user_menu(&menu_user, e.position, cx)))
            .into_any_element()
    }

    // A person's tile tint (oklch 0.24 0.02, their hue): their avatar's hue once fetched, else one
    // from their id so the grid isn't one grey.
    pub(super) fn tile_fill(&self, user_id: &str) -> Hsla {
        let hue = match self.avatar_colors.get(user_id).copied().flatten() {
            Some(color) => oklch_hue(color),
            None => user_id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as f32 % 360.,
        };
        palette::oklch(0.24, 0.02, hue)
    }
}

// .tile .name: a dark chip in the tile's foot, 12px white, so it reads over any picture.
pub(super) fn tile_name(content: Div) -> Div {
    content
        .absolute()
        .left_2()
        .bottom_2()
        .h_6()
        .px_2()
        .squircle(RADIUS_XS)
        .bg(gpui_kit::black().opacity(0.6))
        .flex()
        .items_center()
        .gap_1p5()
        .min_w_0()
        .text_size(ui::TEXT_XS)
        .font_weight(FontWeight::MEDIUM)
        .text_color(gpui_kit::white())
}

// The tile's edge, drawn above its media so a stream can't hide it: a faint 1px line, or while
// speaking a 2px green ring with a dark inner line (neutral when they're muted for you).
// The tile a soundboard effect plays across: a card is 16:9 at its width.
// GAP: the focused tile fills the stage, whose size isn't known here; 960 wide stands in.
pub(super) fn effect_area(width: f32) -> (f32, f32) {
    let width = if width.is_finite() { width } else { 960. };
    (width, width * 9. / 16.)
}

// A soundboard sound gets its own ring, as Discord's, so it never reads as someone talking: honey,
// the colour the picker marks a playing sound with. It outranks the speaking green while it plays.
pub(super) fn tile_ring(speaking: bool, sounding: bool, local_muted: bool, radius: f32) -> Div {
    let ring = div().absolute().inset_0().squircle(radius);
    if !speaking && !sounding {
        return ring.border_1().border_color(stage::wash(0.06));
    }
    // The stage is always dark, so the dark theme's green.
    let color = match (sounding, local_muted) {
        (true, _) => palette::accent(),
        (false, true) => stage::wash(0.42),
        (false, false) => palette::status(crate::discord::Status::Online),
    };
    ring.border_2().border_color(color).child(div().absolute().inset_0().squircle((radius - 2.).max(0.)).border_2().border_color(stage::bg().opacity(0.6)))
}

// .chrome over a focused stream: black fading out from the top (180deg) or bottom (0deg) edge.
pub(super) fn scrim(angle: f32) -> gpui_kit::Background {
    gpui_kit::linear_gradient(angle, gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.6), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.))
}

// .tile.has-media::before: a smooth scrim under the labels, never a hard strip.
pub(super) fn scrim_top(radius: f32) -> Div {
    ui::edge(div().absolute().top_0().left_0().right_0().h(relative(0.3)), ui::top(radius)).bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.35), 0.), gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 1.)))
}

pub(super) fn scrim_bottom(radius: f32) -> Div {
    ui::edge(div().absolute().bottom_0().left_0().right_0().h(relative(0.38)), ui::bottom(radius)).bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(gpui_kit::transparent_black(), 0.), gpui_kit::linear_color_stop(gpui_kit::black().opacity(0.55), 1.)))
}

// .vmark: why you can't hear someone, as a chip; on tiles it can carry the words.
pub(super) fn vmark(glyph: Glyph, words: Option<&'static str>, kind: MarkKind) -> Div {
    let base = ui::row()
        .h(ui::ICON_LG)
        .min_w(ui::ICON_LG)
        .flex_none()
        .squircle(6.)
        .justify_center()
        .gap_1()
        .text_size(ui::TEXT_XXS)
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .text_color(gpui_kit::white())
        .when(words.is_some(), |d| d.pl_1().pr_1p5())
        .when(words.is_none(), |d| d.px(px(MARK_PAD)))
        .child(Icon::new(glyph).size(ui::ICON_XS))
        .children(words);
    match kind {
        MarkKind::Server => base.bg(palette::danger()),
        MarkKind::Local => base.bg(stage::bg().opacity(0.72)).border_1().border_color(stage::wash(0.16)),
    }
}
