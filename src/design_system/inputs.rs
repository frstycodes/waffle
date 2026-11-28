// Inputs: Text field, Text area, Select, Stepper, Code cell, Swatch, Drop zone.
use super::*;

const DEVICES: [&str; 3] = ["MacBook Pro Microphone (Built-in)", "Studio Display Microphone", "AirPods Pro"];

fn variant(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

impl DesignSystem {
    pub(super) fn text_field_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let f = &self.fields;
        let focus = |state: &Entity<InputState>, cx: &App| state.read(cx).focus_handle(cx);
        let input = |id: &'static str, state: &Entity<InputState>, cx: &App| ui::Input::new(id, Input::new(state).appearance(false).px_0().text_size(ui::TEXT_MD), &focus(state, cx));
        let grid = div()
            .grid()
            .grid_cols(3)
            .gap_6()
            .child(ui::Field::new("Display name", cx).child(input("f-name", &f.name, cx)).child(ui::FieldDescription::new("Shown to everyone", Some("3/32".into()), cx)))
            .child(ui::Field::new("Search", cx).child(input("f-search", &f.search, cx).leading(Icon::new(Glyph::Search).size(ui::ICON_LG)).trailing(ui::KbdGroup::new(["⌘", "F"]))))
            .child(ui::Field::new("Channel name", cx).child(input("f-channel", &f.channel, cx).leading(div().text_size(ui::TEXT_MD).child("#"))))
            .child(ui::Field::new("Password", cx).child(input("f-password", &f.password, cx).trailing(self.show_password(cx))))
            .child(ui::Field::new("Invite link", cx).child(input("f-invite", &f.invite, cx).error(true)).child(ui::FieldError::new("This invite has expired. Ask for a new one.", cx)))
            .child(ui::Field::new("Username", cx).child(input("f-username", &f.username, cx).disabled(true)).child(ui::FieldDescription::new("You can change this once every 30 days", None, cx)));
        vec![variant("Text field / State and kind", "Labels above, help and errors below. Focus is the accent border plus a soft ring. Errors say what happened and how to fix it, and keep what the user typed.", grid, cx)]
    }

    pub(super) fn text_area_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let about = &self.fields.about;
        let area = ui::Field::new("About me", cx)
            .max_w_96()
            .child(ui::Textarea::new("f-about", Textarea::new(about).appearance(false).px_0().text_size(ui::TEXT_MD), &about.read(cx).focus_handle(cx)))
            .child(ui::FieldDescription::new("Markdown and emoji work here", Some("0/190".into()), cx));
        vec![variant("Text area / State", "Grows from 88px; the count sits at the end of the hint line.", ui::col().child(area), cx)]
    }

    pub(super) fn select_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let open = self.device_open;
        let trigger = ui::Select::new("f-device", Some(Glyph::Mic), DEVICES[self.flow.device], open, cx).on_click(cx.listener(|this, _, _, cx| {
            this.device_open = !this.device_open;
            cx.notify();
        }));
        let menu = open.then(|| {
            ui::PopoverContent::new(cx).w_full().children(DEVICES.iter().enumerate().map(|(i, name)| {
                ui::DropdownMenuItem::new(("f-device-row", i), cx).child(ui::line(*name).flex_1()).child(ui::Checkbox::mark(if i == self.flow.device { Check::On } else { Check::Off }).size(ui::ICON_LG)).on_click(cx.listener(move |this, _, _, cx| {
                    this.flow.device = i;
                    this.device_open = false;
                    cx.notify();
                }))
            }))
        });
        let field = ui::Field::new("Input device", cx).w_80().child(ui::row().flex_1().child(trigger)).children(menu);
        let closed = ui::Field::new("Input device", cx).w_80().child(ui::row().flex_1().child(ui::Select::new("f-device-off", None, "Default", false, cx).disabled(true)));
        vec![variant(
            "Select / State",
            "A field that opens a menu under it: the open trigger keeps its edge, the chosen row carries the check, a pick closes the menu. Click the field.",
            row().gap_8().items_start().child(field).child(closed),
            cx,
        )]
    }

    pub(super) fn stepper_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let stepper = ui::InputGroup::stepper(
            "f-slowmode",
            format!("{}s", self.slowmode),
            cx.listener(|this, _, _, cx| {
                this.slowmode = this.slowmode.saturating_sub(5);
                cx.notify();
            }),
            cx.listener(|this, _, _, cx| {
                this.slowmode += 5;
                cx.notify();
            }),
            cx,
        );
        vec![variant("Stepper / State", "Minus and plus step by five; the value never goes below zero.", ui::Field::new("Slowmode", cx).w_48().child(stepper), cx)]
    }

    pub(super) fn code_cell_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let cells = [(Some('4'), false), (Some('8'), false), (Some('1'), false), (None, true), (None, false), (None, false)];
        vec![variant("Code cell / State", "Filled, focused (the next to type) and empty.", ui::Field::new("Two-factor code", cx).child(ui::row().items_stretch().gap_2().children(cells.into_iter().map(|(digit, focused)| ui::InputOTP::slot(digit, focused, cx)))), cx)]
    }

    pub(super) fn swatch_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let swatches = [(0.6, 0.2, 5.), (0.62, 0.17, 35.), (0.7, 0.15, 80.), (0.6, 0.14, 155.), (0.58, 0.12, 220.), (0.55, 0.17, 290.)];
        let sidebar = palette::surface(Level::Sidebar, cx);
        let row_ = ui::row().items_stretch().flex_wrap().gap_2().children(swatches.into_iter().enumerate().map(|(i, (l, c, h))| {
            ui::swatch(("swatch", i), palette::oklch(l, c, h), i == self.role_colour, sidebar).on_click(cx.listener(move |this, _, _, cx| {
                this.role_colour = i;
                cx.notify();
            }))
        }));
        vec![variant("Swatch / State", "Ringed and ticked when chosen.", ui::Field::new("Role colour", cx).child(row_), cx)]
    }

    pub(super) fn drop_zone_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let zone = div()
            .id("f-drop")
            .max_w_96()
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if this.drop_over == *over {
                    return;
                }
                this.drop_over = *over;
                cx.notify();
            }))
            .child(ui::drop_zone(self.drop_over, Glyph::Upload, "Drop files to upload", cx).child(ui::row().gap_1().child("Up to 25 MB each, or").child(ui::Button::new("f-browse").variant(ui::ButtonVariant::Link).child("browse files"))));
        vec![variant("Drop zone / State", "Hover it to see the dragging-over state.", ui::col().child(zone), cx)]
    }

    // The password field's eye: 28px, tucked 6px into the field's padding. The slot is narrowed rather than
    // given a -6px margin, which taffy mis-measures (see `ui::stacked`).
    pub(super) fn show_password(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.password_shown { "Hide password" } else { "Show password" };
        let toggle = ui::Button::new("f-password-eye").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Eye).tooltip_text(label).w_7().h_7().on_click(cx.listener(|this, _, window, cx| {
            this.password_shown = !this.password_shown;
            let masked = !this.password_shown;
            this.fields.password.update(cx, |state, cx| state.set_masked(masked, window, cx));
            cx.notify();
        }));
        div().w(px(TOGGLE_SLOT)).h_7().child(toggle)
    }
}
