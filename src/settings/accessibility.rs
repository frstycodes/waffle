// Settings › Accessibility: Discord's five sections (docs/discord-settings-re.md §2). Choice groups (Role colors,
// Play sticker animations) come from the registry; the rest is drawn here, under a preview that follows them.
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, HighlightStyle, Hsla, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, StyledText, UnderlineStyle, Window, component::Icon, div,
    linear_color_stop, linear_gradient, prelude::FluentBuilder, px,
};
use serde_json::json;

use super::{
    CHAT_FONT_SIZES, Density, Fill, GROUP_SPACINGS, Page, RoleStyle, Section, numbered,
    registry::{self, Setting, Toggle},
    stack_row,
};
use crate::{
    Waffle,
    app_keys::ZOOM_STEPS,
    icons::Glyph,
    palette,
    ui::{self, Squircle},
};

const TTS_SAMPLE: &str = "This is what text-to-speech sounds like at the current speed.";
const TTS_STOPS: usize = 41;
const SATURATION_STOPS: usize = 21;
const TOAST_SECS: std::ops::RangeInclusive<u8> = 3..=20;
const ZOOM_CAPTION: &str = if cfg!(target_os = "macos") {
    "Adjust the size of the interface. You can also change this with \u{2318} + / -."
} else {
    "Adjust the size of the interface. You can also change this with Ctrl + / -."
};

