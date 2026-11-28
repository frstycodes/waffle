use super::*;

// The composer row's vertical padding, and the card's hairline edge.
const ROW_PAD_Y: f32 = 8.;
const GLASS_EDGE: f32 = 0.5;
// A tray tile's tag gap, picture height and card width.
const TAG_GAP: f32 = 4.;
const WELL_HEIGHT: f32 = 140.;
const TRAY_CARD_WIDTH: f32 = 184.;
// The popups above the composer: the lift off its top edge, Hold Up!'s side padding, the suggestion detail's
// widest, and the list's tallest.
const POPUP_LIFT: f32 = 8.;
const HOLD_UP_PAD_X: f32 = 18.;
const SUGGEST_DETAIL_MAX: f32 = 180.;
const SUGGEST_MAX_H: f32 = 340.;

impl Waffle {
    pub(crate) fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let open_picker = self.composer_picker();
        // The draft stays in `composer` underneath; the input row comes back when the guard lifts.
        let locked = self.locked_row(cx);
        let row = ui::row()
            .min_h_12()
            .py(px(ROW_PAD_Y))
            .px_1p5()
            .items_end()
            .gap_1()
            .child(ui::Button::new("attach").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::PlusCircle).tooltip_text("Upload a file or create a poll").on_click(cx.listener(|this, _, window, cx| this.open_menu(window.mouse_position(), this.attach_entries(), cx))))
            // XSmall gives the editor 0/4px padding of its own; with these, the design's 4/6.
            .child(self.composer_editor(strong, cx))
            .children(self.slowmode_timer(cx))
            .children([(ComposerPicker::Gifs, "composer-gif", Glyph::Gif, "Open GIF picker"), (ComposerPicker::Stickers, "composer-sticker", Glyph::Sticker, "Open sticker picker"), (ComposerPicker::Emoji, "composer-emoji", Glyph::Emoji, "Select Emoji")].map(|(picker, id, glyph, label)| {
                ui::Button::new(id).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(glyph).tooltip_text(label).toggled(open_picker == Some(picker)).on_click(cx.listener(move |this, _, window, cx| this.show_composer_picker(picker, window, cx)))
            }))
            // Not verified: the tooltip; Discord's chat box shows its button only when this setting is on.
            .when(self.prefs.send_button, |r| r.child(ui::Button::new("composer-send").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::PaperPlane).tooltip_text("Send Message").disabled(!self.has_draft).on_click(cx.listener(|this, _, window, cx| this.submit(false, window, cx)))))
            .child(self.composer_activities_button(cx));
        // lint-allow(custom-surface): ui::PopoverContent::new is a floating menu surface; the composer is a raised card in the flow of the chat
        let card = ui::col()
            .relative()
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx))
            // The ring-less elevation: the hairline below is the edge, and the pop shadow's own 1px ring
            // on top of it drew a line three times as heavy as the design's.
            .shadow(ui::shadow_pop_on(gpui_kit::transparent_black(), cx))
            // A hairline: one device pixel on Retina; a full point read heavy around the glass.
            .border(px(GLASS_EDGE))
            .border_color(if focused { palette::line_strong(cx) } else { palette::line(cx) })
            .when(locked.is_none(), |card| {
                card.children(self.autocomplete_popup(cx))
                    .children(self.command_popup(cx))
                    .children(self.hold_up_popout(cx))
                    .children(self.request_bar(cx))
                    .children(self.queued_bar(cx))
                    .children(self.composer_bar(cx))
                    .children(self.attachment_tray(cx))
            })
            .child(locked.unwrap_or_else(|| row.into_any_element()));
        // Enter on a command that can't run yet.
        let shake = self.command_picker.shake;
        let card = match shake {
            0 => card.into_any_element(),
            n => card.with_animation(("composer-shake", n), Animation::new(crate::motion::SHAKE), |card, t| card.left(px(crate::motion::shake(t)))).into_any_element(),
        };
        // The card's side inset, as padding of the box that holds it (outside the shake, which moves the card itself).
        let card = ui::col().px_4().child(card);
        let guide_bar = self.channel.clone().and_then(|channel| self.guide_action_bar(&channel, cx));
        let typers = self.typers();
        let names: Vec<&str> = typers.iter().map(|t| t.name.as_str()).collect();
        // A line just above the composer, out of the flow, so the box never moves when someone types. It has no fill of
        // its own: it sits on the feed's fade (feed_edge.rs), which keeps it readable over the messages.
        // Out of the way while suggestions, Hold Up! or the command picker are open, which rise over the same spot.
        let typing = (!names.is_empty() && self.autocomplete.is_none() && self.hold_up.is_none() && !self.command_picker.is_open() && !self.command_typed(cx)).then(|| {
            ui::row().absolute().left_7().top_neg_7().max_w(gpui_kit::relative(0.6)).h_6().px_2p5().gap_2()
                .text_size(ui::TEXT_XS)
                .text_color(muted)
                .child(ui::Spinner::new("typing", palette::accent_ink(cx), 12.))
                .child(ui::rich_line(emphasised(&typing_parts(&names), strong)))
        });
        let (height, this) = (self.composer_height.clone(), cx.weak_entity());
        let measure = gpui_kit::canvas(
            move |bounds, _, cx| {
                let h = f32::from(bounds.size.height);
                if (h - height.get()).abs() > 0.5 {
                    height.set(h);
                    // The feed's bottom padding and the jump button follow the composer's height: the main column's.
                    this.update(cx, |this, cx| this.redraw(crate::part::Column::Main, cx)).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let note = self.send_error.clone().map(|error| div().px_4().pt_1p5().text_size(ui::TEXT_XS).text_color(palette::danger_ink(cx)).child(error));
        ui::col()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .capture_key_down(cx.listener(Self::on_composer_key))
            // Before the text field's own Tab, which indents and would never reach on_composer_key.
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::IndentInline, window, cx| {
                if this.on_command_tab(false, window, cx) {
                    return cx.stop_propagation();
                }
                if this.autocomplete.is_some() {
                    this.accept_suggestion(window, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::OutdentInline, window, cx| {
                if this.on_command_tab(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            // A slash command's boxes change what Backspace and Delete take; otherwise the field deletes.
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::Backspace, window, cx| {
                if this.on_command_delete(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::Delete, window, cx| {
                if this.on_command_delete(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            // Before the text field's own Paste, which drops anything that isn't text.
            .capture_action(cx.listener(|this, _: &gpui_kit::component::input::Paste, window, cx| {
                if this.paste_files(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(measure)
            .children(self.jump_to_present(cx))
            // Occludes: the composer sits over the foot of the chat column, which must not take the hover or the click under it.
            // A Server Guide to-do bar rides on top, inside the measured stack so the feed leaves room for it.
            .child(ui::col().relative().occlude().gap_2().children(guide_bar).child(card).children(typing))
            .children(note)
            // 8px off the window's foot, level with the user card in the left column.
            .child(div().h_2())
    }

    pub(super) fn composer_editor(&self, ink: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let waffle = cx.weak_entity();
        // An empty OS menu shows nothing; Waffle draws its own in its place.
        let menu = move |_, window: &mut Window, cx: &mut App| {
            let at = window.mouse_position();
            waffle.update(cx, |this, cx| this.open_text_box_menu(at, cx)).ok();
            gpui_kit::component::native_menu::NativeMenu::new()
        };
        let field = Textarea::new(&self.composer).context_menu(menu).appearance(false).with_size(Size::XSmall).text_size(ui::TEXT_MD).line_height(ui::LINE_LEAD).text_color(ink).token(composer_tokens(cx));
        // XSmall gives the editor 0/4px padding of its own; with these, the design's 4/6.
        // The option boxes paint after the field: they read where its text landed this frame.
        div().relative().flex_1().min_w_0().child(field.py_1().px_0p5()).children(self.option_boxes(cx)).into_any_element()
    }

    // A mention badge's click: a person's profile at the click, a role's card at `beside` (just off the
    // badge's right edge), or the channel it names.
    pub(super) fn open_mention(&mut self, url: &str, at: gpui_kit::Point<Pixels>, beside: gpui_kit::Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = url.strip_prefix("waffle://user/") {
            let Some(user) = self.known_users().into_iter().find(|u| u.id == id) else { return };
            return self.open_profile(user, at, cx);
        }
        if let Some(id) = url.strip_prefix("waffle://role/") {
            return self.open_role_popout(Some(id.to_owned()), beside, cx);
        }
        if url == "waffle://everyone" {
            return self.open_role_popout(None, beside, cx);
        }
        let Some(id) = url.strip_prefix("waffle://channel/") else { return };
        let channel = self.guild().and_then(|g| g.channels.iter().find(|c| c.id == id).cloned());
        if let Some(channel) = channel {
            self.open_channel(channel, window, cx);
        }
    }

    // Right-clicking a mention: a user's member menu, as on their name; a channel's or role's own menu.
    pub(super) fn open_mention_menu(&mut self, url: &str, at: gpui_kit::Point<Pixels>, cx: &mut Context<Self>) {
        if let Some(id) = url.strip_prefix("waffle://channel/").map(str::to_owned) {
            return self.open_live_menu(at, move |this| this.channel_mention_menu(&id), cx);
        }
        if let Some(id) = url.strip_prefix("waffle://role/").map(str::to_owned) {
            return self.open_live_menu(at, move |this| this.role_mention_menu(&id), cx);
        }
        let Some(id) = url.strip_prefix("waffle://user/") else { return };
        // A mention of someone not loaded yet still gets the menu; its items need only the id.
        let user = self.known_users().into_iter().find(|u| u.id == id).unwrap_or_else(|| crate::discord::User {
            id: id.to_owned(),
            name: self.user_name(id),
            avatar: crate::discord::avatar_url(id, None),
            ..Default::default()
        });
        self.open_author_menu(user, at, cx);
    }

    pub(super) fn shows_jump_to_present(&self) -> bool {
        self.scrolled_up && self.list.item_count() > 0
    }

    // A small capsule 12px above the composer, out of the flow, so it never opens a gap.
    pub(super) fn jump_to_present(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if !self.shows_jump_to_present() {
            return None;
        }
        let overlay = palette::surface(Level::Overlay, cx);
        let hover = ui::mix(overlay, palette::strong(cx), 0.06);
        // Occludes what's under it, or a click would land on the message behind as well.
        let capsule = ui::pressable(div().id("jump-to-present"), 0.97)
            .occlude()
            .h_8()
            .px_3p5()
            .flex()
            .items_center()
            .gap_2()
            .rounded_full()
            .bg(overlay)
            .shadow(ui::shadow_pop(cx))
            .cursor_pointer()
            .whitespace_nowrap()
            .text_size(ui::TEXT_SM)
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::strong(cx))
            .hover(move |s| s.bg(hover))
            .child(Icon::new(Glyph::ArrowDown).size(ui::ICON_MD))
            .child("Jump To Present")
            .on_click(cx.listener(|this, _, _, cx| {
                this.list.scroll_to_end();
                this.scrolled_up = false;
                this.redraw(crate::part::Column::Main, cx);
            }));
        Some(ui::row().items_stretch().absolute().left_0().right_0().top_neg_11().justify_center().child(capsule).with_animation(
            "jump-to-present-in",
            Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()),
            // lint-allow(raw-margin, raw-px): the entrance slides 8px as it fades, a margin computed from the animation's progress
            |d, t| d.opacity(t).mt(px((1. - t) * 8.)),
        ))
    }

    // The reply or edit bar at the top of the composer.
    pub(super) fn composer_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let strong = palette::strong(cx);
        let label = match (&self.replying, &self.editing) {
            (Some(reply), _) => emphasised(&[("Replying to ".into(), false), (self.member_name(&reply.author), true)], strong),
            (_, Some(_)) => emphasised(&[("Editing message".into(), true)], strong),
            _ => return None,
        };
        let bar = bar_row(cx).child(bar_label(label));
        let bar = match self.replying.is_some() {
            true => bar.child(self.ping_mention(cx)),
            false => bar.child(ui::key_hint(ui::Kbd::new("Esc"), "Cancel", cx)).child(ui::key_hint(ui::Kbd::new("Enter"), "Save", cx)),
        };
        Some(bar.child(ui::Button::new("cancel-compose").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::X).tooltip_text("Cancel").on_click(cx.listener(|this, _, window, cx| this.cancel_compose(window, cx)))))
    }

    // Whether the reply notifies its author; it remembers your last choice.
    pub(super) fn ping_mention(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let on = self.reply_ping;
        let hover = palette::hover(cx);
        // lint-allow(custom-button): ui::Button's Ghost variant is 30px at its smallest, this one sits in a 28px bar and takes the accent ink when on
        ui::row()
            .id("reply-ping")
            .h_7()
            .px_2()
            .gap_1()
            .squircle(RADIUS_XS)
            .cursor_pointer()
            .text_size(ui::TEXT_XS)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(if on { palette::accent_ink(cx) } else { palette::faint(cx) })
            .hover(move |s| s.bg(hover))
            .child(Icon::new(Glyph::AtBold).size(ui::ICON_MD))
            .child(if on { "On" } else { "Off" })
            .tooltip(ui::Tooltip::text("Mention the author of this message"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.reply_ping = !this.reply_ping;
                this.redraw(crate::part::Column::Main, cx);
            }))
    }

    // Files waiting to send, as cards in a borderless well inside the composer (design: direction A): the
    // picture on top (the OS's thumbnail, else the type's icon), its name and size under it. Discord's
    // three actions float at the picture's corner while the card is hovered.
    pub(super) fn attachment_tray(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        if self.pending_files.is_empty() {
            return None;
        }
        let cards: Vec<AnyElement> = self.pending_files.iter().enumerate().map(|(i, file)| self.tray_card(i, file, cx)).collect();
        Some(ui::row().id("composer-tray").items_end().gap_2().p_2p5().overflow_x_scroll().border_b_1().border_color(palette::line(cx)).children(cards))
    }

    pub(super) fn tray_card(&self, i: usize, file: &crate::attachments::PendingFile, cx: &mut Context<Self>) -> AnyElement {
        let key = format!("tray-{i}");
        let hovered = self.hovered_tile.as_deref() == Some(key.as_str());
        let track = track_tile_hover(cx.weak_entity(), key);
        let picture = self.file_previews.get(&file.path).cloned().flatten();
        let veiled = file.spoiler && file.kind() != crate::attachments::FileKind::Other;
        let has_alt = file.description.as_deref().is_some_and(|d| !d.is_empty());
        let tags = ui::row().items_stretch()
            .absolute()
            .left_1p5()
            .bottom_1p5()
            .gap(px(TAG_GAP))
            .when(has_alt, |t| t.child(media_label("Alt")))
            .when(file.spoiler, |t| t.child(media_label("Spoiler")));
        let well = ui::row()
            .relative()
            .h(px(WELL_HEIGHT))
            .flex_none()
            .overflow_hidden()
            .squircle(RADIUS_MD)
            .bg(palette::surface(Level::Raised, cx))
            .text_color(palette::muted(cx))
            .justify_center()
            .map(|w| match picture {
                // Thumbnails fill the well; icons keep their own transparent margins.
                Some(image) => w.child(ui::media(image, ui::all(RADIUS_MD)).size_full().object_fit(gpui_kit::ObjectFit::Cover)),
                None => w.child(Icon::new(file.kind().glyph()).size_10()),
            })
            .when(veiled, |w| w.child(spoiler_veil()))
            .child(tags);
        let meta = ui::col()
            .px_1p5()
            .pb_0p5()
            .gap_px()
            .child(ui::line(file.name.clone()).text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
            .child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(format_size(file.size)));
        let (spoiler_glyph, spoiler_tip) = match file.spoiler {
            true => (Glyph::EyeSlash, "Remove Spoiler"),
            false => (Glyph::Eye, "Spoiler Attachment"),
        };
        let actions = hovered.then(|| {
            floating_bar(SharedString::from(format!("tray-actions-{i}")), cx)
                .top_3()
                .right_3()
                .on_hover(track_tile_hover(cx.weak_entity(), format!("tray-{i}")))
                .child(ui::Button::new(SharedString::from(format!("spoiler-file-{i}"))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(spoiler_glyph).tooltip_text(spoiler_tip).on_click(cx.listener(move |this, _, _, cx| this.toggle_spoiler(i, cx))))
                .child(ui::Button::new(SharedString::from(format!("modify-file-{i}"))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::Pen).tooltip_text("Modify Attachment").on_click(cx.listener(move |this, _, window, cx| this.open_modify_pending(i, window, cx))))
                .child(ui::Button::new(SharedString::from(format!("remove-file-{i}"))).variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::X).tooltip_text("Remove Attachment").on_click(cx.listener(move |this, _, _, cx| this.remove_file(i, cx))))
        });
        ui::col()
            .id(SharedString::from(format!("tray-card-{i}")))
            .relative()
            .w(px(TRAY_CARD_WIDTH))
            .flex_none()
            .p_1p5()
            .gap_1p5()
            .squircle(RADIUS_LG)
            .bg(palette::selected(cx))
            .on_hover(track)
            .child(well)
            .child(meta)
            .children(actions)
            .into_any_element()
    }

    // While files are dragged over the chat: the whole pane says where they'll go, and takes the drop
    // anywhere, the composer included. Invisible otherwise; its hitbox blocks nothing beneath.
    pub(super) fn drop_zone(&self, channel: &Channel, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let limit = format_size(crate::discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier)));
        let accent = palette::accent_ink(cx);
        let card = ui::col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx).opacity(0.92))
            .border_2()
            .border_color(accent)
            .child(div().text_color(accent).child(Icon::new(Glyph::Upload).size_10()))
            .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("Drop to upload to {}", crate::channel_mention(channel))))
            .child(div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(format!("Up to {} files, {limit} each", crate::discord::MAX_FILES)));
        div()
            .id("drop-zone")
            .absolute()
            .inset_0()
            .p_3()
            .opacity(if cfg!(debug_assertions) && std::env::var("WAFFLE_DEMO").as_deref() == Ok("attachments-drop") { 1. } else { 0. })
            .drag_over::<gpui_kit::ExternalPaths>(|style, _, _, _| style.opacity(1.))
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, window, cx| this.add_files(paths.paths().to_vec(), window, cx)))
            .child(card)
    }

    // Arrows drive the suggestion list (Tab and Enter accept elsewhere); Esc cancels a reply or edit; Up in an empty box edits your last message.
    pub(super) fn on_composer_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        // Discord's slash lists also move on Ctrl-P / Ctrl-N.
        let command_key = match (event.keystroke.modifiers.control, key) {
            (true, "p") => "ctrl-p",
            (true, "n") => "ctrl-n",
            _ => key,
        };
        if self.on_command_key(command_key, cx) {
            return cx.stop_propagation();
        }
        if self.autocomplete.is_some() {
            match key {
                "up" => self.move_suggestion(-1, cx),
                "down" => self.move_suggestion(1, cx),
                "escape" => self.autocomplete = None,
                _ => return,
            }
            return cx.stop_propagation();
        }
        match key {
            "escape" if self.hold_up.is_some() => self.hold_up = None,
            "escape" if self.replying.is_some() || self.editing.is_some() => self.cancel_compose(window, cx),
            "up" if !self.has_draft && self.edit_last(window, cx) => {}
            _ => return,
        }
        cx.stop_propagation();
    }

    // Discord's "Hold Up!" before a mass ping, anchored above the composer: Esc edits, Enter sends.
    pub(super) fn hold_up_popout(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let hold_up = self.hold_up.as_ref()?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let body = emphasised(&[("By using ".into(), false), (hold_up.mention.into(), true), (hold_up.after_mention(), false)], strong);
        let edit = ui::Button::new("hold-up-edit").variant(ui::ButtonVariant::Secondary).gap_2().child("Edit Message").child(ui::Kbd::new("Esc")).on_click(cx.listener(|this, _, window, cx| {
            this.hold_up = None;
            this.composer.update(cx, |state, cx| state.focus(window, cx));
            this.redraw(crate::part::Column::Main, cx);
        }));
        let send = ui::Button::new("hold-up-send").gap_2().child("Send Now").child(ui::Kbd::new("Enter")).on_click(cx.listener(|this, _, window, cx| this.submit(true, window, cx)));
        Some(
            // lint-allow(custom-badge, custom-mark, custom-surface): ui::PopoverContent::new isn't positioned off its parent's top edge; ui::Popover::floating_at anchors to a window point, not to the composer card
            ui::col()
                .id("hold-up")
                .occlude()
                .absolute()
                .left_neg_px()
                .right_neg_px()
                .bottom(gpui_kit::relative(1.))
                // lint-allow(raw-margin): `bottom` is a fraction of the card's height, which can't also take a pixel lift
                .mb(px(POPUP_LIFT))
                .py_4()
                .px(px(HOLD_UP_PAD_X))
                .gap_2p5()
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Overlay, cx))
                .shadow(ui::shadow_pop(cx))
                .text_size(ui::TEXT_SM)
                .text_color(palette::text(cx))
                .child(
                    ui::row().gap_2().text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD).text_color(strong)
                        .child(div().text_color(palette::accent_ink(cx)).child(Icon::new(Glyph::Megaphone).size(ui::ICON_XL)))
                        .child("Hold Up!"),
                )
                .child(div().line_height(ui::LINE_SM).child(body))
                .child(ui::row().gap_1p5().text_size(ui::TEXT_XS).text_color(muted).child(Icon::new(Glyph::Info).size(ui::ICON_SM)).child("Maybe try mentioning a role or individual users instead?"))
                .child(ui::row().items_stretch().flex_wrap().gap_2().child(edit).child(send))
                .transform_origin(point(gpui_kit::relative(0.5), gpui_kit::relative(1.)))
                .with_animation("hold-up", Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t)),
        )
    }

    // Suggestions above the composer: its width, 8px above it, rising from it.
    pub(super) fn autocomplete_popup(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let auto = self.autocomplete.as_ref()?;
        let (hover, selected, strong, muted, text) = (palette::hover(cx), palette::selected(cx), palette::strong(cx), palette::muted(cx), palette::text(cx));
        let heading: SharedString = match auto.token.chars().next() {
            Some('#') => "Text Channels".into(),
            Some(':') => format!("Emoji matching {}", auto.token).into(),
            _ => "Members".into(),
        };
        let rows = auto.items.iter().enumerate().map(|(i, item)| {
            let lead = suggest_lead(&item.lead, cx);
            let detail = item.detail.clone().map(|d| ui::line(d).flex_none().max_w(px(SUGGEST_DETAIL_MAX)).text_size(ui::TEXT_XS).text_color(muted));
            // lint-allow(custom-button): ui::DropdownMenuItem::new fills on hover but has no persistent selected state driven by the arrow keys
            ui::row()
                .id(SharedString::from(format!("suggest-{i}")))
                .min_h_10()
                .py_1p5()
                .px_2p5()
                .gap_2p5()
                .squircle(RADIUS_SM)
                .cursor_pointer()
                .text_color(text)
                .when(i == auto.selected, |r| r.bg(selected))
                .when(i != auto.selected, |r| r.hover(move |s| s.bg(hover)))
                .child(lead)
                .child(ui::line(item.label.clone()).flex_1().font_weight(FontWeight::MEDIUM).text_color(strong))
                .children(detail)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(auto) = &mut this.autocomplete {
                        auto.selected = i;
                    }
                    this.accept_suggestion(window, cx);
                }))
        });
        // msg-pop: it grows from 95% at its bottom centre as it fades in.
        Some(
            // lint-allow(custom-badge, custom-mark, custom-surface): ui::PopoverContent::new isn't positioned off its parent's top edge; ui::Popover::floating_at anchors to a window point, not to the composer card
            ui::col()
                .absolute()
                .left_neg_px()
                .right_neg_px()
                .bottom(gpui_kit::relative(1.))
                // lint-allow(raw-margin): `bottom` is a fraction of the card's height, which can't also take a pixel lift
                .mb(px(POPUP_LIFT))
                .p_1p5()
                .gap_0p5()
                .max_h(px(SUGGEST_MAX_H))
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Overlay, cx))
                .shadow(ui::shadow_pop(cx))
                .text_size(ui::TEXT_SM)
                .child(div().flex_none().pt_1p5().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(muted).child(heading))
                // The rows scroll inside the popup's height rather than spilling out over the composer.
                .child(ui::col().min_h_0().id("autocomplete-list").flex_1().gap_0p5().overflow_y_scroll().track_scroll(&self.autocomplete_scroll).children(rows))
                .transform_origin(point(gpui_kit::relative(0.5), gpui_kit::relative(1.)))
                .with_animation("autocomplete", Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t)),
        )
    }
}

