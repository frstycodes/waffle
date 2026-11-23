use super::*;

impl Waffle {
    pub(crate) fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let open_picker = self.composer_picker();
        // The draft stays in `composer` underneath; the input row comes back when the guard lifts.
        let locked = self.locked_row(cx);
        let row = ui::row()
            .min_h_12()
            .py(px(7.))
            .px_1p5()
            .items_end()
            .gap_1()
            .child(ui::icon_button("attach", Glyph::PlusCircle, "Upload a file or create a poll", cx).on_click(cx.listener(|this, _, window, cx| this.open_menu(window.mouse_position(), this.attach_entries(), cx))))
            // XSmall gives the editor 0/4px padding of its own; with these, the design's 4/6.
            .child(self.composer_field(strong, cx))
            .children(self.slowmode_chip(cx))
            .children([(ComposerPicker::Gifs, "composer-gif", Glyph::Gif, "Open GIF picker"), (ComposerPicker::Stickers, "composer-sticker", Glyph::Sticker, "Open sticker picker"), (ComposerPicker::Emoji, "composer-emoji", Glyph::Emoji, "Select Emoji")].map(|(picker, id, glyph, label)| {
                ui::icon_button(id, glyph, label, cx).toggled(open_picker == Some(picker), cx).on_click(cx.listener(move |this, _, window, cx| this.show_composer_picker(picker, window, cx)))
            }))
            // Not verified: the tooltip; Discord's chat box shows its button only when this setting is on.
            .when(self.prefs.send_button, |r| r.child(ui::icon_button("composer-send", Glyph::PaperPlane, "Send Message", cx).disabled(!self.has_draft).on_click(cx.listener(|this, _, window, cx| this.submit(false, window, cx)))))
            .child(self.composer_activities_button(cx));
        let card = ui::col()
            .relative()
            .mx_4()
            .squircle(RADIUS_LG)
            // Glass: the feed shows through, blurred, as it scrolls under.
            .bg(palette::surface(Level::Raised, cx).opacity(0.72))
            .backdrop_blur(px(24.))
            .backdrop_saturate(1.6)
            // The ring-less elevation: the hairline below is the edge, and the pop shadow's own 1px ring
            // on top of it drew a line three times as heavy as the design's.
            .shadow(ui::shadow_pop_on(gpui_kit::transparent_black(), cx))
            // A hairline: one device pixel on Retina; a full point read heavy around the glass.
            .border(px(0.5))
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
        let guide_bar = self.channel.clone().and_then(|channel| self.guide_action_bar(&channel, cx));
        let typers = self.typers();
        let names: Vec<&str> = typers.iter().map(|t| t.name.as_str()).collect();
        // A glass pill just above the composer, out of the flow, so the box never moves when someone types.
        // Out of the way while suggestions, Hold Up! or the command picker are open, which rise over the same spot.
        let typing = (!names.is_empty() && self.autocomplete.is_none() && self.hold_up.is_none() && !self.command_picker.is_open() && !self.command_typed(cx)).then(|| {
            ui::row().absolute().left_7().top_neg_7().max_w(gpui_kit::relative(0.6)).h_6().px_2p5().gap_2().rounded_full()
                .bg(palette::surface(Level::Raised, cx).opacity(0.72))
                .backdrop_blur(px(16.))
                .text_size(ui::TEXT_XS)
                .text_color(muted)
                .child(ui::bars("typing", palette::accent_ink(cx), 12.))
                .child(div().min_w_0().truncate().child(emphasised(&typing_parts(&names), strong)))
        });
        let (height, this) = (self.composer_height.clone(), cx.weak_entity());
        let measure = gpui_kit::canvas(
            move |bounds, _, cx| {
                let h = f32::from(bounds.size.height);
                if (h - height.get()).abs() > 0.5 {
                    height.set(h);
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let note = self.send_error.clone().map(|error| div().mx_4().mt_1p5().text_size(ui::TEXT_XS).text_color(palette::danger_ink(cx)).child(error));
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
            // Occludes: the composer floats over the feed, which must not take the hover or the click under it.
            // A Server Guide to-do bar rides on top, inside the measured stack so the feed leaves room for it.
            .child(ui::col().relative().occlude().gap_2().children(guide_bar).child(card).children(typing))
            .children(note)
            // 8px off the window's foot, level with the user card in the left column.
            .child(div().h_2())
    }

    pub(super) fn composer_field(&self, ink: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let field = Textarea::new(&self.composer).appearance(false).with_size(Size::XSmall).text_size(ui::TEXT_LEAD).line_height(ui::LINE_LEAD).text_color(ink).token(composer_chip(cx));
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
            .child("Jump to present")
            .on_click(cx.listener(|this, _, _, cx| {
                this.list.scroll_to_end();
                this.scrolled_up = false;
                cx.notify();
            }));
        Some(ui::row().items_stretch().absolute().left_0().right_0().top_neg_11().justify_center().child(capsule).with_animation(
            "jump-to-present-in",
            Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()),
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
        let bar = bar_row(cx).child(div().min_w_0().truncate().child(label).flex_1());
        let bar = match self.replying.is_some() {
            true => bar.child(self.ping_toggle(cx)),
            false => bar.child(ui::key_hint(ui::kbd("Esc", cx), "Cancel", cx)).child(ui::key_hint(ui::kbd("Enter", cx), "Save", cx)),
        };
        Some(bar.child(ui::icon_button("cancel-compose", Glyph::X, "Cancel", cx).small().on_click(cx.listener(|this, _, window, cx| this.cancel_compose(window, cx)))))
    }

    // Whether the reply notifies its author; it remembers your last choice.
    pub(super) fn ping_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let on = self.reply_ping;
        let hover = palette::hover(cx);
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
            .tooltip(|window, cx| ui::tip("Mention the author of this message", window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.reply_ping = !this.reply_ping;
                cx.notify();
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
            .gap(px(3.))
            .when(has_alt, |t| t.child(media_label("Alt")))
            .when(file.spoiler, |t| t.child(media_label("Spoiler")));
        let well = ui::row()
            .relative()
            .h(px(140.))
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
                .child(ui::icon_button(SharedString::from(format!("spoiler-file-{i}")), spoiler_glyph, spoiler_tip, cx).small().on_click(cx.listener(move |this, _, _, cx| this.toggle_spoiler(i, cx))))
                .child(ui::icon_button(SharedString::from(format!("modify-file-{i}")), Glyph::Pen, "Modify Attachment", cx).small().on_click(cx.listener(move |this, _, window, cx| this.open_modify_pending(i, window, cx))))
                .child(ui::icon_button(SharedString::from(format!("remove-file-{i}")), Glyph::X, "Remove Attachment", cx).small().on_click(cx.listener(move |this, _, _, cx| this.remove_file(i, cx))))
        });
        ui::col()
            .id(SharedString::from(format!("tray-card-{i}")))
            .relative()
            .w(px(184.))
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
        let edit = ui::button("hold-up-edit", ui::Tone::Secondary, cx).gap_2().child("Edit Message").child(ui::kbd("Esc", cx)).on_click(cx.listener(|this, _, window, cx| {
            this.hold_up = None;
            this.composer.update(cx, |state, cx| state.focus(window, cx));
            cx.notify();
        }));
        let send = ui::button("hold-up-send", ui::Tone::Primary, cx).gap_2().child("Send Now").child(ui::kbd("Enter", cx)).on_click(cx.listener(|this, _, window, cx| this.submit(true, window, cx)));
        Some(
            ui::col()
                .id("hold-up")
                .occlude()
                .absolute()
                .left_neg_px()
                .right_neg_px()
                .bottom(gpui_kit::relative(1.))
                .mb(px(9.))
                .py_4()
                .px(px(18.))
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
            let detail = item.detail.clone().map(|d| ui::line(d).flex_none().max_w(px(180.)).text_size(ui::TEXT_XS).text_color(muted));
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
            ui::col()
                .absolute()
                .left_neg_px()
                .right_neg_px()
                .bottom(gpui_kit::relative(1.))
                .mb(px(9.))
                .p_1p5()
                .gap_0p5()
                .max_h(px(340.))
                .squircle(RADIUS_LG)
                .bg(palette::surface(Level::Overlay, cx))
                .shadow(ui::shadow_pop(cx))
                .text_size(ui::TEXT_SM)
                .child(div().flex_none().pt_1p5().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(muted).child(heading))
                // The rows scroll inside the popup's height rather than spilling out over the composer.
                .child(ui::col().id("autocomplete-list").flex_1().gap_0p5().overflow_y_scroll().track_scroll(&self.autocomplete_scroll).children(rows))
                .transform_origin(point(gpui_kit::relative(0.5), gpui_kit::relative(1.)))
                .with_animation("autocomplete", Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t)),
        )
    }
}