impl Waffle {
    pub(super) fn accessibility_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        vec![
            Section::new(self.a11y_preview(cx)),
            Section::keyed("Text readability", self.readability_group(window, cx)),
            Section::keyed("Visual density", self.density_group(window, cx)),
            Section::keyed("Colour and contrast", self.contrast_group(window, cx)),
            Section::keyed("Reduced motion", self.motion_group(window, cx)),
            Section::keyed("Audio and screen reader", self.speech_group(window, cx)),
        ]
    }

    // The link under Appearance's Theme group.
    pub(super) fn accessibility_link(&self, cx: &mut Context<Self>) -> Div {
        let link = ui::button("theme-accessibility", ui::Tone::Link, cx)
            .text_size(px(12.))
            .child("Adjust the visual density of chat messages and UI")
            .on_click(cx.listener(|this, _, window, cx| this.open_page(Page::Accessibility, window, cx)));
        div().px(px(4.)).flex().child(link)
    }

    fn readability_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let size = self.prefs.chat_font_size;
        let at = nearest(CHAT_FONT_SIZES.map(f32::from), f32::from(size));
        let slider = self.stepped("chat-font-size", CHAT_FONT_SIZES.len(), at, |this, i, cx| this.update_prefs(cx, |p| p.chat_font_size = CHAT_FONT_SIZES[i]), cx);
        let card = ui::settings_card(cx)
            .child(slider_row(setting("chat-font-size"), None, format!("{size}px"), slider, value_marks(&CHAT_FONT_SIZES, "px", at, cx), false, cx))
            .child(self.registry_row(setting("underline-links"), true, window, cx))
            .child(self.registry_row(setting("display-name-styles"), true, window, cx));
        ui::settings_group("Text readability", cx).child(card)
    }

    fn density_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let compact = self.account_settings.message_display_compact;
        let density = self.prefs.ui_density as usize;
        let this = cx.entity().downgrade();
        let density_control = ui::segmented_live(
            "ui-density-segments",
            &["Compact", "Default", "Spacious"],
            density,
            move |i, _, cx| {
                let density = [Density::Compact, Density::Default, Density::Spacious][i.min(2)];
                this.update(cx, |this, cx| this.update_prefs(cx, |p| p.ui_density = density)).ok();
            },
            window,
            cx,
        );
        let this = cx.entity().downgrade();
        let display_control = ui::segmented_live(
            "message-display-segments",
            &["Default", "Compact"],
            compact as usize,
            move |i, _, cx| {
                this.update(cx, |this, cx| this.set_message_display(i == 1, cx)).ok();
            },
            window,
            cx,
        );
        let spacing = self.prefs.group_spacing(compact);
        let spacing_at = nearest(GROUP_SPACINGS.map(f32::from), f32::from(spacing));
        let spacing_slider = self.stepped("group-spacing", GROUP_SPACINGS.len(), spacing_at, |this, i, cx| this.update_prefs(cx, |p| p.group_spacing = Some(GROUP_SPACINGS[i])), cx);
        let zoom = self.prefs.zoom;
        let zoom_at = nearest(ZOOM_STEPS, zoom);
        let zoom_slider = self.stepped("zoom", ZOOM_STEPS.len(), zoom_at, |_, i, cx| cx.defer(move |cx| crate::app_keys::set_zoom_to(cx, ZOOM_STEPS[i])), cx);
        let zoom_percents = ZOOM_STEPS.map(|z| (z * 100.).round() as u32);
        let card = ui::settings_card(cx)
            .child(segmented_row(setting("ui-density"), density_control, false, cx))
            .child(segmented_row(setting("message-display"), display_control, true, cx))
            .child(slider_row(setting("group-spacing"), None, format!("{spacing}px"), spacing_slider, value_marks(&GROUP_SPACINGS, "px", spacing_at, cx), true, cx))
            .child(slider_row(setting("zoom"), Some(ZOOM_CAPTION), format!("{}%", zoom_percents[zoom_at]), zoom_slider, value_marks(&zoom_percents, "%", zoom_at, cx), true, cx));
        ui::settings_group("Visual density", cx)
            .child(card)
            .child(ui::settings_caption("Space between message groups follows the display: 16px in Default, 0 in Compact, until you move it.", cx))
    }

    // Discord's message_display_compact, synced like the account's other switches.
    fn set_message_display(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.account_settings.message_display_compact == compact {
            return;
        }
        self.account_settings.message_display_compact = compact;
        self.save_account_settings(json!({ "message_display_compact": compact }), cx);
    }

    fn contrast_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let at = (self.prefs.saturation.clamp(0., 1.) * (SATURATION_STOPS - 1) as f32).round() as usize;
        let slider = self.stepped("saturation", SATURATION_STOPS, at, |this, i, cx| this.update_prefs(cx, |p| p.saturation = i as f32 / (SATURATION_STOPS - 1) as f32), cx);
        let caption = "Reduce the saturation of colors within the app, for those with color sensitivities. This does not affect the saturation of images, videos, role colors or other user content.";
        let marks = marks(["0%", "25%", "50%", "75%", "100%"].map(|m| (SharedString::from(m), false)), cx);
        let card = ui::settings_card(cx)
            .child(slider_row(setting("saturation"), Some(caption), format!("{}%", at * 5), slider, marks, false, cx))
            .child(self.registry_row(setting("desaturate-user-colors"), true, window, cx))
            .child(self.registry_row(setting("high-contrast"), true, window, cx))
            .child(self.registry_row(setting("switch-icons"), true, window, cx));
        ui::settings_group("Colour and contrast", cx).child(card)
    }

    fn motion_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let secs = self.prefs.min_toast_secs.clamp(*TOAST_SECS.start(), *TOAST_SECS.end());
        let stops = usize::from(TOAST_SECS.end() - TOAST_SECS.start()) + 1;
        let slider = self.stepped("min-toast", stops, usize::from(secs - TOAST_SECS.start()), |this, i, cx| this.update_prefs(cx, |p| p.min_toast_secs = TOAST_SECS.start() + i as u8), cx);
        let caption = "Keep pop-up notifications on screen for at least this long. Notifications that already stay longer are unaffected.";
        let marks = marks(["3s", "20s"].map(|m| (SharedString::from(m), false)), cx);
        let card = ui::settings_card(cx)
            .child(self.toggle_row(setting("reduce-motion"), Toggle::ReduceMotion, false, window, cx))
            .child(self.toggle_row(setting("reduce-motion-sync"), Toggle::ReduceMotionSync, true, window, cx))
            .child(slider_row(setting("min-toast"), Some(caption), format!("{secs}s"), slider, marks, true, cx))
            .child(self.toggle_row(setting("gif-autoplay"), Toggle::GifAutoPlay, true, window, cx))
            .child(self.toggle_row(setting("animate-emoji"), Toggle::AnimateEmoji, true, window, cx));
        ui::settings_group("Reduced motion", cx).child(card)
    }

    fn speech_group(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let rate = self.prefs.tts_rate;
        let at = ((rate.clamp(0., 10.) * 4.).round() as usize).min(TTS_STOPS - 1);
        let slider = self.stepped("tts-rate", TTS_STOPS, at, |this, i, cx| this.update_prefs(cx, |p| p.tts_rate = i as f32 / 4.), cx);
        let tts = setting("tts-rate");
        let caption = "Control the speed at which text is read aloud when using Speak Message or other text-to-speech features.";
        // `say` is macOS's; elsewhere there's no voice to preview, as Speak message is macOS only.
        let preview = cfg!(target_os = "macos").then(|| {
            ui::button("tts-preview", ui::Tone::Secondary, cx)
                .small()
                .child(Icon::new(Glyph::Play).size(px(14.)))
                .child("Preview")
                .on_click(cx.listener(|this, _, _, _| crate::message_menu::speak(TTS_SAMPLE, this.prefs.tts_rate)))
        });
        let value = div().whitespace_nowrap().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(format!("x{:.2}", at as f32 / 4.));
        let header = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .child(ui::settings_text(tts.title, Some(caption), cx))
            .child(div().flex().items_center().gap(px(12.)).flex_none().child(value).children(preview));
        let marks = div()
            .flex()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(palette::muted(cx))
            .child("Slower")
            .child("x1.0")
            .child(div().ml_auto().child("Faster"));
        let rate_row = stack_row(tts.id, false, cx).child(header).child(slider).child(marks);
        let card = ui::settings_card(cx).child(rate_row).child(self.toggle_row(setting("image-descriptions"), Toggle::ImageDescriptions, true, window, cx));
        ui::settings_group("Audio and screen reader", cx).child(card)
    }

    // A slider that snaps to `stops` evenly spaced stops; `on_stop` runs once the stop under the pointer
    // differs from `at`, so a drag saves once per stop it crosses.
    fn stepped(&self, key: &'static str, stops: usize, at: usize, on_stop: impl Fn(&mut Waffle, usize, &mut Context<Waffle>) + Clone + 'static, cx: &mut Context<Self>) -> AnyElement {
        let last = (stops - 1) as f32;
        self.range(
            key.into(),
            at as f32 / last,
            Fill::Accent,
            move |this, f, cx| {
                let stop = (f.clamp(0., 1.) * last).round() as usize;
                if stop != at {
                    on_stop(this, stop, cx);
                }
            },
            cx,
        )
    }

    // Discord's preview over the page: three sample messages drawn the way the settings below would
    // draw them. GAP: Discord pins it while the page scrolls; gpui's list has no sticky items.
    // GAP: drawn here, not by chat.rs's message_row, which needs a live channel's RowContext.
    fn a11y_preview(&self, cx: &mut Context<Self>) -> Div {
        let look = PreviewLook::new(self);
        let card = div()
            .flex()
            .flex_col()
            .py(px(12.))
            .px(px(8.))
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(palette::Level::Group, cx))
            .text_size(px(look.font))
            .line_height(px(look.font * 1.375))
            .children(SAMPLES.iter().enumerate().map(|(i, sample)| look.message(sample, i, cx)));
        let heading = div()
            .px(px(4.))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::muted(cx))
            .child(Icon::new(Glyph::Eye).size(px(14.)))
            .child("Preview");
        div().flex().flex_col().gap(px(8.)).child(heading).child(card)
    }
}