// Discord's text box menu (its desktop app's), drawn as Waffle's menu instead of the OS one: Cut and Copy
// with a selection, then Paste. Spellcheck, AI rewrites and Paste as Plain Text need what Waffle lacks.
fn text_box_entries(selected: bool) -> Vec<crate::menu::Entry> {
    use crate::menu::item;
    use gpui_kit::component::input::{Copy, Cut, Paste};
    let mut out = Vec::new();
    if selected {
        out.push(item("Cut", |this, window, cx| this.composer_action(&Cut, window, cx)).shortcut("secondary+x").entry());
        out.push(item("Copy", |this, window, cx| this.composer_action(&Copy, window, cx)).shortcut("secondary+c").entry());
    }
    out.push(item("Paste", |this, window, cx| this.composer_action(&Paste, window, cx)).shortcut("secondary+v").entry());
    out
}

impl Waffle {
    pub(crate) fn open_text_box_menu(&mut self, at: gpui_kit::Point<Pixels>, cx: &mut Context<Self>) {
        let selected = self.composer.read(cx).selected_text().len() > 0;
        self.open_menu(at, text_box_entries(selected), cx);
    }

    fn composer_action(&mut self, action: &dyn gpui_kit::Action, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.composer.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        focus.dispatch_action(action, window, cx);
    }
}

