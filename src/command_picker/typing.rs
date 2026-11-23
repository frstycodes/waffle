// Reading a typed command back from the composer, filling its boxes, and running or blocking it.

use super::*;

// TYPING A COMMAND

impl Waffle {
    pub(super) fn typed_command(&self, cx: &App) -> Option<Typed> {
        if self.editing.is_some() {
            return None;
        }
        let index = self.ready_index()?;
        let state = self.composer.read(cx);
        let content = state.content();
        let line = command_line::parse(content.text(), &marks(&content), &index.commands, self.command_picker.preferred.as_deref())?;
        Some(Typed { index, line, caret: state.cursor(), content })
    }

    pub(crate) fn command_typed(&self, cx: &App) -> bool {
        self.typed_command(cx).is_some()
    }

    /// A restored draft's command; a `/` draft also needs the index to read it as one.
    pub(crate) fn restore_command(&mut self, command: Option<String>, picks: Vec<CommandPick>, cx: &mut Context<Self>) {
        self.command_picker.restore(command, picks);
        if self.composer.read(cx).value().starts_with('/') {
            self.ensure_command_index(cx);
        }
    }

    /// What a draft keeps of the command typed into it, if it is one.
    /// GAP: attached files aren't kept; their chips come back as plain text.
    pub(crate) fn command_draft(&self, cx: &App) -> (Option<String>, Vec<CommandPick>) {
        if !self.command_typed(cx) {
            return (None, Vec::new());
        }
        (self.command_picker.preferred.clone(), self.command_picker.picked.clone())
    }

    // What an option's value holds: a chip's id, a picked choice, an attached file, or typed text, with a
    // typed @name, #name or @everyone resolved to its id.
    pub(super) fn arg_value(&self, typed: &Typed, arg: &Arg) -> ArgValue {
        let option = typed.option(arg);
        let token = typed.value_token(arg);
        let text = typed.text()[arg.value.clone()].to_owned();
        if option.kind == OptionKind::Attachment {
            let chipped = token.is_some_and(|t| t.id().strip_prefix(FILE_TOKEN) == Some(option.name.as_str()));
            let file = chipped.then(|| self.command_picker.files.iter().position(|(o, _)| *o == option.name)).flatten();
            return ArgValue { text, file, ..Default::default() };
        }
        if let Some(token) = token {
            let id = command_line::mention_id(token.id()).to_owned();
            return ArgValue { channel_type: self.channel_type(&id), text: id, ..Default::default() };
        }
        let picked = self.command_picker.picked.iter().find(|p| p.option == option.name && p.label == text).map(|p| p.value.clone());
        let resolved = picked.is_none().then(|| self.resolve_name(option.kind, text.trim())).flatten();
        let text = resolved.unwrap_or(text);
        ArgValue { channel_type: self.channel_type(&text), text, picked, file: None }
    }

    // Discord resolves typed text to a mention for people, channels and roles.
    // Not verified: its matching rules; this takes an exact name, ignoring case.
    pub(super) fn resolve_name(&self, kind: OptionKind, text: &str) -> Option<String> {
        let guild = self.guild();
        let roles = || guild.into_iter().flat_map(|g| g.roles.iter());
        let role = |name: &str| match name {
            "@everyone" => guild.map(|g| g.id.clone()),
            _ => roles().find(|r| r.name.eq_ignore_ascii_case(name.trim_start_matches('@'))).map(|r| r.id.clone()),
        };
        let user = |name: &str| {
            let name = name.trim_start_matches('@');
            let members = self.guild_id.as_ref().and_then(|g| self.members.get(g)).into_iter().flat_map(|m| m.values());
            let mut people = members.chain(self.channel.iter().flat_map(|c| c.recipients.iter()));
            people.find(|u| u.username.eq_ignore_ascii_case(name) || u.name.eq_ignore_ascii_case(name)).map(|u| u.id.clone())
        };
        match kind {
            OptionKind::Role => role(text),
            OptionKind::User => user(text),
            OptionKind::Mentionable => role(text).or_else(|| user(text)),
            OptionKind::Channel => {
                let name = text.trim_start_matches('#');
                guild.into_iter().flat_map(|g| g.channels.iter().chain(&g.threads)).find(|c| c.name.eq_ignore_ascii_case(name)).map(|c| c.id.clone())
            }
            _ => None,
        }
    }

