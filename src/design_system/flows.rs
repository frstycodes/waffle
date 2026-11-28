// Flow frames: a component in use, start to finish, with the states between. Everything is live and resets itself.
use super::*;

const SAVE_MS: u64 = 1200;
const SAVED_MS: u64 = 1500;
const UNDO_MS: u64 = 4000;
const COPIED_MS: u64 = 2400;
// The dialog stage: tall enough for the dialog, wide enough to show the page behind it.
const STAGE_HEIGHT: f32 = 340.;
const DIALOG_WIDTH: f32 = 400.;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Save {
    #[default]
    Idle,
    Saving,
    Saved,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Leave {
    #[default]
    Idle,
    Confirm,
    Left,
}

#[derive(Default)]
pub(super) struct Flows {
    pub(super) save: Save,
    pub(super) leave: Leave,
    pub(super) join: Option<Result<(), &'static str>>,
    pub(super) noise: bool,
    pub(super) undo: bool,
    pub(super) undo_run: u32,
    pub(super) parts: [bool; 3],
    pub(super) copied: bool,
    pub(super) dismissed: bool,
    pub(super) dialog: bool,
    pub(super) deleted: bool,
    pub(super) device: usize,
}

impl DesignSystem {
    pub(super) fn flow_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let frame = match self.page {
            Page::Button => self.buttons_flows(cx),
            Page::Input => self.fields_flow(cx),
            Page::Switch | Page::Checkbox => self.choices_flows(self.page, window, cx),
            Page::Sonner | Page::Alert => self.notices_flows(self.page, cx),
            Page::Dialog => self.dialog_flow(window, cx),
            _ => return Vec::new(),
        };
        vec![(Kind::Flows, frame.into_any_element())]
    }

    pub(super) fn after(&self, ms: u64, cx: &mut Context<Self>, then: impl FnOnce(&mut Self) + 'static) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(ms)).await;
            this.update(cx, |this, cx| {
                then(this);
                cx.notify();
            })
            .ok();
        })
        // lint-allow(detached-task): ends itself after its timer and holds only a weak handle
        .detach();
    }

    fn buttons_flows(&self, cx: &mut Context<Self>) -> Div {
        let (saving, saved) = (self.flow.save == Save::Saving, self.flow.save == Save::Saved);
        let save = ui::Button::new("fl-save").loading(saving.then_some("Saving…")).children(saved.then(|| glyph(Glyph::Checkmark))).child(if saved { "Saved" } else { "Save changes" }).on_click(cx.listener(|this, _, _, cx| {
            if this.flow.save != Save::Idle {
                return;
            }
            this.flow.save = Save::Saving;
            this.after(SAVE_MS, cx, |this| this.flow.save = Save::Saved);
            this.after(SAVE_MS + SAVED_MS, cx, |this| this.flow.save = Save::Idle);
            cx.notify();
        }));
        let go = |leave: Leave| cx.listener(move |this, _, _, cx| {
            this.flow.leave = leave;
            if leave == Leave::Left {
                this.after(UNDO_MS, cx, |this| this.flow.leave = Leave::Idle);
            }
            cx.notify();
        });
        let leave = match self.flow.leave {
            Leave::Idle => ui::Button::new("fl-leave").variant(ui::ButtonVariant::Destructive).child("Leave server").on_click(go(Leave::Confirm)).into_any_element(),
            Leave::Confirm => ui::Alert::new(Notice::Danger, Glyph::Warning, "Leave Night Shift?", "You'll need a new invite to rejoin.", cx)
                .child(row().child(ui::Button::new("fl-leave-yes").variant(ui::ButtonVariant::Destructive).size(ui::ButtonSize::Sm).child("Leave server").on_click(go(Leave::Left))).child(ui::Button::new("fl-leave-no").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child("Stay").on_click(go(Leave::Idle))))
                .into_any_element(),
            Leave::Left => ui::Sonner::toast_with("fl-left", Glyph::Checkmark, palette::positive_ink(cx), "Left Night Shift", None, Some(ui::Button::new("fl-undo-leave").variant(ui::ButtonVariant::Link).size(ui::ButtonSize::Sm).child("Undo").on_click(go(Leave::Idle)).into_any_element()), cx),
        };
        section("Save, then confirm a destructive step", "Saving keeps the button's width and shows the wave, then says Saved and rests. Leaving a server asks once, in place, and the undo toast outlasts a second thought.", cx).child(
            board(cx).child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_6()
                    .child(group("Save: idle, saving, saved", cx).child(row().child(save).child(note("Click it", cx))))
                    .child(group("Leave: button, confirm, undo toast", cx).child(div().min_h_16().child(leave))),
            ),
        )
    }

    fn fields_flow(&self, cx: &mut Context<Self>) -> Div {
        let state = &self.fields.join;
        let input = ui::Input::new("fl-join", Input::new(state).appearance(false).px_0().text_size(ui::TEXT_MD), &state.read(cx).focus_handle(cx)).error(matches!(self.flow.join, Some(Err(_))));
        let join = ui::Button::new("fl-join-go").child("Join server").on_click(cx.listener(|this, _, _, cx| {
            let value = this.fields.join.read(cx).value().trim().to_owned();
            this.flow.join = Some(match value.as_str() {
                "" => Err("Paste an invite link first."),
                v if v.ends_with("-old") => Err("This invite has expired. Ask for a new one."),
                _ => Ok(()),
            });
            cx.notify();
        }));
        let result = match self.flow.join {
            Some(Err(message)) => Some(ui::FieldError::new(message, cx).into_any_element()),
            Some(Ok(())) => Some(ui::Alert::new(Notice::Success, Glyph::CheckCircle, "You're in", "Night Shift is in your server list.", cx).into_any_element()),
            None => None,
        };
        section("Validate on submit", "Nothing is wrong until the user asks: errors appear under the field on Join, keep what was typed, and say how to fix it. Clear the field, or drop the \"-old\", then Join again.", cx).child(
            board(cx).child(ui::col().w_96().gap_2().child(ui::Field::new("Invite link", cx).child(input).children(result)).child(row().child(join)))
        )
    }

    fn choices_flows(&self, page: Page, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let on = self.flow.noise;
        let toggle = ui::toggle_row("fl-noise", "Noise suppression", Some("Applies at once".into()), on, window, cx).on_click(cx.listener(|this, _, _, cx| this.flip_noise(cx)));
        let undo = self.flow.undo.then(|| {
            let button = ui::Button::new("fl-noise-undo").variant(ui::ButtonVariant::Link).size(ui::ButtonSize::Sm).child("Undo").on_click(cx.listener(|this, _, _, cx| {
                this.flow.undo = false;
                this.flip_noise(cx);
                this.flow.undo = false;
            }));
            ui::Sonner::toast_with("fl-noise-toast", Glyph::Checkmark, palette::positive_ink(cx), if on { "Noise suppression on" } else { "Noise suppression off" }, None, Some(button.into_any_element()), cx)
        });
        let all = self.flow.parts.iter().all(|p| *p);
        let none = !self.flow.parts.iter().any(|p| *p);
        let parent = ui::Checkbox::new("fl-all").checked(if all { Check::On } else if none { Check::Off } else { Check::Mixed });
        let names = ["Messages", "Mentions", "Reactions"];
        let children: Vec<_> = names.iter().enumerate().map(|(i, name)| {
            let mark = ui::Checkbox::new(("fl-part", i)).checked(if self.flow.parts[i] { Check::On } else { Check::Off });
            ui::pick_row(("fl-part-row", i), mark, *name, None, cx).on_click(cx.listener(move |this, _, _, cx| {
                this.flow.parts[i] = !this.flow.parts[i];
                cx.notify();
            }))
        }).collect();
        section("Apply at once, and pick many", "A switch has no Save: it applies, then offers Undo for a few seconds. A parent checkbox is mixed while only some children are on, and a click settles it to all, then none.", cx).child(
            board(cx).child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_6()
                    .children((page == Page::Switch).then(|| group("Switch with an undo toast", cx).child(toggle).child(div().min_h_16().children(undo))))
                    .children((page == Page::Checkbox).then(|| {
                        group("Parent and children", cx)
                            .child(ui::pick_row("fl-all-row", parent, "Notify me about", Some("Mixed until all or none".into()), cx).on_click(cx.listener(move |this, _, _, cx| {
                                this.flow.parts = [!all; 3];
                                cx.notify();
                            })))
                            .child(ui::col().pl_6().children(children))
                    })),
            ),
        )
    }

    fn flip_noise(&mut self, cx: &mut Context<Self>) {
        self.flow.noise = !self.flow.noise;
        self.flow.undo = true;
        self.flow.undo_run += 1;
        let run = self.flow.undo_run;
        self.after(UNDO_MS, cx, move |this| {
            if this.flow.undo_run == run {
                this.flow.undo = false;
            }
        });
        cx.notify();
    }

    fn notices_flows(&self, page: Page, cx: &mut Context<Self>) -> Div {
        let copy = ui::Button::new("fl-copy").variant(ui::ButtonVariant::Secondary).child(glyph(Glyph::Copy)).child("Copy invite link").on_click(cx.listener(|this, _, _, cx| {
            this.flow.copied = true;
            this.after(COPIED_MS, cx, |this| this.flow.copied = false);
            cx.notify();
        }));
        let toast = self.flow.copied.then(|| ui::Sonner::toast("fl-copied", Glyph::Checkmark, palette::positive_ink(cx), "Invite link copied", None, cx));
        let notice = match self.flow.dismissed {
            false => ui::Alert::new(Notice::Info, Glyph::InfoFill, "Slowmode is on", "You can send one message every 10 seconds.", cx)
                .child(ui::Button::new("fl-dismiss").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::X).tooltip_text("Dismiss").self_center().on_click(cx.listener(|this, _, _, cx| {
                    this.flow.dismissed = true;
                    cx.notify();
                })))
                .into_any_element(),
            true => ui::Button::new("fl-restore").variant(ui::ButtonVariant::Link).child("Show the notice again").on_click(cx.listener(|this, _, _, cx| {
                this.flow.dismissed = false;
                cx.notify();
            })).into_any_element(),
        };
        section("A toast that leaves, a notice you can dismiss", "A toast confirms what just happened and leaves by itself after about two seconds. A callout stays until dismissed, and what dismisses it can bring it back.", cx).child(
            board(cx).child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_6()
                    .children((page == Page::Sonner).then(|| group("Copy, toast, gone", cx).child(row().child(copy)).child(div().min_h_16().children(toast))))
                    .children((page == Page::Alert).then(|| group("Dismiss and restore", cx).child(div().min_h_16().child(notice))))
            ),
        )
    }

    fn dialog_flow(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let close = |deleted: bool| cx.listener(move |this, _, _, cx| {
            this.flow.dialog = false;
            this.flow.deleted = deleted;
            cx.notify();
        });
        let backdrop = div().id("fl-backdrop").absolute().inset_0().on_click(close(false));
        let dialog = ui::Dialog::new("fl-dialog", DIALOG_WIDTH, ui::DialogContent::new()
            .child(div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Delete #general?"))
            .child(div().text_color(palette::muted(cx)).child("This removes the channel and its 1,204 messages for everyone. You can't undo it."))
            .child(row().justify_end().child(ui::Button::new("fl-cancel").variant(ui::ButtonVariant::Secondary).child("Keep channel").on_click(close(false))).child(ui::Button::new("fl-delete").variant(ui::ButtonVariant::Destructive).child("Delete channel").on_click(close(true)))), window, cx);
        let stage = ui::col()
            .relative()
            .h(px(STAGE_HEIGHT))
            .overflow_hidden()
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(Level::Page, cx))
            .border_1()
            .border_color(palette::line(cx))
            .p_4()
            .gap_2()
            .child(ui::section_label("Channels", ui::Heading::Muted, cx))
            .child(ui::DropdownMenuItem::new("fl-ch-1", cx).child(glyph(Glyph::Hashtag)).child("general").child(div().flex_1()).child(ui::Button::new("fl-ch-trash").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::Trash).tooltip_text("Delete channel").on_click(cx.listener(|this, _, _, cx| {
                this.flow.dialog = true;
                this.flow.deleted = false;
                cx.notify();
            }))))
            .child(ui::DropdownMenuItem::new("fl-ch-2", cx).child(glyph(Glyph::Hashtag)).child("looking-for-group"))
            .when(self.flow.dialog, |d| d.child(ui::Dialog::modal(backdrop, dialog, cx)));
        let undo = self.flow.deleted.then(|| {
            let restore = ui::Button::new("fl-restore-channel").variant(ui::ButtonVariant::Link).size(ui::ButtonSize::Sm).child("Undo").on_click(cx.listener(|this, _, _, cx| {
                this.flow.deleted = false;
                cx.notify();
            }));
            ui::Sonner::toast_with("fl-deleted", Glyph::Checkmark, palette::positive_ink(cx), "#general deleted", None, Some(restore.into_any_element()), cx)
        });
        section("Confirm, then act", "A dialog is for what can't be undone: it names the thing and the cost, the safe button comes first and is named for what it keeps, and a click outside cancels. Open it with the bin.", cx)
            .child(board(cx).child(stage).child(div().min_h_16().children(undo)))
    }
}