// Discord's channel- and role-mention menus: Mention (into the draft), then Copy link or Open in Server
// Settings, then the ID.
impl Waffle {
    fn channel_mention_menu(&self, id: &str) -> Vec<crate::menu::Entry> {
        use crate::menu::{Entry, item};
        let Some((guild, channel)) = self.guilds.iter().find_map(|g| g.channels.iter().find(|c| c.id == id).map(|c| (g, c))) else { return Vec::new() };
        let mut out = Vec::new();
        if self.can_mention_here() {
            let (raw, shown) = (format!("<#{id}>"), format!("#{}", channel.name));
            out.push(item("Mention", move |this, window, cx| this.insert_mention(raw.clone(), shown.clone(), window, cx)).entry());
            out.push(Entry::Separator);
        }
        let link = format!("https://discord.com/channels/{}/{id}", guild.id);
        out.push(item("Copy link", move |this, _, cx| this.copy_text(link.clone(), cx)).icon(Glyph::Link).entry());
        if self.developer_mode {
            let id = id.to_owned();
            out.push(item("Copy channel ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    fn role_mention_menu(&self, id: &str) -> Vec<crate::menu::Entry> {
        use crate::menu::{Entry, item};
        let Some((guild, role)) = self.guilds.iter().find_map(|g| g.roles.iter().find(|r| r.id == id).map(|r| (g, r))) else { return Vec::new() };
        let mut out = Vec::new();
        if self.can_mention_here() {
            let (raw, shown) = (format!("<@&{id}>"), format!("@{}", role.name));
            out.push(item("Mention", move |this, window, cx| this.insert_mention(raw.clone(), shown.clone(), window, cx)).entry());
            out.push(Entry::Separator);
        }
        if guild.can(self.my_id(), crate::discord::perm::MANAGE_ROLES) {
            let (gid, rid) = (guild.id.clone(), id.to_owned());
            out.push(item("Open in Server Settings", move |this, window, cx| {
                this.open_server_settings(gid.clone(), window, cx);
                this.open_server_page(crate::server_settings::access::Page::Roles, cx);
                if let Some(view) = this.server_settings.as_mut() {
                    view.roles.pending_open = Some(rid.clone());
                }
            }).entry());
            out.push(Entry::Separator);
        }
        if self.developer_mode {
            let id = id.to_owned();
            out.push(item("Copy role ID", move |this, _, cx| this.copy_text(id.clone(), cx)).icon(Glyph::Id).entry());
        }
        out
    }

    // Discord offers Mention while the open channel takes messages from us.
    fn can_mention_here(&self) -> bool {
        self.channel.as_ref().is_some_and(|c| c.kind == crate::discord::ChannelKind::Dm || self.can_in(&c.id, crate::discord::perm::SEND_MESSAGES))
    }
}
