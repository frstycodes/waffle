// Selection: Switch, Checkbox, Radio, Segmented control, Slider, Tabs.
use super::*;

fn variant(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

impl DesignSystem {
    pub(super) fn switch_frames(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let bare = row().gap_4().child(ui::Switch::new("sw-off").checked(false)).child(ui::Switch::new("sw-on").checked(true)).child(note("Off · On", cx));
        let rows = ui::col()
            .w_80()
            .gap_2()
            .child(ui::toggle_row("c-noise", "Noise suppression", Some("Removes background sound from your mic".into()), self.noise, window, cx).on_click(cx.listener(|this, _, _, cx| {
                this.noise = !this.noise;
                cx.notify();
            })))
            .child(ui::toggle_row("c-echo", "Echo cancellation", Some("Stops others hearing themselves".into()), self.echo, window, cx).on_click(cx.listener(|this, _, _, cx| {
                this.echo = !this.echo;
                cx.notify();
            })));
        vec![variant("Switch / Layout", "Switches apply at once, with no Save. Bare, or as a row with a title and a line of detail; the whole row is the target.", ui::col().gap_6().child(group("Bare", cx).child(bare)).child(group("Toggle row", cx).child(rows)), cx)]
    }

    pub(super) fn checkbox_frames(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let k0 = ui::Checkbox::new("k-0").checked(if self.checks[0] { Check::On } else { Check::Off });
        let k1 = ui::Checkbox::new("k-1").checked(if self.checks[1] { Check::On } else { Check::Off });
        let k2 = ui::Checkbox::new("k-2").checked(Check::Mixed);
        let rows = ui::col()
            .w_80()
            .gap_2()
            .child(ui::pick_row("c-k0", k0, "Show game activity", Some("Lets friends see what you play".into()), cx).on_click(cx.listener(|this, _, _, cx| {
                this.checks[0] = !this.checks[0];
                cx.notify();
            })))
            .child(ui::pick_row("c-k1", k1, "Allow DMs from server members", None, cx).on_click(cx.listener(|this, _, _, cx| {
                this.checks[1] = !this.checks[1];
                cx.notify();
            })))
            .child(ui::pick_row("c-k2", k2, "Notify for every channel", Some("Some channels on, some off".into()), cx));
        vec![variant("Checkbox / State", "Checkboxes pick many. Mixed is derived, never clicked into.", ui::col().child(rows), cx)]
    }

    pub(super) fn radio_frames(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let r0 = ui::RadioGroupItem::new("r-0").checked(self.voice_mode == 0);
        let r1 = ui::RadioGroupItem::new("r-1").checked(self.voice_mode == 1);
        let rows = ui::col()
            .w_80()
            .gap_2()
            .child(ui::pick_row("c-r0", r0, "Voice activity", Some("Transmits when you speak".into()), cx).on_click(cx.listener(|this, _, _, cx| {
                this.voice_mode = 0;
                cx.notify();
            })))
            .child(ui::pick_row("c-r1", r1, "Push to talk", Some("Hold a key to transmit".into()), cx).on_click(cx.listener(|this, _, _, cx| {
                this.voice_mode = 1;
                cx.notify();
            })));
        vec![variant("Radio / State", "Pick one of a set; the band thickens on the spring when chosen.", ui::col().child(rows), cx)]
    }

    pub(super) fn segmented_frames(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let this = cx.entity().downgrade();
        let seg = ui::Tabs::new("quality").labels(&["720p", "1080p", "Source"]).value(self.quality).on_value_change(move |i, _, cx| {
            this.update(cx, |this, cx| {
                this.quality = i;
                cx.notify();
            })
            .ok();
        });
        vec![variant("Segmented control / Options", "Switches a view or a value in place; the track hugs its options.", ui::col().child(seg), cx)]
    }

    pub(super) fn slider_frames(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let this = cx.entity().downgrade();
        let volume = ui::Slider::new("volume").value(self.volume).on_value_change(move |v, _, cx| {
            this.update(cx, |this, cx| {
                this.volume = v;
                cx.notify();
            })
            .ok();
        });
        let field = ui::Field::new("Mira's volume", cx).w_80().child(ui::row().gap_2().child(div().flex_1().min_w_0().child(volume)).child(div().min_w_11().text_right().text_color(palette::strong(cx)).child(format!("{}%", (self.volume * 200.).round()))));
        vec![variant("Slider / State", "Press anywhere on the track to jump there, drag to follow, arrows step 1%.", ui::col().child(field), cx)]
    }

    pub(super) fn tabs_frames(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let this = cx.entity().downgrade();
        let items = |names: &[(&'static str, Option<u32>)]| names.iter().map(|(n, c)| ui::Tab::new(*n).count(*c)).collect::<Vec<_>>();
        let tabs = ui::Tabs::new("tabs").variant(ui::TabsVariant::Line).tabs(&items(&[("Overview", None), ("Roles", None), ("Emoji", None), ("Members", Some(4)), ("Audit log", None)])).value(self.tab).on_value_change({
            let this = this.clone();
            move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.tab = i;
                    cx.notify();
                })
                .ok();
            }
        });
        let pills = ui::ToggleGroup::new("pills").items(&items(&[("Online", None), ("All", None), ("Pending", Some(2)), ("Blocked", None)])).value(self.pill).on_value_change(move |i, _, cx| {
            this.update(cx, |this, cx| {
                this.pill = i;
                cx.notify();
            })
            .ok();
        });
        vec![variant("Tabs / Style", "Underline tabs split a page into views; pill tabs filter a list inside one view.", ui::col().gap_6().child(group("Underline", cx).child(tabs)).child(group("Pills", cx).child(pills)), cx)]
    }
}
