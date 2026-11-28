// The picker's popups: the command list, the option names and choices, and the hint bar.
use crate::ui;

use super::*;

// RENDERING

/// How a rail tile looks chosen and not: the fill and ink of each, and the fill under the pointer.
pub(crate) struct RailTint {
    pub on_fill: gpui_kit::Hsla,
    pub on_ink: gpui_kit::Hsla,
    pub off_fill: Option<gpui_kit::Hsla>,
    pub off_ink: gpui_kit::Hsla,
    pub hover: gpui_kit::Hsla,
}

// A picker rail's square tile: its face centred, the chosen one filled, a tooltip naming it.
pub(crate) fn rail_slot(id: impl Into<gpui_kit::ElementId>, face: AnyElement, tip: SharedString, tint: RailTint, on: bool) -> gpui_kit::Stateful<Div> {
    ui::row() // lint-allow(custom-button): ui::Button takes a glyph, and a rail tile's face is an app icon or a section mark
        .id(id)
        .size_9()
        .flex_none()
        .justify_center()
        .squircle(RADIUS_SM)
        .cursor_pointer()
        .text_color(if on { tint.on_ink } else { tint.off_ink })
        .when(on, |d| d.bg(tint.on_fill))
        .when(!on, |d| d.when_some(tint.off_fill, |d, fill| d.bg(fill)).hover(move |s| s.bg(tint.hover)))
        .child(face)
        .tooltip(move |window, cx| ui::Tooltip::new(tip.clone()).build(window, cx))
}