fn setting(id: &str) -> &'static Setting {
    registry::SETTINGS.iter().find(|s| s.id == id).expect("accessibility settings are in the registry")
}

// The index of the stop closest to `value`.
fn nearest<const N: usize>(stops: [f32; N], value: f32) -> usize {
    (0..N).min_by(|&a, &b| (stops[a] - value).abs().total_cmp(&(stops[b] - value).abs())).unwrap_or(0)
}

// A slider row: title, caption and value over the track, labelled stops under it.
fn slider_row(setting: &'static Setting, caption: Option<&'static str>, value: String, slider: AnyElement, marks: Div, line: bool, cx: &mut Context<Waffle>) -> Stateful<Div> {
    let caption = caption.or(setting.caption).unwrap_or_default();
    stack_row(setting.id, line, cx).child(numbered(setting.title, caption, &value, cx)).child(slider).child(marks)
}

fn segmented_row(setting: &'static Setting, control: Div, line: bool, cx: &mut Context<Waffle>) -> Stateful<Div> {
    ui::settings_row(setting.id, line, cx).child(ui::settings_text(setting.title, setting.caption, cx)).child(control.flex_none())
}

// Every stop labelled with its value; the chosen one in the accent.
fn value_marks<T: std::fmt::Display>(values: &[T], unit: &str, at: usize, cx: &mut Context<Waffle>) -> Div {
    marks(values.iter().enumerate().map(|(i, v)| (SharedString::from(format!("{v}{unit}")), i == at)), cx)
}

