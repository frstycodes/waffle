// Actions: Button, Button group, Toggle, Card.
use super::*;

fn variant(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

fn state(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::States, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

impl DesignSystem {
    pub(super) fn button_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let start = |i: usize| {
            cx.listener(move |this: &mut Self, _, _, cx| {
                if this.busy[i] {
                    return;
                }
                this.busy[i] = true;
                cx.notify();
                this.after(1400, cx, move |this| this.busy[i] = false);
            })
        };
        let variants = group("Variant", cx).child(
            row()
                .child(ui::Button::new("v-default").child("Default"))
                .child(ui::Button::new("v-secondary").variant(ui::ButtonVariant::Secondary).child("Secondary"))
                .child(ui::Button::new("v-destructive").variant(ui::ButtonVariant::Destructive).child("Destructive"))
                .child(ui::Button::new("v-outline").variant(ui::ButtonVariant::Outline).child("Outline"))
                .child(ui::Button::new("v-ghost").variant(ui::ButtonVariant::Ghost).child("Ghost"))
                .child(ui::Button::new("v-link").variant(ui::ButtonVariant::Link).child("Link")),
        );
        let button_sizes = group("Size", cx).child(
            row()
                .child(ui::Button::new("s-sm").size(ui::ButtonSize::Sm).child("Small"))
                .child(ui::Button::new("s-default").child("Default"))
                .child(ui::Button::new("s-lg").size(ui::ButtonSize::Lg).child("Large"))
                .child(ui::Button::new("s-icon").size(ui::ButtonSize::Icon).variant(ui::ButtonVariant::Outline).child(glyph(Glyph::Bell)))
                .child(ui::Button::new("s-off").disabled(true).child("Disabled")),
        );
        let over_art = group("Scope: Art, Media", cx).child(
            row().p_4().squircle(ui::RADIUS_LG).bg(palette::stage::dock()).child(ui::Button::new("b-overlay").scope(ui::ButtonScope::Art).child("Join Activity")).child(ui::Button::new("b-on-media").scope(ui::ButtonScope::Media).child("Stop watching")),
        );
        let sizes = group("Size", cx).child(
            row()
                .child(ui::Button::new("b-lg").size(ui::ButtonSize::Lg).child(glyph(Glyph::Stream)).child("Go live"))
                .child(ui::Button::new("b-md").child(glyph(Glyph::Stream)).child("Go live"))
                .child(ui::Button::new("b-sm").size(ui::ButtonSize::Sm).child(glyph(Glyph::Stream)).child("Go live"))
                .child(note("Large 44 · Medium 36 · Small 30 · radius 10, small 8", cx)),
        );
        let states = group("State: Disabled, Loading, Toggled", cx).child(
            row()
                .child(ui::Button::new("b-off-1").disabled(true).child("Join voice"))
                .child(ui::Button::new("b-off-2").variant(ui::ButtonVariant::Secondary).disabled(true).child("Invite friends"))
                .child(ui::Button::new("b-busy-1").loading(self.busy[0].then_some("Saving…")).child("Save changes").on_click(start(0)))
                .child(ui::Button::new("b-busy-2").variant(ui::ButtonVariant::Secondary).loading(self.busy[1].then_some("Sending…")).child(glyph(Glyph::PaperPlaneBold)).child("Send invite").on_click(start(1)))
                .child(ui::Button::new("b-muted").variant(ui::ButtonVariant::Secondary).toggled(self.muted).child(glyph(if self.muted { Glyph::BellSlash } else { Glyph::Bell })).child(if self.muted { "Muted" } else { "Mute" }).on_click(cx.listener(|this, _, _, cx| {
                    this.muted = !this.muted;
                    cx.notify();
                })))
                .child(note("Loading keeps the width and shows the wave", cx)),
        );
        let matrix = {
            let variants = [("Default", ui::ButtonVariant::Default), ("Secondary", ui::ButtonVariant::Secondary), ("Destructive", ui::ButtonVariant::Destructive), ("Outline", ui::ButtonVariant::Outline), ("Ghost", ui::ButtonVariant::Ghost), ("Link", ui::ButtonVariant::Link)];
            let head = row().gap_4().child(div().w_24().flex_none()).children(["Default", "Hover", "Disabled", "Loading"].map(|s| div().w_32().flex_none().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(s)));
            ui::col().gap_2().child(head).children(variants.map(|(name, variant)| {
                let id = |state: &'static str| gpui_kit::SharedString::from(format!("m-{name}-{state}"));
                row()
                    .gap_4()
                    .child(div().w_24().flex_none().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(name))
                    .child(div().w_32().flex_none().child(ui::Button::new(id("rest")).variant(variant).child("Save draft")))
                    .child(div().w_32().flex_none().child(ui::Button::new(id("hover")).variant(variant).highlighted(true).child("Save draft")))
                    .child(div().w_32().flex_none().child(ui::Button::new(id("off")).variant(variant).disabled(true).child("Save draft")))
                    .child(div().w_32().flex_none().child(ui::Button::new(id("busy")).variant(variant).loading(Some("Saving…")).child("Save draft")))
            }))
        };
        vec![
            state("Button / Variant × state", "Every variant at rest, hovered, disabled and loading, pinned side by side; pressed and focus are live on the buttons above.", matrix, cx),
            variant("Button / Variant", "shadcn's Button: variant and size, defined once in ui/button.rs.", ui::col().gap_6().child(variants).child(button_sizes), cx),
            variant("Button / Scope", "Verb plus object, sentence case. One primary per surface; secondary is the quiet fill, ghost for low-stakes actions in dense rows, danger only for the destructive step itself.", ui::col().gap_6().child(over_art), cx),
            variant("Button / Size", "Three heights; the icon and label keep their gap at each.", ui::col().child(sizes), cx),
            state("Button / State", "Hover, press and focus are live: try them. Loading and toggled are clickable.", ui::col().child(states), cx),
        ]
    }

    pub(super) fn icon_button_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let sizes = group("Size", cx).child(
            row()
                .child(ui::Button::new("i-close").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconXs).icon(Glyph::X).tooltip_text("Close"))
                .child(ui::Button::new("i-pin").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Pin).tooltip_text("Pin"))
                .child(ui::Button::new("i-settings").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconLg).icon(Glyph::Settings).tooltip_text("Settings"))
                .child(note("Small 24 · Medium 32 · Large 40", cx)),
        );
        let states = group("State: Toggled, Off, Succeeded, Disabled", cx).child(
            row()
                .child(ui::Button::new("i-members").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Users).tooltip_text("Member list").toggled(self.members).on_click(cx.listener(|this, _, _, cx| {
                    this.members = !this.members;
                    cx.notify();
                })))
                .child(ui::Button::new("i-unmute").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::MicOff).tooltip_text("Unmute").off(true))
                .child(ui::Button::new("i-copied").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Checkmark).tooltip_text("Copied").succeeded(true))
                .child(ui::Button::new("i-disabled").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Trash).tooltip_text("Delete").disabled(true))
                .child(note("Pressed is the selected fill · off-states are coral", cx)),
        );
        let shapes = group("Shape: Round, on the stage and the rail", cx).child(
            row()
                .child(div().p_2().bg(palette::stage::bg()).child(ui::Button::new("i-stage").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).scope(ui::ButtonScope::Stage).icon(Glyph::Settings).tooltip_text("Settings").round()))
                .child(ui::Button::new("i-rail").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Square(44.)).icon(Glyph::Plus).tooltip_text("Add a server")),
        );
        vec![variant("Button / Icon size", "Every icon button has a tooltip: hover one.", ui::col().gap_6().child(sizes).child(shapes), cx), state("Button / Icon state", "", ui::col().child(states), cx)]
    }

    pub(super) fn button_group_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let split = |id: &'static str, more: &'static str, variant: ui::ButtonVariant, glyph_: Glyph, label: &'static str, _cx: &App| {
            ui::ButtonGroup::new().item(ui::Button::new(id).variant(variant).child(glyph(glyph_)).child(label)).item(ui::Button::new(more).variant(variant).px_2().child(Icon::new(Glyph::ChevronDown).size(ui::ICON_SM)))
        };
        vec![variant(
            "Button group / Variant",
            "The main action and its options, joined: inner corners tighten to 4px with a 1px seam.",
            row().child(split("b-mic", "b-mic-more", ui::ButtonVariant::Secondary, Glyph::Mic, "Microphone", cx)).child(split("b-live", "b-live-more", ui::ButtonVariant::Default, Glyph::Stream, "Go live", cx)),
            cx,
        )]
    }

    pub(super) fn toggle_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        let toggles = row().children([("Images", Glyph::Image), ("Links", Glyph::Link), ("Files", Glyph::File)].into_iter().enumerate().map(|(i, (name, icon))| {
            ui::Toggle::new(("chip", i)).pressed(self.chips[i]).icon(icon).child(name).on_click(cx.listener(move |this, _, _, cx| {
                this.chips[i] = !this.chips[i];
                cx.notify();
            }))
        }));
        let removable = row().child(ui::Toggle::new("ds-chip-x").pressed(false).child("Mira").removable(|_, _, _| {})).child(ui::Toggle::new("ds-chip-plain").pressed(true).child("No icon"));
        vec![
            variant("Toggle / State", "A filter or toggle: pressed takes the accent tint and a half-accent edge. Click to toggle.", ui::col().gap_4().child(toggles), cx),
            variant("Toggle / Removable", "The × takes the chip away, in the chip's own ink.", ui::col().child(removable), cx),
        ]
    }

    pub(super) fn card_frames(&self, cx: &mut Context<Self>) -> Vec<Frame> {
        vec![variant(
            "Card / State",
            "A quiet fill on the page that deepens on hover; chosen takes the toggled fill.",
            row().gap_4().child(ui::Card::interactive("ds-card", cx).w_48().child("Start from a template")).child(ui::Card::interactive("ds-card-on", cx).bordered(cx).toggled(true, cx).w_48().child("Chosen")),
            cx,
        )]
    }
}