    pub(super) fn channel_type(&self, id: &str) -> Option<u8> {
        self.guild()?.channels.iter().chain(&self.guild()?.threads).find(|c| c.id == id).map(|c| c.raw_type)
    }

    /// The caret or selection moved, typing or not: the hint bar and the boxes follow it, and an
    /// autocomplete option it lands in asks its app for choices. A box the caret leaves empty turns red,
    /// as Discord validates the option it leaves with `allowEmptyValues: false`.
    pub(crate) fn on_composer_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.composer.read(cx).selected_range();
        if self.command_picker.selection == selection {
            return;
        }
        self.command_picker.selection = selection;
        self.command_picker.names_chosen = false;
        let Some(typed) = self.typed_command(cx) else { return };
        let focused = typed.arg().map(|(_, a)| a.option);
        if self.command_picker.focused != focused {
            let left = self.command_picker.focused.and_then(|o| typed.line.args.iter().find(|a| a.option == o));
            if let Some(left) = left.filter(|a| matches!(command_line::resolve(typed.option(a), &self.arg_value(&typed, a)), Ok(None)))
                && !self.command_picker.errored.contains(&left.option)
            {
                self.command_picker.errored.push(left.option);
            }
            self.command_picker.focused = focused;
            self.command_picker.highlighted = 0;
            self.schedule_autocomplete(cx);
        }
        self.redraw(Column::Main, cx);
    }

    /// The composer's text changed: false when it isn't a command. Option keys become chips, a command
    /// just typed gets its required boxes, and an autocomplete option asks its app again.
    pub(crate) fn on_command_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(typed) = self.typed_command(cx) else {
            self.command_picker.forget_typing();
            return false;
        };
        let picker = &mut self.command_picker;
        picker.hidden = false;
        picker.highlighted = 0;
        picker.names_chosen = false;
        self.box_new_keys(&typed, window, cx);
        if self.activate(&typed, window, cx) {
            return true;
        }
        self.keep_errors(&typed);
        self.schedule_autocomplete(cx);
        true
    }

    // A key typed or pasted as text becomes a box, as Discord's `pY` turns `name:` into a pill: the key a
    // chip, and an end chip after its value in place of the spaces trailing it. A caret in those spaces
    // lands at the value's end, inside. Only a newly keyed option is boxed, so undoing a box sticks.
    pub(super) fn box_new_keys(&mut self, typed: &Typed, window: &mut Window, cx: &mut Context<Self>) {
        let keyed = std::mem::replace(&mut self.command_picker.keyed, typed.line.args.iter().map(|a| a.option).collect());
        let chipped = |key: &Range<usize>| typed.content.tokens().iter().any(|t| t.range() == *key);
        let mut edits: Vec<(Range<usize>, InlineToken)> = Vec::new();
        for arg in typed.line.args.iter().filter(|a| !keyed.contains(&a.option)) {
            if !chipped(&arg.key) {
                edits.push((arg.key.clone(), option_token(&typed.text()[arg.key.clone()])));
            }
            if arg.end.is_none() {
                edits.push((arg.value.end..arg.extent.end, end_token()));
            }
        }
        if edits.is_empty() {
            return;
        }
        let ends = edits.iter().filter(|(_, token)| is_end_token(token)).map(|(range, token)| (range.clone(), token.text().len()));
        let caret = moved_caret(typed.caret, ends);
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        self.composer.update(cx, |state, cx| {
            for (range, token) in edits {
                state.replace_range_with_token(range, token, window, cx).ok();
            }
            state.set_selected_range(caret..caret, cx);
        });
    }

    // The text just became a command (`/name ` typed by hand): Discord adds every required option's
    // box, or, for a command whose one option is optional, turns the text typed after it into that
    // option's value. True when it edited the text (the edit comes back as another change).
    pub(super) fn activate(&mut self, typed: &Typed, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let after = &typed.text()[typed.line.name_end..];
        if after.is_empty() {
            return false;
        }
        let picker = &mut self.command_picker;
        if picker.activated != Some(typed.line.command) {
            picker.activated = Some(typed.line.command);
            picker.shortcut_taken = false;
            let keys: Vec<String> = typed.command().options.iter().filter(|o| o.required).map(key_text).collect();
            if typed.line.args.is_empty() && after.trim().is_empty() && typed.caret == typed.text().len() && !keys.is_empty() {
                let end = typed.text().len();
                let caret = end + keys[0].len();
                self.composer.update(cx, |state, cx| {
                    insert_boxes(state, end, &keys, window, cx);
                    state.set_selected_range(caret..caret, cx);
                });
                return true;
            }
        }
        let options = &typed.command().options;
        let lone = matches!(options.as_slice(), [only] if !only.required && only.kind != OptionKind::Attachment);
        if !lone || picker.shortcut_taken || !typed.line.args.is_empty() || after.trim().is_empty() {
            return false;
        }
        picker.shortcut_taken = true;
        let (at, end) = (typed.text().len() - after.trim_start().len(), typed.text().len());
        let key = key_text(&options[0]);
        let caret = typed.caret + key.len();
        self.composer.update(cx, |state, cx| {
            state.replace_range_with_token(end..end, end_token(), window, cx).ok();
            state.replace_range_with_token(at..at, option_token(&key), window, cx).ok();
            state.set_selected_range(caret..caret, cx);
        });
        true
    }

    // An errored box stays red until its value would send.
    pub(super) fn keep_errors(&mut self, typed: &Typed) {
        let still: Vec<usize> = typed.line.args.iter().filter(|a| self.command_picker.errored.contains(&a.option) && !self.value_sends(typed, a)).map(|a| a.option).collect();
        self.command_picker.errored = still;
    }

    pub(super) fn value_sends(&self, typed: &Typed, arg: &Arg) -> bool {
        matches!(command_line::resolve(typed.option(arg), &self.arg_value(typed, arg)), Ok(Some(_)))
    }

    // Why the caret's box can't send, as Discord's header says it in place of the description: checked
    // live once it has a value, or, empty, since the caret left it or the last Enter marked it.
    pub(super) fn box_problem(&self, typed: &Typed, arg: &Arg) -> Option<String> {
        let resolved = command_line::resolve(typed.option(arg), &self.arg_value(typed, arg));
        match resolved {
            Err(why) => Some(why),
            Ok(None) if self.command_picker.errored.contains(&arg.option) => Some(command_line::empty_message(typed.option(arg))),
            _ => None,
        }
    }

    pub(super) fn move_highlight(&mut self, typed: &Typed, delta: isize, cx: &mut Context<Self>) -> bool {
        let (count, unchosen) = match typed.naming() {
            Some((_, query)) => (typed.open_options(query).len(), !self.names_on(query)),
            None => (self.option_choices(typed).len(), false),
        };
        if count == 0 {
            return false;
        }
        let picker = &mut self.command_picker;
        picker.names_chosen = true;
        picker.highlighted = match unchosen {
            true if delta < 0 => count - 1,
            true => 0,
            false => (picker.highlighted as isize + delta).rem_euclid(count as isize) as usize,
        };
        self.redraw(Column::Main, cx);
        true
    }

    // Tab onto a box selects `range`, its whole value, so typing replaces it.
    pub(super) fn select_box(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.command_picker.highlighted = 0;
        self.composer.update(cx, |state, cx| state.set_selected_range(range, cx));
        self.redraw(Column::Main, cx);
    }

    // Past the last box, Discord puts the caret at the end of the text, outside every box, where the
    // option-name list shows. A command with no box yet gets the space after its name first.
    pub(super) fn caret_to_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let end = typed.text().len();
        let space = !typed.text().ends_with(char::is_whitespace) && !typed.open_options("").is_empty();
        self.command_picker.highlighted = 0;
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            if space {
                state.insert(" ", window, cx);
            }
        });
        self.redraw(Column::Main, cx);
    }

    // The name typed outside the boxes becomes `option`'s empty box, the caret in it.
    pub(super) fn pick_name(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((start, _)) = typed.naming() else { return };
        let key = key_text(&typed.command().options[option]);
        let space = !typed.text()[..start].ends_with(char::is_whitespace);
        let at = start + usize::from(space);
        let caret = typed.caret;
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(start..caret, cx);
            state.replace(if space { " " } else { "" }, window, cx);
            insert_boxes(state, at, std::slice::from_ref(&key), window, cx);
            state.set_selected_range(at + key.len()..at + key.len(), cx);
        });
        self.redraw(Column::Main, cx);
    }

    // Adds `option`'s empty box at the end, the caret in it: the hint bar's optional chips.
    pub(super) fn add_option(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let key = key_text(&typed.command().options[option]);
        let end = typed.text().len();
        let space = !typed.text().ends_with(char::is_whitespace);
        let at = end + usize::from(space);
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(end..end, cx);
            if space {
                state.insert(" ", window, cx);
            }
            insert_boxes(state, at, std::slice::from_ref(&key), window, cx);
            state.set_selected_range(at + key.len()..at + key.len(), cx);
            state.focus(window, cx);
        });
        self.redraw(Column::Main, cx);
    }

    // The highlighted choice replaces the value (a person, channel or role as a mention chip), then on to
    // the next box, as Discord does. An attachment's row opens the file picker instead.
    pub(super) fn pick_choice(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((n, arg)) = typed.arg() else { return };
        let Some(choice) = self.option_choices(&typed).into_iter().nth(choice) else { return };
        let name = typed.option(arg).name.clone();
        if choice.attach {
            return self.choose_command_file(name, window, cx);
        }
        let range = arg.inner();
        let picked = &mut self.command_picker.picked;
        picked.retain(|p| p.option != name);
        if choice.token.is_none() {
            picked.push(CommandPick { option: name, label: choice.label.clone(), value: choice.value });
        }
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            match choice.token {
                Some(token) => {
                    state.replace_with_token(token, window, cx).ok();
                }
                None => state.replace(choice.label, window, cx),
            }
        });
        self.next_box_after(n, window, cx);
    }

    pub(super) fn next_box_after(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(typed) = self.typed_command(cx) else { return };
        match typed.line.args.get(n + 1) {
            Some(next) => self.select_box(next.inner(), cx),
            None => self.caret_to_end(window, cx),
        }
    }

    /// Backspace and Delete (`back`: Backspace) where a box changes what they do, as Discord's `v` does
    /// with its pills: in an empty box, or an attachment's, they remove the box; at a value's edge they
    /// step out of it and act on what's beyond; just outside a box they step into it, deleting its last
    /// (or first) character, or removing it when it's empty. True when handled here; false lets the
    /// composer delete, from a caret this may have moved. A selection is always the composer's.
    pub(crate) fn on_command_delete(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(typed) = self.typed_command(cx) else { return false };
        if !self.composer.read(cx).selected_range().is_empty() {
            return false;
        }
        let caret = typed.caret;
        let removable = |a: &Arg| a.inner().is_empty() || typed.option(a).kind == OptionKind::Attachment;
        let mut beyond = caret;
        if let Some((_, arg)) = typed.arg().filter(|(_, a)| a.end.is_some()) {
            if removable(arg) {
                return self.remove_box(arg.span(), window, cx);
            }
            let inner = arg.inner();
            if caret != if back { inner.start } else { inner.end } {
                return false;
            }
            beyond = if back { arg.key.start } else { arg.span().end };
        }
        let neighbour = typed.line.args.iter().filter(|a| a.end.is_some()).find(|a| if back { a.span().end == beyond } else { a.key.start == beyond });
        let into = match neighbour {
            Some(arg) if removable(arg) => return self.remove_box(arg.span(), window, cx),
            Some(arg) => if back { arg.inner().end } else { arg.inner().start },
            None if !back && beyond == typed.text().len() => return true,
            None => beyond,
        };
        if into != caret {
            self.composer.update(cx, |state, cx| state.set_selected_range(into..into, cx));
        }
        false
    }

    pub(super) fn remove_box(&mut self, span: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(span, cx);
            state.replace("", window, cx);
        });
        true
    }

    // The composer's own file picker, one file, for an ATTACHMENT option: it fills the option's value
    // as a chip naming the file, within the same upload limit as a message.
    pub(super) fn choose_command_file(&mut self, option: String, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            this.update_in(cx, |this, window, cx| this.attach_command_file(option, path, window, cx)).ok();
        })
        .detach();
    }

    pub(super) fn attach_command_file(&mut self, option: String, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = PendingFile::new(path) else { return };
        let limit = crate::discord::upload_limit(self.my_account.premium_type, self.guild().map_or(0, |g| g.premium_tier));
        let mut admitted = Vec::new();
        self.send_error = attachments::admit(&mut admitted, vec![file], limit).map(Into::into);
        let Some(file) = admitted.pop() else { return self.redraw(Column::Main, cx) };
        let Some(typed) = self.typed_command(cx) else { return };
        let Some((n, arg)) = typed.line.args.iter().enumerate().find(|(_, a)| typed.option(a).name == option) else { return };
        let token = InlineToken::new(format!("{FILE_TOKEN}{option}"), file.name.clone());
        let range = arg.inner();
        let picker = &mut self.command_picker;
        picker.files.retain(|(o, _)| *o != option);
        picker.files.push((option, file));
        self.composer.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.replace_with_token(token, window, cx).ok();
            state.focus(window, cx);
        });
        self.next_box_after(n, window, cx);
    }

    /// What the caret's option offers under the hint bar, filtered by what's typed.
    pub(super) fn option_choices(&self, typed: &Typed) -> Vec<OptionChoice> {
        let Some((_, arg)) = typed.arg() else { return Vec::new() };
        let option = typed.option(arg);
        let typed_text = typed.text()[arg.value.clone()].trim();
        if option.kind == OptionKind::Attachment {
            return match self.arg_value(typed, arg).file {
                Some(_) => Vec::new(),
                None => vec![OptionChoice { attach: true, lead: ChoiceLead::Glyph(Glyph::Upload), ..OptionChoice::plain(ATTACH_FILE, Value::Null) }],
            };
        }
        if option.autocomplete {
            return match self.suggestions(typed) {
                Some(Suggestions::Ready(choices)) => choices.iter().map(|c| OptionChoice::plain(c.display_name(), c.value.clone())).collect(),
                _ => Vec::new(),
            };
        }
        if !option.choices.is_empty() {
            return option.choices.iter().filter(|c| find_ci(c.display_name(), typed_text).is_some()).map(|c| OptionChoice::plain(c.display_name(), c.value.clone())).collect();
        }
        match option.kind {
            OptionKind::Boolean => [("True", true), ("False", false)].into_iter().filter(|(name, _)| find_ci(name, typed_text).is_some_and(|r| r.start == 0)).map(|(name, v)| OptionChoice::plain(name, json!(v))).collect(),
            OptionKind::User => self.mention_choices('@', typed_text),
            OptionKind::Channel => self.channel_choices(option, typed_text),
            OptionKind::Role => self.role_choices(typed_text),
            OptionKind::Mentionable => self.mention_choices('@', typed_text).into_iter().chain(self.role_choices(typed_text)).collect(),
            _ => Vec::new(),
        }
    }

    // The composer's own @ and # suggestions, picked as the same mention chips.
    pub(super) fn mention_choices(&self, lead: char, typed: &str) -> Vec<OptionChoice> {
        let token = typed.rsplit(char::is_whitespace).next().unwrap_or_default().trim_start_matches(lead);
        let Some(auto) = self.suggest(&format!("{lead}{token}")) else { return Vec::new() };
        auto.items
            .into_iter()
            .map(|s| {
                let value = json!(command_line::mention_id(&s.wire));
                OptionChoice { token: Some(InlineToken::new(s.wire, s.insert.clone())), label: s.insert, value, lead: ChoiceLead::Suggest(s.lead), attach: false }
            })
            .collect()
    }

    // Channels the option takes (`channel_types`), as # suggestions.
    pub(super) fn channel_choices(&self, option: &CommandOption, typed: &str) -> Vec<OptionChoice> {
        let mut channels = self.mention_choices('#', typed);
        if !option.channel_types.is_empty() {
            channels.retain(|c| c.value.as_str().and_then(|id| self.channel_type(id)).is_none_or(|t| option.channel_types.contains(&t)));
        }
        channels
    }

    // Roles by name, @everyone (sent as the server's id) among them, as Discord offers it.
    pub(super) fn role_choices(&self, typed: &str) -> Vec<OptionChoice> {
        let Some(guild) = self.guild() else { return Vec::new() };
        let typed = typed.trim_start_matches('@');
        let mut roles: Vec<OptionChoice> = guild
            .roles
            .iter()
            .filter(|r| find_ci(&r.name, typed).is_some())
            .map(|r| {
                let label = if r.name.starts_with('@') { r.name.clone() } else { format!("@{}", r.name) };
                OptionChoice { token: Some(InlineToken::new(format!("<@&{}>", r.id), label.clone())), label, value: json!(r.id), lead: ChoiceLead::Role(r.color), attach: false }
            })
            .collect();
        roles.truncate(8);
        roles
    }

    pub(super) fn run_command(&mut self, typed: Typed, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, channel_id, _)) = self.command_scope() else { return };
        let command = typed.command().clone();
        let values = match command_line::validate(&command, &typed.line, |a| self.arg_value(&typed, a)) {
            Ok(values) => values,
            Err(problems) => return self.block_command(&typed, problems, window, cx),
        };
        let (values, uploads) = self.staged_values(&command, values);
        let app = typed.index.apps.iter().find(|a| a.id == command.application_id).cloned();
        let app = app.unwrap_or_else(|| app_commands::App { id: command.application_id.clone(), name: "App".into(), icon: None });
        if uploads.is_empty() {
            return self.send_command(command, app, values, Vec::new(), window, cx);
        }
        let Some(client) = self.client.clone() else { return };
        self.command_picker.staging = true;
        self.send_error = None;
        cx.spawn_in(window, async move |this, cx| {
            let staged = client.stage_files(&channel_id, &uploads).await;
            this.update_in(cx, |this, window, cx| {
                this.command_picker.staging = false;
                match staged {
                    Ok(files) => this.send_command(command, app, values, files, window, cx),
                    Err(error) => {
                        this.send_error = Some(format!("Couldn't upload the file: {error}").into());
                        this.redraw(Column::Main, cx);
                    }
                }
            })
            .ok();
        })
        .detach();
        self.redraw(Column::Main, cx);
    }

    // Attachment values hold an index into the picker's files; on the wire it's the file's index among
    // those this run uploads.
    pub(super) fn staged_values(&self, command: &AppCommand, mut values: Vec<(String, Value)>) -> (Vec<(String, Value)>, Vec<crate::discord::Upload>) {
        let mut uploads = Vec::new();
        for (name, value) in &mut values {
            let attachment = command.options.iter().any(|o| o.name == *name && o.kind == OptionKind::Attachment);
            let Some((_, file)) = value.as_u64().filter(|_| attachment).and_then(|i| self.command_picker.files.get(i as usize)) else { continue };
            *value = json!(uploads.len());
            uploads.push(file.upload());
        }
        (values, uploads)
    }

    pub(super) fn send_command(&mut self, command: AppCommand, app: app_commands::App, values: Vec<(String, Value)>, files: Vec<app_commands::CommandAttachment>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, channel_id, guild_id)) = self.command_scope() else { return };
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let mut body = app_commands::command_body(&command, &values, None, &origin);
        app_commands::attach_files(&mut body, &files);
        self.command_picker.recent.record(&command, guild_id.as_deref(), now_ms());
        self.interact(nonce, app, InteractionKind::Command, body, cx);
        self.command_picker.reset();
        self.composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.has_draft = !self.pending_files.is_empty();
        self.drafts.clear(&channel_id);
        self.save_drafts_soon(cx);
        cx.notify(); // lint-allow(global-notify): clears the draft the channel list marks and adds the pending message to the feed
    }

    // Enter that can't run, as Discord handles it: a box for every missing required option, every bad
    // box marked, the first one's value selected. Discord shakes the window; Waffle shakes the composer.
    pub(super) fn block_command(&mut self, typed: &Typed, problems: Vec<Problem>, window: &mut Window, cx: &mut Context<Self>) {
        let missing: Vec<String> = problems.iter().filter(|p| p.missing).map(|p| key_text(&typed.command().options[p.option])).collect();
        if !missing.is_empty() {
            let end = typed.text().len();
            let space = !typed.text().ends_with(char::is_whitespace);
            self.composer.update(cx, |state, cx| {
                state.set_selected_range(end..end, cx);
                if space {
                    state.insert(" ", window, cx);
                }
                insert_boxes(state, end + usize::from(space), &missing, window, cx);
            });
        }
        let first = problems[0].option;
        let picker = &mut self.command_picker;
        picker.errored = problems.iter().map(|p| p.option).collect();
        picker.hidden = false;
        picker.shake += 1;
        if let Some(typed) = self.typed_command(cx)
            && let Some(arg) = typed.line.args.iter().find(|a| a.option == first)
        {
            let value = arg.inner();
            self.composer.update(cx, |state, cx| state.set_selected_range(value, cx));
        }
        self.redraw(Column::Main, cx);
    }
}
