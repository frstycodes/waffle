// New Post: the dialog that starts a post (title, opening message, files, tags, Post Guidelines), its slowmode
// clock and its submit.
use super::*;

impl Waffle {
    // New Post: a title, the opening message (starting as the forum's template), files and up to five
    // tags. Post Guidelines open by themselves the first time per forum.
    pub(super) fn open_new_post(&mut self, forum_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let (template, has_guidelines) = self.forum_channel(&forum_id).map_or((String::new(), false), |f| (f.props.template.trim().to_owned(), guidelines_text(f).is_some()));
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Title").validate(|text, _| text.chars().count() <= MAX_TITLE));
        let body = cx.new(|cx| TextareaState::new(window, cx).placeholder("Message").default_value(template));
        // lint-allow(detached-task, global-notify): the subscription ends when the input, held in ForumState, drops; the new post dialog is drawn by the app's own render
        cx.subscribe(&title, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        // lint-allow(detached-task, global-notify): the subscription ends when the input, held in ForumState, drops; the new post dialog is drawn by the app's own render
        cx.subscribe(&body, |_, _, _: &InputEvent, cx| cx.notify()).detach();
        title.update(cx, |s, cx| s.focus(window, cx));
        self.menu = None;
        for state in self.forums.values_mut() {
            state.composer = None;
        }
        let state = self.forums.entry(forum_id).or_default();
        let guidelines = has_guidelines && !state.guidelines_seen;
        state.guidelines_seen = true;
        state.composer = Some(Composer { title, body, tags: Vec::new(), files: Vec::new(), guidelines, guidelines_full: false, previewing: false, error: None, sending: false });
        cx.notify(); // lint-allow(global-notify): the new post dialog is drawn by the app's own render, outside the columns
    }

    pub(super) fn open_guidelines(&mut self, forum_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_new_post(forum_id, window, cx);
        self.change_composer(|c| c.guidelines = true, cx);
    }

    pub(super) fn open_composer(&self) -> Option<(&str, &Composer)> {
        self.forums.iter().find_map(|(id, s)| Some((id.as_str(), s.composer.as_ref()?)))
    }

    pub(super) fn composer_mut(&mut self) -> Option<&mut Composer> {
        self.forums.values_mut().find_map(|s| s.composer.as_mut())
    }

    pub(super) fn change_composer(&mut self, change: impl FnOnce(&mut Composer), cx: &mut Context<Self>) {
        if let Some(composer) = self.composer_mut() {
            change(composer);
        }
        cx.notify(); // lint-allow(global-notify): the new post dialog is drawn by the app's own render, outside the columns
    }

    pub(super) fn close_new_post(&mut self, cx: &mut Context<Self>) {
        for state in self.forums.values_mut() {
            state.composer = None;
        }
        cx.notify(); // lint-allow(global-notify): the new post dialog is drawn by the app's own render, outside the columns
    }

    pub(super) fn toggle_post_draft_tag(&mut self, tag_id: &str, cx: &mut Context<Self>) {
        let Some(composer) = self.composer_mut() else { return };
        match composer.tags.iter().position(|t| t == tag_id) {
            Some(at) => drop(composer.tags.remove(at)),
            None if composer.tags.len() < MAX_TAGS => composer.tags.push(tag_id.to_owned()),
            None => {}
        }
        composer.error = None;
        cx.notify(); // lint-allow(global-notify): the new post dialog is drawn by the app's own render, outside the columns
    }

    pub(super) fn pick_post_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            this.update(cx, |this, cx| this.add_post_files(paths, cx)).ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once the file prompt resolves, and holds only a weak handle
    }