fn marks(labels: impl IntoIterator<Item = (SharedString, bool)>, cx: &mut Context<Waffle>) -> Div {
    let (muted, on) = (palette::muted(cx), palette::accent_ink(cx));
    div().flex().justify_between().text_size(px(12.)).text_color(muted).children(
        labels.into_iter().map(|(label, chosen)| div().when(chosen, |d| d.text_color(on).font_weight(FontWeight::SEMIBOLD)).child(label)),
    )
}

struct Sample {
    author: &'static str,
    // The author's role colour, as 0xRRGGBB.
    color: u32,
    time: &'static str,
    // A message without an author starts no group: it continues the one above.
    starts_group: bool,
    text: &'static str,
    link: Option<&'static str>,
    spoiler: Option<&'static str>,
    media: SampleMedia,
}

enum SampleMedia {
    None,
    Image { description: &'static str },
    Sticker,
}

const SAMPLES: [Sample; 3] = [
    Sample { author: "Juno", color: 0xE67E22, time: "4:12 PM", starts_group: true, text: "Ramen at mine tonight? Recipe's here, and ", link: Some("here"), spoiler: Some("the secret is miso butter"), media: SampleMedia::None },
    Sample { author: "Juno", color: 0xE67E22, time: "4:12 PM", starts_group: false, text: "Bring the good chopsticks", link: None, spoiler: None, media: SampleMedia::Image { description: "A bowl of ramen with a soft egg" } },
    Sample { author: "Theo", color: 0x3498DB, time: "4:14 PM", starts_group: true, text: "On my way", link: None, spoiler: None, media: SampleMedia::Sticker },
];

// What the preview reads from the settings, worked out once per draw.
struct PreviewLook {
    font: f32,
    gap: f32,
    compact: bool,
    compact_avatars: bool,
    role_style: RoleStyle,
    saturation: f32,
    desaturate_user_colors: bool,
    underline_links: bool,
    descriptions: bool,
}

impl PreviewLook {
    fn new(waffle: &Waffle) -> Self {
        let prefs = &waffle.prefs;
        let compact = waffle.account_settings.message_display_compact;
        Self {
            font: f32::from(prefs.chat_font_size),
            gap: f32::from(prefs.group_spacing(compact)),
            compact,
            compact_avatars: prefs.compact_avatars,
            role_style: prefs.role_style,
            saturation: prefs.saturation.clamp(0., 1.),
            desaturate_user_colors: prefs.desaturate_user_colors,
            underline_links: prefs.underline_links,
            descriptions: prefs.view_image_descriptions,
        }
    }