impl Waffle {
    /// The picker, or the hint bar for the command being typed, floating above the composer card.
    pub(crate) fn command_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.command_picker.open {
            return self.picker_visible().then(|| self.picker_popup(cx));
        }
        let typed = self.typed_command(cx).filter(|_| !self.command_picker.hidden)?;
        Some(self.options_popup(&typed, cx))
    }

    pub(super) fn picker_popup(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match self.command_index() {
            None | Some(IndexState::Loading) => loading_rows(3, cx).into_any_element(),
            Some(IndexState::Failed(error)) => self.index_error(error, cx).into_any_element(),
            Some(IndexState::Ready(index)) if index.commands.is_empty() && self.command_rows().is_empty() => picker_note("No apps here have commands", "Apps added to this server show their commands here.", cx).into_any_element(),
            Some(IndexState::Ready(index)) => self.command_picker_body(index, cx),
        };
        let foot = popup_foot(cx)
            .child(ui::key_hint(ui::row().items_stretch().gap_1().child(ui::Kbd::glyph(Glyph::ArrowUp)).child(ui::Kbd::glyph(Glyph::ArrowDown)), "Move", cx))
            .child(ui::key_hint(ui::Kbd::new("Tab"), "Pick", cx))
            .child(ui::key_hint(ui::Kbd::new("Esc"), "Close", cx));
        popup("command-picker", [body, foot.into_any_element()], cx)
    }

    pub(super) fn command_picker_body(&self, index: &CommandIndex, cx: &mut Context<Self>) -> AnyElement {
        let query = self.command_picker.query.clone();
        let ranks = self.command_ranks(index);
        let rows = self.command_rows();
        if rows.is_empty() {
            return picker_note(format!("No commands match /{query}"), "Try another name, or a word from what it does.", cx).into_any_element();
        }
        let picks = pickable(&rows);
        let chosen = picks.get(self.command_picker.selected).copied();
        let list = rows.iter().enumerate().map(|(row, r)| match r {
            ListRow::Header(section) => section_header(index, *section, &query, cx).into_any_element(),
            ListRow::Command { command, name, description } => {
                let pick = picks.iter().position(|&p| p == row).unwrap_or_default();
                self.command_row(index, *command, name.clone(), description.clone(), chosen == Some(row), pick, cx).into_any_element()
            }
            ListRow::Tts { name } => {
                let pick = picks.iter().position(|&p| p == row).unwrap_or_default();
                self.tts_row(name.clone(), chosen == Some(row), pick, cx).into_any_element()
            }
        });
        let list = ui::col().min_h_0().id("command-list").flex_1().max_h(px(LIST_HEIGHT)).p_1p5().gap_0p5().overflow_y_scroll().track_scroll(&self.command_picker.scroll).children(list);
        if !query.is_empty() {
            return list.into_any_element();
        }
        // The browser: a rail of its sections beside the list.
        let current = self.command_picker.filter.map(Section::App).or_else(|| chosen.map(|row| section_of(&rows, row)));
        let sections = browser_sections(index, &ranks);
        let mut rail: Vec<AnyElement> = Vec::new();
        for section in sections {
            rail.push(self.rail_tile(index, section, current == Some(section), cx));
            if section == Section::Frecency {
                rail.push(div().w_6().h(ui::HAIRLINE).my_0p5().bg(palette::line(cx)).into_any_element()); // lint-allow(custom-divider, raw-margin): ui::Separator lacks a short centred rule in a rail; the margin is its optical gap
            }
        }
        ui::row().items_stretch()
            .min_h_0()
            .child(ui::col().w(px(RAIL_WIDTH)).flex_none().py_2().items_center().gap_1().border_r_1().border_color(palette::line(cx)).children(rail))
            .child(list)
            .into_any_element()
    }

    // A section's tile in the rail: an app narrows the list to it; Recent, or the app already chosen,
    // shows everything again.
    pub(super) fn rail_tile(&self, index: &CommandIndex, section: Section, on: bool, cx: &mut Context<Self>) -> AnyElement { // lint-allow(custom-component): ui::server_tile lacks a section rail tile with a selected bar
        let (selected, hover) = (palette::selected(cx), palette::hover(cx));
        let (face, name): (AnyElement, SharedString) = match section {
            Section::App(a) => (app_tile(&index.apps[a], 28., cx), index.apps[a].name.clone().into()),
            _ => (Icon::new(Glyph::Recent).size(ui::ICON_LG).into_any_element(), "Recent".into()),
        };
        let id = match section {
            Section::App(a) => format!("command-rail-{a}"),
            _ => "command-rail-recent".into(),
        };
        let tint = RailTint { on_fill: selected, on_ink: palette::strong(cx), off_fill: None, off_ink: palette::muted(cx), hover };
        rail_slot(SharedString::from(id), face, name, tint, on)
            .on_click(cx.listener(move |this, _, _, cx| {
                let picker = &mut this.command_picker;
                picker.filter = match section {
                    Section::App(a) if picker.filter != Some(a) => Some(a),
                    _ => None,
                };
                picker.selected = 0;
                picker.scroll.scroll_to_item(0);
                this.redraw(Column::Main, cx);
            }))
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn command_row(&self, index: &CommandIndex, command: usize, name: Option<Range<usize>>, description: Option<Range<usize>>, on: bool, pick: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = &index.commands[command];
        let app = index.apps.iter().find(|a| a.id == c.application_id);
        let (strong, muted, faint, accent) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::accent_ink(cx));
        let mark = |range: Option<Range<usize>>, shift: usize| range.map(|r| (r.start + shift..r.end + shift, HighlightStyle { color: Some(accent), ..Default::default() }));
        let args: Vec<&str> = c.options.iter().filter(|o| o.required).map(CommandOption::display_name).collect();
        let title = ui::row()
            .items_baseline()
            .gap_1p5()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(StyledText::new(format!("/{}", c.display_name())).with_highlights(mark(name, 1))))
            .when(!args.is_empty(), |d| d.child(ui::line(args.join(" ")).text_size(ui::TEXT_SM).text_color(faint)));
        let what = ui::col().min_h_0()
            .flex_1()
            .child(title)
            .child(div().min_w_0().truncate().child(StyledText::new(c.description.clone()).with_highlights(mark(description, 0))).text_size(ui::TEXT_SM).text_color(muted)); // lint-allow(raw-truncate): ui::line takes plain text; this one is a highlighted StyledText (the match marks)
        let app = app.cloned().unwrap_or_else(|| app_commands::App { id: c.application_id.clone(), name: String::new(), icon: None });
        ui::DropdownMenuItem::new(SharedString::from(format!("command-{command}")), cx)
            .highlighted(on)
            .min_h_11()
            .gap_3()
            .child(app_tile(&app, 32., cx))
            .child(what)
            .child(div().flex_none().text_size(ui::TEXT_XS).text_color(faint).child(app.name))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.command_picker.selected = pick;
                this.pick_selected_command(window, cx);
            }))
    }

    // Discord's built-in /tts, drawn like an app's command.
    pub(super) fn tts_row(&self, name: Option<Range<usize>>, on: bool, pick: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (strong, muted, faint, accent) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::accent_ink(cx));
        let name = name.map(|r| (r.start + 1..r.end + 1, HighlightStyle { color: Some(accent), ..Default::default() }));
        let title = ui::row()
            .items_baseline()
            .gap_1p5()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(StyledText::new(send_options::TTS).with_highlights(name)))
            .child(ui::line("message").text_size(ui::TEXT_SM).text_color(faint));
        let what = ui::col().min_h_0().flex_1().child(title).child(ui::line(send_options::TTS_DESCRIPTION).text_size(ui::TEXT_SM).text_color(muted));
        ui::DropdownMenuItem::new("command-tts", cx)
            .highlighted(on)
            .min_h_11()
            .gap_3()
            .child(ui::row().size_8().flex_none().justify_center().child(crate::chat::suggest_lead(&SuggestLead::Command(Glyph::VolumeHigh), cx)))
            .child(what)
            .child(div().flex_none().text_size(ui::TEXT_XS).text_color(faint).child(BUILT_IN))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.command_picker.selected = pick;
                this.pick_selected_command(window, cx);
            }))
    }

    pub(super) fn index_error(&self, error: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::row()
            .p_3p5()
            .gap_3()
            .child(div().text_color(palette::danger_ink(cx)).child(Icon::new(Glyph::WarningCircle).size(ui::ICON_XL)))
            .child(
                ui::col().min_h_0()
                    .flex_1()
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child("Couldn't load this server's commands"))
                    .child(ui::line(error.to_owned()).text_size(ui::TEXT_SM).text_color(palette::muted(cx))),
            )
            .child(ui::Button::new("command-index-retry").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Try again").on_click(cx.listener(|this, _, _, cx| this.fetch_command_index(cx))))
    }

    // The hint bar: the option names outside the boxes, or what the caret's option wants and its choices.
    pub(super) fn options_popup(&self, typed: &Typed, cx: &mut Context<Self>) -> AnyElement {
        if let Some((_, query)) = typed.naming()
            && !typed.open_options(query).is_empty()
        {
            return self.names_popup(typed, query, cx);
        }
        let highlighted = self.command_picker.highlighted;
        let list = match self.suggestions(typed) {
            Some(Suggestions::Loading) => Some(loading_rows(AUTOCOMPLETE_SKELETONS, cx).into_any_element()),
            Some(Suggestions::Failed) => Some(list_note("Loading options failed", cx).into_any_element()),
            Some(Suggestions::Ready([])) => Some(list_note("No options match your search", cx).into_any_element()),
            _ => {
                let choices = self.option_choices(typed);
                (!choices.is_empty()).then(|| {
                    let rows = choices.into_iter().enumerate().map(|(i, choice)| self.choice_row(i, choice, i == highlighted, cx));
                    ui::col().id("command-choices").max_h(px(CHOICE_LIST_MAX)).p_1p5().gap_0p5().overflow_y_scroll().children(rows).into_any_element()
                })
            }
        };
        let app = typed.index.apps.iter().find(|a| a.id == typed.command().application_id).map(|a| a.name.clone()).unwrap_or_default();
        let suggesting = matches!(self.suggestions(typed), Some(Suggestions::Ready([_, ..])));
        let from = suggesting.then(|| ui::line(format!("Suggestions from {app}")).flex_1());
        let pick = self.wants_pick(typed);
        let foot = popup_foot(cx)
            .children(from)
            .child(div().flex_1())
            .when(self.command_picker.staging, |d| d.child("Uploading…"))
            .child(ui::key_hint(ui::Kbd::new("Tab"), if pick { "Pick" } else { "Next option" }, cx))
            .child(ui::key_hint(ui::Kbd::new("Enter"), if pick { "Pick" } else { "Run" }, cx));
        let hint = self.hint_bar(typed, cx).into_any_element();
        popup("command-options", [Some(hint), list, Some(foot.into_any_element())].into_iter().flatten(), cx)
    }

    // Outside the boxes: the options still to give whose name starts with what's typed, under Discord's
    // headers.
    pub(super) fn names_popup(&self, typed: &Typed, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let options = typed.open_options(query);
        let command = typed.command();
        let required = options.iter().filter(|&&o| command.options[o].required).count();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, &option) in options.iter().enumerate() {
            if i == 0 && required > 0 {
                rows.push(list_header("Required options", cx).into_any_element());
            }
            if i == required {
                rows.push(list_header(if required > 0 { "Other options" } else { "Options" }, cx).into_any_element());
            }
            let on = self.names_on(query) && i == self.command_picker.highlighted;
            rows.push(self.name_row(&command.options[option], option, on, cx).into_any_element());
        }
        let body = ui::col().id("command-option-names").max_h(px(CHOICE_LIST_MAX)).p_1p5().gap_0p5().overflow_y_scroll().children(rows);
        let foot = popup_foot(cx)
            .child(ui::key_hint(ui::row().items_stretch().gap_1().child(ui::Kbd::glyph(Glyph::ArrowUp)).child(ui::Kbd::glyph(Glyph::ArrowDown)), "Move", cx))
            .child(div().flex_1())
            .child(ui::key_hint(ui::Kbd::new("Tab"), "Add", cx))
            .child(ui::key_hint(ui::Kbd::new("Esc"), "Close", cx));
        popup("command-option-names-popup", [body.into_any_element(), foot.into_any_element()], cx)
    }

    pub(super) fn name_row(&self, option: &CommandOption, i: usize, on: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ui::DropdownMenuItem::new(SharedString::from(format!("command-option-{i}")), cx)
            .highlighted(on)
            .gap_2p5()
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(option.display_name().to_owned()))
            .child(ui::line(option.description.clone()).flex_1().text_color(palette::muted(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_name(i, window, cx)))
    }

    // Discord's header bar: the app, the command, the caret's option with its description (or, when its
    // value can't send, the reason in red in its place), and the optional options still to add.
    pub(super) fn hint_bar(&self, typed: &Typed, cx: &mut Context<Self>) -> Div {
        let (strong, muted, faint, danger) = (palette::strong(cx), palette::muted(cx), palette::faint(cx), palette::danger_ink(cx));
        let command = typed.command();
        let optional: Vec<(usize, String)> = command.options.iter().enumerate().filter(|(i, o)| !o.required && !typed.line.has(*i)).map(|(i, o)| (i, o.display_name().to_owned())).collect();
        let add = (!optional.is_empty()).then(|| {
            let count = format!("+{} optional", optional.len());
            ui::row().flex_none().gap_1p5().text_size(ui::TEXT_XS).text_color(faint).child(count).children(optional.into_iter().map(|(i, name)| {
                ui::Toggle::new(SharedString::from(format!("add-option-{i}"))).pressed(false).icon(Glyph::Plus).child(name)
                    .h_6()
                    .px_2()
                    .text_size(ui::TEXT_XS)
                    .on_click(cx.listener(move |this, _, window, cx| this.add_option(i, window, cx)))
            }))
        });
        let app = typed.index.apps.iter().find(|a| a.id == command.application_id).cloned();
        let head = ui::row()
            .gap_2p5()
            .children(app.map(|a| app_tile(&a, 20., cx)))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("/{}", command.display_name())));
        let head = match typed.arg() {
            Some((_, arg)) => {
                let option = typed.option(arg);
                let problem = self.box_problem(typed, arg);
                let detail = match &problem {
                    Some(why) => ui::row().flex_1().gap_1p5().text_color(danger).child(Icon::new(Glyph::WarningCircle).size(ui::ICON_MD)).child(ui::line(why.clone())),
                    None => ui::line(option.description.clone()).flex_1().text_color(muted),
                };
                head.child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(strong).child(option.display_name().to_owned())).child(detail).when(option.required && problem.is_none(), |d| d.child(required_mark(cx)))
            }
            None => head.child(ui::line(command.description.clone()).flex_1().text_color(muted)),
        };
        ui::col().px_3p5().py_2p5().gap_1p5().border_b_1().border_color(palette::line(cx)).text_size(ui::TEXT_SM).child(head.children(add))
    }

    pub(super) fn choice_row(&self, i: usize, choice: OptionChoice, on: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let lead = match &choice.lead {
            ChoiceLead::None => None,
            ChoiceLead::Suggest(lead) => Some(crate::chat::suggest_lead(lead, cx)),
            ChoiceLead::Role(color) => Some(ui::row().size_7().flex_none().justify_center().child(div().size_3().rounded_full().bg(role_dot(*color, cx))).into_any_element()),
            ChoiceLead::Glyph(glyph) => Some(ui::row().size_7().flex_none().justify_center().text_color(palette::muted(cx)).child(Icon::new(*glyph).size(ui::ICON_LG)).into_any_element()),
        };
        ui::DropdownMenuItem::new(SharedString::from(format!("command-choice-{i}")), cx)
            .highlighted(on)
            .children(lead)
            .child(ui::line(choice.label).flex_1().text_color(palette::strong(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_choice(i, window, cx)))
    }
}