    // The chat composer's caps: your upload limit per file and ten files.
    pub(super) fn add_post_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let limit = crate::discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier));
        let files = paths.into_iter().filter_map(PendingFile::new).collect();
        self.change_composer(|c| c.error = crate::attachments::admit(&mut c.files, files, limit).map(Into::into), cx);
    }

    pub(super) fn cooldown_left(&self, forum_id: &str) -> Option<Duration> {
        let until = self.forums.get(forum_id)?.cooldown_until?;
        until.checked_duration_since(Instant::now()).filter(|left| !left.is_zero())
    }

    // Not verified: that Manage Messages or Manage Channel skip a forum's post slowmode, as they do a channel's.
    pub(super) fn skips_slowmode(&self, forum_id: &str) -> bool {
        self.can_in(forum_id, perm::MANAGE_MESSAGES) || self.can_in(forum_id, perm::MANAGE_CHANNELS)
    }

    // After a post, the forum's slowmode runs down, ticking the composer's clock each second.
    pub(super) fn start_post_cooldown(&mut self, forum_id: &str, cx: &mut Context<Self>) {
        let slowmode = self.forum_channel(forum_id).map_or(0, |f| f.props.slowmode);
        if slowmode == 0 || self.skips_slowmode(forum_id) {
            return;
        }
        self.forums.entry(forum_id.to_owned()).or_default().cooldown_until = Some(Instant::now() + Duration::from_secs(slowmode.into()));
        let id = forum_id.to_owned();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let ticking = this.update(cx, |this, cx| {
                    cx.notify(); // lint-allow(hot-notify, global-notify): ticks once a second only while a post slowmode runs down, redrawing the app-level dialog's clock, and stops when it ends
                    this.cooldown_left(&id).is_some()
                });
                if !ticking.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach(); // lint-allow(detached-task): the loop ends itself when the slowmode runs out, and holds only a weak handle
    }

    pub(super) fn submit_new_post(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((forum_id, composer)) = self.open_composer().map(|(id, c)| (id.to_owned(), c)) else { return };
        if composer.sending || self.cooldown_left(&forum_id).is_some() {
            return;
        }
        let (name, content, tags) = (composer.title.read(cx).value().trim().to_owned(), composer.body.read(cx).value().trim().to_owned(), composer.tags.clone());
        let files: Vec<crate::discord::Upload> = composer.files.iter().map(PendingFile::upload).collect();
        let (require_tag, archive_after) = self.forum_channel(&forum_id).map_or((false, 0), |f| (f.props.require_tag, f.props.archive_after));
        if let Some(error) = post_error(&name, !content.is_empty() || !files.is_empty(), require_tag && tags.is_empty()) {
            return self.change_composer(|c| c.error = Some(error.into()), cx);
        }
        let Some(client) = self.client.clone() else { return };
        self.change_composer(|c| c.sending = true, cx);
        let guild_id = self.guild_id.clone().unwrap_or_default();
        let body = post_body(&name, &content, &tags, &files, if archive_after > 0 { archive_after } else { ARCHIVE_AFTER });
        cx.spawn_in(window, async move |this, cx| {
            let path = format!("/channels/{forum_id}/threads?use_nested_fields=true");
            let created = match files.is_empty() {
                true => client.call(Method::POST, path, Some(body)).await,
                false => client.call_files(path, body, files).await,
            };
            this.update_in(cx, |this, window, cx| {
                let post = match created {
                    Ok(thread) => parse_thread(&thread, &guild_id),
                    Err(error) => return this.change_composer(|c| (c.sending, c.error) = (false, Some(error.into())), cx),
                };
                this.close_new_post(cx);
                this.start_post_cooldown(&forum_id, cx);
                let Some(post) = post else { return this.show_toast("Couldn't open the new post", cx) };
                this.on_event(crate::discord::Event::ChannelUpsert { guild_id: guild_id.clone(), channel: post.clone() }, cx);
                this.open_channel(post, window, cx);
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or timer resolves, and holds only a weak handle
    }

    pub(crate) fn render_new_post(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (forum_id, composer) = self.open_composer()?;
        let forum = self.forum_channel(forum_id)?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let guidelines = composer.guidelines.then(|| self.guidelines_panel(forum, composer.guidelines_full, window, cx)).flatten();
        let fields = match composer.previewing {
            true => vec![self.post_preview(forum, composer, cx)],
            false => self.post_fields(forum, composer, cx),
        };
        let error = composer.error.clone().map(|e| ui::field_error(e, cx));
        let cooling = self.cooldown_left(forum_id);
        let slowmode = (forum.props.slowmode > 0 && !self.skips_slowmode(forum_id)).then(|| slowmode_clock(cooling.map_or(forum.props.slowmode.into(), |left| left.as_secs() + 1), cx));
        let preview = ui::button("new-post-preview", ui::Tone::Ghost, cx)
            .child(Icon::new(if composer.previewing { Glyph::EyeSlash } else { Glyph::Eye }).size(ui::ICON_MD))
            .child(if composer.previewing { "Exit Preview" } else { "Preview" })
            .on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.previewing = !c.previewing, cx)));
        let actions = ui::row()
            .gap_2()
            .child(preview)
            .children(slowmode)
            .child(div().flex_1())
            .child(ui::button("new-post-cancel", ui::Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_new_post(cx))))
            .child(
                ui::button("new-post-submit", ui::Tone::Primary, cx)
                    .loading(composer.sending.then_some("Posting"))
                    .disabled(cooling.is_some())
                    .child("Post")
                    .on_click(cx.listener(|this, _, window, cx| this.submit_new_post(window, cx))),
            );
        let reopen = (!composer.guidelines && guidelines_text(forum).is_some())
            .then(|| ui::icon_button("new-post-guidelines", Glyph::BookOpen, "Post Guidelines", cx).on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.guidelines = true, cx))));
        let head = ui::row()
            .items_start()
            .gap_1()
            .child(
                ui::col().min_h_0()
                    .flex_1()
                    .gap_0p5()
                    .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("New post"))
                    .child(ui::line(format!("in {}", forum.name)).text_size(ui::TEXT_SM).text_color(muted)),
            )
            .children(reopen)
            .child(ui::icon_button("new-post-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_new_post(cx))));
        let body = ui::dialog_body()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_new_post(cx);
                }
            }))
            .child(head)
            .child(ui::dialog_scroll("new-post-scroll", guidelines.map(IntoElement::into_any_element).into_iter().chain(fields).chain(error.map(IntoElement::into_any_element)), window, cx))
            .child(actions);
        let backdrop = div().id("new-post-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_new_post(cx)));
        Some(ui::modal(backdrop, ui::dialog("new-post", 520., body, window, cx), cx))
    }

    // Title, tags, message and files, as you write them.
    pub(super) fn post_fields(&self, forum: &Channel, composer: &Composer, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let manage = self.can_in(&forum.id, perm::MANAGE_THREADS);
        let full = composer.tags.len() >= MAX_TAGS;
        let title_focus = composer.title.read(cx).focus_handle(cx);
        let title_count = composer.title.read(cx).value().chars().count();
        let title = ui::field("Title", cx)
            .child(ui::text_field("new-post-title", Input::new(&composer.title).appearance(false), &title_focus))
            .child(ui::field_hint("", Some(format!("{title_count} / {MAX_TITLE}").into()), cx));
        let body_focus = composer.body.read(cx).focus_handle(cx);
        let message = ui::field("Message", cx).child(ui::text_area("new-post-body", Textarea::new(&composer.body).appearance(false).px_0().text_size(ui::TEXT_SM), &body_focus));
        let usable: Vec<&ForumTag> = forum.props.tags.iter().filter(|t| manage || !t.moderated).collect();
        let tags = (!usable.is_empty()).then(|| {
            let chips: Vec<AnyElement> = usable
                .iter()
                .map(|tag| {
                    let (on, tag_id) = (composer.tags.contains(&tag.id), tag.id.clone());
                    let chip = tag_chip(SharedString::from(format!("new-post-tag-{}", tag.id)), tag, on, cx).disabled(full && !on);
                    chip.on_click(cx.listener(move |this, _, _, cx| this.toggle_post_draft_tag(&tag_id, cx))).into_any_element()
                })
                .collect();
            ui::field(format!("Tags \u{b7} pick up to {MAX_TAGS}"), cx).child(ui::row().items_stretch().flex_wrap().gap_1p5().children(chips))
        });
        let files: Vec<AnyElement> = composer
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let path = file.path.clone();
                let remove = ui::icon_button(SharedString::from(format!("new-post-file-remove-{i}")), Glyph::X, "Remove", cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.change_composer(|c| c.files.retain(|f| f.path != path), cx)));
                file_chip(file, cx).child(remove).into_any_element()
            })
            .collect();
        let add = ui::button("new-post-attach", ui::Tone::Secondary, cx).small().child(Icon::new(Glyph::Upload).size(ui::ICON_SM)).child("Add files").on_click(cx.listener(|this, _, window, cx| this.pick_post_files(window, cx)));
        let attachments = ui::field("Files", cx).child(ui::row().flex_wrap().gap_1p5().children(files).child(add));
        [Some(title.into_any_element()), tags.map(IntoElement::into_any_element), Some(message.into_any_element()), Some(attachments.into_any_element())].into_iter().flatten().collect()
    }

    // The post as its card will read: tags, title, the message rendered, and its files.
    pub(super) fn post_preview(&self, forum: &Channel, composer: &Composer, cx: &mut Context<Self>) -> AnyElement {
        let (title, content) = (composer.title.read(cx).value().trim().to_owned(), composer.body.read(cx).value().to_string());
        let tags: Vec<Div> = forum.props.tags.iter().filter(|t| composer.tags.contains(&t.id)).map(|t| post_tag_badge(t.emoji.as_ref(), t.name.clone(), false, cx)).collect();
        let (heading, ink) = match title.is_empty() {
            true => ("Untitled post".to_owned(), palette::faint(cx)),
            false => (title, palette::strong(cx)),
        };
        ui::col()
            .p_4()
            .gap_2()
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(Level::Sidebar, cx))
            .border_1()
            .border_color(palette::line(cx))
            .child(ui::row().items_stretch().flex_wrap().gap_1p5().children(tags))
            .child(div().text_size(px(PREVIEW_HEADING)).font_weight(FontWeight::SEMIBOLD).text_color(ink).child(heading))
            .child(TextView::markdown("new-post-preview-body", crate::chat::discord_breaks(&content)).selectable(true))
            .child(ui::row().items_stretch().flex_wrap().gap_1p5().children(composer.files.iter().map(|f| file_chip(f, cx).pr_2p5())))
            .into_any_element()
    }

    // Post Guidelines: the forum's topic as markdown, 220px tall until expanded; its pencil opens the
    // forum's settings for Manage Channels.
    pub(super) fn guidelines_panel(&self, forum: &Channel, full: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let topic = guidelines_text(forum)?;
        let edit = self.can_in(&forum.id, perm::MANAGE_CHANNELS).then(|| {
            let (channel, guild_id) = (forum.clone(), self.guild_id.clone().unwrap_or_default());
            ui::icon_button("new-post-guidelines-edit", Glyph::Pen, "Edit Post Guidelines", cx).on_click(cx.listener(move |this, _, window, cx| {
                this.close_new_post(cx);
                this.open_channel_settings(channel.clone(), guild_id.clone(), window, cx);
            }))
        });
        let close = ui::icon_button("new-post-guidelines-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.guidelines = false, cx)));
        let measured = window.use_keyed_state(SharedString::from(format!("new-post-guidelines-{}", forum.id)), cx, |_, _| 0f32);
        let overflows = *measured.read(cx) > GUIDELINES_HEIGHT;
        let measure = measured.clone();
        let text = div()
            .max_h(px(if full { GUIDELINES_HEIGHT * 2. } else { GUIDELINES_HEIGHT }))
            .child(div().child(TextView::markdown("new-post-guidelines-markdown", crate::chat::discord_breaks(topic)).selectable(true)))
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(content) = bounds.first() else { return };
                let next = f32::from(content.size.height);
                measure.update(cx, |h, cx| {
                    if (*h - next).abs() > 0.5 {
                        *h = next;
                        cx.notify(); // lint-allow(global-notify): notifies the measured height's own entity, not Waffle
                    }
                });
            })
            .id("new-post-guidelines-text")
            .overflow_y_scrollbar();
        let expand = overflows.then(|| {
            ui::button("new-post-guidelines-expand", ui::Tone::Link, cx)
                .child(if full { "Collapse" } else { "Expand" })
                .on_click(cx.listener(|this, _, _, cx| this.change_composer(|c| c.guidelines_full = !c.guidelines_full, cx)))
        });
        let head = ui::row()
            .gap_1()
            .child(Icon::new(Glyph::BookOpen).size(ui::ICON_MD).text_color(palette::muted(cx)))
            .child(div().flex_1().pl_1().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Post Guidelines"))
            .children(edit)
            .child(close);
        Some(
            ui::col()
                .p_3()
                .gap_2()
                .squircle(ui::RADIUS_MD)
                .bg(palette::surface(Level::Sidebar, cx))
                .border_1()
                .border_color(palette::line(cx))
                .child(head)
                .child(text)
                .children(expand),
        )
    }
}