    fn message(&self, sample: &Sample, index: usize, cx: &Context<Waffle>) -> Div {
        let row = div().px(px(8.)).py(px(if self.compact { 1. } else { 2. })).when(sample.starts_group && index > 0, |d| d.mt(px(self.gap)));
        let media = self.media(&sample.media, cx);
        if self.compact {
            let time = div().w(px(52.)).flex_none().text_size(px(12.)).text_color(palette::faint(cx)).child(sample.time);
            let face = (sample.starts_group && self.compact_avatars).then(|| ui::monogram(sample.author, 18.));
            let line = div().flex().items_baseline().gap(px(8.)).child(time).children(face).when(sample.starts_group, |d| d.child(self.name(sample, cx))).child(self.text(sample, cx));
            return row.child(line).children(media.map(|m| m.ml(px(60.))));
        }
        let face = if sample.starts_group { ui::monogram(sample.author, 40.) } else { div().w(px(40.)).flex_none() };
        let who = sample.starts_group.then(|| {
            let time = div().text_size(px(12.)).text_color(palette::faint(cx)).child(format!("Today at {}", sample.time));
            div().flex().items_baseline().gap(px(8.)).child(self.name(sample, cx)).child(time)
        });
        let body = div().flex_1().min_w_0().flex().flex_col().children(who).child(self.text(sample, cx)).children(media);
        row.flex().gap(px(14.)).child(face).child(body)
    }

    fn name(&self, sample: &Sample, cx: &Context<Waffle>) -> Div {
        let role = gpui_kit::rgb(sample.color).into();
        let role = if self.desaturate_user_colors { self.saturate(role) } else { role };
        let ink = if self.role_style == RoleStyle::InNames { role } else { palette::strong(cx) };
        let dot = (self.role_style == RoleStyle::NextToNames).then(|| div().size(px(10.)).flex_none().rounded_full().bg(role));
        div().flex().items_center().gap(px(6.)).font_weight(FontWeight::SEMIBOLD).text_color(ink).children(dot).child(sample.author)
    }

    fn text(&self, sample: &Sample, cx: &Context<Waffle>) -> Div {
        let mut full = sample.text.to_owned();
        let mut runs = Vec::new();
        if let Some(link) = sample.link
            && let Some(start) = full.find(link)
        {
            let ink = self.saturate(palette::accent_ink(cx));
            let underline = self.underline_links.then_some(UnderlineStyle { thickness: px(1.), color: Some(ink), wavy: false });
            runs.push((start..start + link.len(), HighlightStyle { color: Some(ink), underline, ..Default::default() }));
        }
        if let Some(spoiler) = sample.spoiler {
            let start = full.len();
            full.push_str(spoiler);
            let hidden = palette::strong(cx);
            runs.push((start..full.len(), HighlightStyle { color: Some(hidden), background_color: Some(hidden), ..Default::default() }));
        }
        div().min_w_0().text_color(palette::strong(cx)).child(StyledText::new(full).with_highlights(runs))
    }

    // Images and stickers are user content: saturation leaves them alone.
    fn media(&self, media: &SampleMedia, cx: &Context<Waffle>) -> Option<Div> {
        let frame = div().mt(px(6.)).flex();
        match media {
            SampleMedia::None => None,
            SampleMedia::Image { description } => {
                let fill = linear_gradient(135., linear_color_stop(palette::oklch(0.7, 0.13, 60.), 0.), linear_color_stop(palette::oklch(0.55, 0.12, 200.), 1.));
                let alt = self.descriptions.then(|| {
                    div().px(px(6.)).py(px(1.)).rounded(px(6.)).bg(gpui_kit::black().opacity(0.6)).text_size(px(11.)).line_height(px(15.)).text_color(gpui_kit::white()).child(*description)
                });
                Some(frame.child(div().w(px(180.)).h(px(112.)).p(px(6.)).squircle(ui::RADIUS_MD).bg(fill).flex().items_end().children(alt)))
            }
            SampleMedia::Sticker => Some(frame.child(div().size(px(72.)).flex().items_center().justify_center().text_color(self.saturate(palette::accent_ink(cx))).child(Icon::new(Glyph::Sticker).size(px(56.))))),
        }
    }

    fn saturate(&self, color: Hsla) -> Hsla {
        Hsla { s: color.s * self.saturation, ..color }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_snaps_off_list_values_to_the_closest_stop() {
        let sizes = CHAT_FONT_SIZES.map(f32::from);
        assert_eq!(nearest(sizes, 16.), 3);
        assert_eq!(nearest(sizes, 13.), 0);
        assert_eq!(nearest(sizes, 22.5), 6);
        assert_eq!(nearest(ZOOM_STEPS, 1.), 5);
        assert_eq!(nearest(ZOOM_STEPS, 0.3), 0);
    }
}