pub(super) fn required_mark(cx: &App) -> Div {
    div().flex_none().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::danger_ink(cx)).child("Required")
}

// An app's icon, or its initial on a colour of its own.
pub(super) fn app_tile(app: &app_commands::App, size: f32, cx: &App) -> AnyElement {
    ui::app_tile(app.icon.as_deref(), &app.name, &app.id, size, cx)
}

pub(super) fn role_dot(color: u32, cx: &App) -> gpui_kit::Hsla {
    match color {
        0 => palette::muted(cx),
        color => palette::data_color(color),
    }
}

// The surface both popups share: the composer's width, 8px above it, rising from its foot.
pub(super) fn popup(id: &'static str, content: impl IntoIterator<Item = AnyElement>, cx: &App) -> AnyElement {
    ui::col() // lint-allow(custom-badge, custom-mark, custom-surface): ui::PopoverContent::new lacks an anchored composer popup that rises from the field
        .id(id)
        .occlude()
        .absolute()
        .left_neg_px()
        .right_neg_px()
        .bottom(relative(1.))
        .mb(px(POPUP_RAISE)) // lint-allow(raw-margin): lifts the popup clear of the composer by a computed amount, POPUP_RAISE
        .squircle(RADIUS_LG)
        .bg(palette::surface(Level::Overlay, cx))
        .shadow(ui::shadow_pop(cx))
        .text_size(ui::TEXT_SM)
        .children(content)
        .transform_origin(point(relative(0.5), relative(1.)))
        .with_animation(id, Animation::new(ui::DUR_POP).with_easing(gpui_kit::ease_out_quint()), |el, t| el.opacity(t).scale(0.95 + 0.05 * t))
        .into_any_element()
}

pub(super) fn popup_foot(cx: &App) -> Div {
    ui::row().gap_3p5().px_3p5().py_2().border_t_1().border_color(palette::line(cx)).text_size(ui::TEXT_XS).text_color(palette::faint(cx))
}

pub(super) fn list_header(title: &'static str, cx: &App) -> Div {
    div().flex_none().pt_2().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(title)
}

pub(super) fn list_note(text: &'static str, cx: &App) -> Div {
    div().px_4().py_3p5().text_color(palette::muted(cx)).child(text)
}

pub(super) fn section_header(index: &CommandIndex, section: Section, query: &str, cx: &App) -> Div {
    let header = ui::row().flex_none().gap_2().pt_2().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx));
    match section {
        Section::Frecency => header.child(Icon::new(Glyph::Recent).size(ui::ICON_SM)).child("Recent"),
        Section::App(a) => header.child(app_tile(&index.apps[a], 16., cx)).child(index.apps[a].name.clone()),
        // Discord's "Commands matching **{prefix}**".
        Section::Matching => header.child("Commands matching").child(div().text_color(palette::strong(cx)).child(query.to_owned())),
        Section::BuiltIn => header.child(BUILT_IN),
    }
}

pub(super) fn loading_rows(count: usize, _cx: &App) -> Div {
    let widths = [260., 200., 230., 180., 240.];
    let row = |n: usize| {
        let width = widths[n % widths.len()];
        ui::row()
            .min_h_11()
            .px_2p5()
            .gap_3()
            .child(ui::Skeleton::new(SharedString::from(format!("command-skeleton-tile-{n}"))).radius(RADIUS_SM).size_8().flex_none())
            .child(
                ui::col()
                    .gap_1p5()
                    .child(ui::Skeleton::new(SharedString::from(format!("command-skeleton-name-{n}"))).radius(4.).w(px(width * 0.4)).h_3())
                    .child(ui::Skeleton::new(SharedString::from(format!("command-skeleton-desc-{n}"))).radius(4.).w(px(width)).h_2p5()),
            )
    };
    ui::col().p_1p5().gap_0p5().children((0..count).map(row))
}

pub(super) fn picker_note(title: impl Into<SharedString>, body: impl Into<SharedString>, cx: &App) -> Div {
    ui::col()
        .px_4()
        .py_3p5()
        .gap_0p5()
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title.into()))
        .child(div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(body.into()))
}
