// Settings, ported from design/settings.html: the whole window, the nav on the sidebar tone, one page
// of titled groups on the right. Only settings Waffle honours are shown.
use std::{collections::HashMap, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Div, ElementId, FontWeight, Hsla, MouseButton, Pixels, Radians, ScrollHandle, SharedString, Stateful, Task, Window, canvas,
    component::Icon,
    div, img, prelude::*, px, relative,
};
use serde::{Deserialize, Serialize};

mod accessibility;
mod activity;
mod advanced;
mod mini;
use mini::{MINI_DARK, MINI_LIGHT, mini_sync, mini_window};
mod games;
pub(crate) mod profile;
mod profile_draft;
mod profile_preview;
mod profiles;
pub(crate) mod proto;
pub(crate) mod registry;
mod updates;
mod view;
mod meter;
mod page;
mod prefs;
mod voice_pages;
use meter::*;
use page::NAV;
pub use page::Page;
pub use prefs::*;

pub(crate) use profiles::ProfileTab;

use crate::{
    Waffle,
    hotkeys::{self, Action, Keybind},
    icons::Glyph,
    palette::{self, Level},
    sounds::{self, Cue},
    motion,
    ui::{self, Squircle, Tone},
    voice,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slider {
    InputVolume,
    OutputVolume,
    SoundboardVolume,
    Sensitivity,
    ReleaseDelay,
    Attenuation,
}

impl Slider {
    fn fraction(self, prefs: &Prefs) -> f32 {
        match self {
            Slider::InputVolume => prefs.input_volume / 2.,
            Slider::OutputVolume => prefs.output_volume / 2.,
            Slider::SoundboardVolume => crate::soundboard::slider_fraction(prefs.soundboard_volume),
            Slider::Sensitivity => (prefs.sensitivity_db + 100.) / 100.,
            Slider::ReleaseDelay => prefs.ptt_release_ms as f32 / 2000.,
            Slider::Attenuation => f32::from(prefs.attenuation) / 100.,
        }
    }

    fn set(self, prefs: &mut Prefs, fraction: f32) {
        let f = fraction.clamp(0., 1.);
        match self {
            Slider::InputVolume => prefs.input_volume = f * 2.,
            Slider::OutputVolume => prefs.output_volume = f * 2.,
            Slider::SoundboardVolume => prefs.soundboard_volume = crate::soundboard::amplitude(f * 100.),
            Slider::Sensitivity => prefs.sensitivity_db = (f * 100. - 100.).round(),
            Slider::ReleaseDelay => prefs.ptt_release_ms = (f * 2000.).round() as u32,
            Slider::Attenuation => prefs.attenuation = (f * 100.).round() as u8,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Select {
    Microphone,
    Speaker,
    Camera,
    Action(usize),
    UiFont,
    MonoFont,
    JoinActivity,
}

// The live drag payload for a slider; drawn as nothing.
#[derive(Clone)]
struct SliderDrag(SharedString);

impl Render for SliderDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

// A running mic test and the deafen state to give back when it ends.
pub(crate) struct MicTestRun {
    _test: voice::MicTest,
    _task: Task<()>,
    held: HeldDeafen,
}

// Deafen held on for the mic test, remembering what to restore. A toggle during the test replaces
// it: the user's latest choice wins over the state from before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HeldDeafen(bool);

impl HeldDeafen {
    fn hold(deafened: &mut bool) -> Self {
        let before = *deafened;
        *deafened = true;
        Self(before)
    }

    fn chosen(&mut self, deafened: bool) {
        self.0 = deafened;
    }

    fn release(self, deafened: &mut bool) {
        *deafened = self.0;
    }
}

pub struct SettingsView {
    page: Page,
    section: usize,
    select: Option<Select>,
    recording: Option<usize>,
    more_sounds: bool,
    advanced: bool,
    // Log out and Reset ask in place before acting.
    asking: Option<Ask>,
    // What Reset replaced, kept while its Undo shows.
    reset_undo: Option<Prefs>,
    copied: bool,
    scroll: ScrollHandle,
    devices: Devices,
    // From the running mic test: whether the voice-activity gate is open, and Discord's
    // "isn't detecting any input" state.
    mic_speaking: bool,
    mic_input: Option<bool>,
    // The highlighted search result, and the group a jump still has to scroll to once its page is drawn.
    result: usize,
    pending_jump: Option<&'static str>,
    editing: Option<profile::ProfileEdit>,
    // The Profiles page's drafts, kept while Settings stays open.
    profiles: Option<profiles::ProfileEditor>,
    // Add it!'s list of running programs, while open.
    adding_game: Option<Vec<(String, String)>>,
}

#[derive(Default)]
struct Devices {
    inputs: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
    cameras: Vec<(String, String)>,
}

impl Devices {
    fn list() -> Self {
        let pairs = |devices: Vec<voice::Device>| devices.into_iter().map(|d| (d.id, d.name)).collect();
        Self { inputs: pairs(voice::input_devices()), outputs: pairs(voice::output_devices()), cameras: pairs(voice::cameras()) }
    }
}

impl SettingsView {
    pub(crate) fn reset_result(&mut self) {
        self.result = 0;
    }

    // Demo scenes open the first keybind's action list.
    #[cfg(debug_assertions)]
    pub fn open_first_select(&mut self) {
        self.select = Some(Select::Action(0));
    }

    pub fn new(page: Page) -> Self {
        Self {
            page,
            section: 0,
            select: None,
            recording: None,
            more_sounds: false,
            advanced: false,
            asking: None,
            reset_undo: None,
            copied: false,
            scroll: ScrollHandle::new(),
            devices: Devices::list(),
            mic_speaking: false,
            mic_input: None,
            result: 0,
            pending_jump: None,
            editing: None,
            profiles: None,
            adding_game: None,
        }
    }
}

impl Waffle {
    pub(crate) fn open_settings(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = Some(SettingsView::new(page));
        self.settings_search.update(cx, |state, cx| state.set_value("", window, cx));
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = &mut self.settings {
            (s.page, s.section, s.select, s.recording, s.asking) = (page, 0, None, None, None);
            s.scroll.set_offset(Default::default());
        }
        self.settings_search.update(cx, |state, cx| state.set_value("", window, cx));
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = None;
        // The mic test plays the mic back through a speaker stream of its own, outside any call's
        // echo cancellation: left running, it feeds the far end their own voice whenever we talk.
        self.stop_mic_test();
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    // Saves, then pushes every setting that has a live effect: sounds, hotkeys, the voice engine, the look.
    pub(crate) fn update_prefs(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Prefs)) {
        change(&mut self.prefs);
        self.prefs.save();
        let restyled = palette::set_look(&self.prefs);
        self.apply_prefs();
        if restyled {
            palette::apply(cx);
        }
        cx.set_reduce_motion(self.prefs.reduces_motion());
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    pub(crate) fn apply_prefs(&mut self) {
        sounds::silence(&self.prefs.silenced_cues);
        crate::chat::set_clock_24(self.prefs.clock_24h);
        palette::set_look(&self.prefs);
        crate::ui::set_density(self.prefs.ui_density);
        crate::ui::set_switch_icons(self.prefs.switch_icons);
        if let Some(hotkeys) = &mut self.hotkeys {
            let active: Vec<Keybind> = self.prefs.keybinds.iter().filter(|k| self.prefs.push_to_talk || k.action != Action::PushToTalk).cloned().collect();
            self.keybind_errors = hotkeys.bind(&active);
        }
        if let Some(call) = &self.call {
            self.prefs.apply_to(&call.session);
        }
        crate::games::set_rules(crate::games::Rules { off: self.prefs.games_off.iter().cloned().collect(), added: self.prefs.games_added.clone() });
    }

    pub(crate) fn start_hotkeys(&mut self, cx: &mut Context<Self>) {
        let (hotkeys, mut events) = hotkeys::Hotkeys::start();
        self.hotkeys = Some(hotkeys);
        self.apply_prefs();
        cx.spawn(async move |this, cx| {
            while let Some(held) = futures::StreamExt::next(&mut events).await {
                if this.update(cx, |this, cx| this.on_keys_held(held, cx)).is_err() {
                    break;
                }
            }
        })
        // lint-allow(detached-task): a one-shot task that ends when its request answers, and it updates through a WeakEntity so a closed view drops the result
        .detach();
    }


    // Settings take the whole window: the nav on the sidebar tone, one page on the right with a big
    // title, and a close button pinned top right (design/settings.html, .set).
    pub(crate) fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let page = self.settings.as_ref()?.page;
        if page == Page::Profiles {
            self.ensure_profile_editor(window, cx);
        }
        let scroll = self.settings.as_ref()?.scroll.clone();
        let mut sections: Vec<Section> = match page {
            Page::Account => self.account_page(cx),
            Page::Profiles => self.profiles_page(window, cx),
            Page::Notifications => self.notifications_page(window, cx),
            Page::VoiceVideo => self.voice_page(window, cx),
            Page::Keybinds => self.keybinds_page(cx),
            Page::Appearance => self.appearance_page(window, cx),
            Page::Updates => self.updates_page(window, cx),
            Page::Advanced => self.advanced_page(cx),
            Page::Accessibility => self.accessibility_page(window, cx),
            Page::ActivityPrivacy => [self.activity_sharing_page(window, cx), self.games_page(window, cx)].into_iter().flatten().collect(),
            Page::ContentSocial | Page::Chat => Vec::new(),
        };
        sections.extend(self.registry_sections(page, window, cx));
        // Groups follow the registry's order; an unkeyed section stays with the one before it.
        let order = registry::groups(registry::SETTINGS, page);
        let mut rank = 0;
        let mut ranked: Vec<(usize, Section)> = sections
            .into_iter()
            .map(|section| {
                rank = section.key.and_then(|k| order.iter().position(|g| *g == k)).unwrap_or(rank);
                (rank, section)
            })
            .collect();
        // lint-allow(render-work): one page's groups, a dozen at most
        ranked.sort_by_key(|(rank, _)| *rank);
        let sections: Vec<Section> = ranked.into_iter().map(|(_, s)| s).collect();
        // A search jump scrolls once its page is laid out: the title is the list's first item.
        if let Some(group) = self.settings.as_mut().and_then(|v| v.pending_jump.take())
            && let Some(i) = sections.iter().position(|s| s.key == Some(group))
        {
            scroll.scroll_to_top_of_item(i + 1);
        }
        // The scroll list's first item is the page title, so a section's item index is one past its own.
        let anchors: Vec<usize> = page.sections().iter().map(|name| sections.iter().position(|s| s.anchor == Some(*name)).map_or(0, |i| i + 1)).collect();
        let title = (page != Page::Keybinds).then(|| ui::settings_title(page.label(), cx).flex_none());
        let body = ui::settings_page("settings-scroll", &scroll)
            .children(title.map(|t| column(page, t)))
            .children(sections.into_iter().map(|s| column(page, s.body)));
        let (bar, undo) = match page {
            Page::Profiles => (self.profiles_bar(cx), self.profiles_undo(cx)),
            _ => (None, None),
        };
        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(palette::surface(Level::Page, cx))
            .child(div().size_full().relative().child(body).child(gpui_kit::component::scroll::Scrollbar::vertical(&scroll)).with_animation(SharedString::from(format!("set-page-{page:?}")), Animation::new(ui::DUR_POP).with_easing(|t| motion::ease_out().sample(t)), |el, t| el.top(px(-PAGE_DROP * (1. - t))).opacity(t)))
            .children(bar)
            .children(undo)
            .child(ui::settings_close("settings-close", cx.listener(|this, _, _, cx| this.close_settings(cx)), cx));
        let nav = self.settings_nav(&anchors, cx).into_any_element();
        Some(self.settings_frame("settings", nav, main, cx).capture_key_down(cx.listener(Self::settings_search_key)).into_any_element())
    }

    fn settings_nav(&self, anchors: &[usize], cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.settings.as_ref().expect("settings are open");
        let query = self.settings_search.read(cx).value().to_lowercase();
        let searching = !query.is_empty();
        // Search first, then you as the My account row, as macOS System Settings does: one compact row
        // instead of an identity block stacked over the field.
        let search = div().flex_none().pb_3().child(ui::search_field("settings-search", &self.settings_search, cx).on_sidebar());
        let me = (!searching).then(|| self.me.clone()).flatten().map(|me| {
            let sidebar = crate::material::sidebar_fill(cx);
            let avatar = ui::badged(ui::avatar(me.avatar.clone(), ui::AVATAR_LG), ui::AVATAR_LG, ui::Shape::Round, sidebar).status(self.my_mark());
            let text = ui::col().min_h_0()
                .flex_1()
                .line_height(ui::LINE_DETAIL)
                .child(ui::line(me.name.clone()).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)))
                .child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(Page::Account.label()));
            ui::menu_row("nav-Account", cx).toggled(view.page == Page::Account, cx).h_auto().py_1p5().child(avatar).child(text).on_click(cx.listener(|this, _, window, cx| this.open_page(Page::Account, window, cx)))
        });
        let mut list: Vec<AnyElement> = Vec::new();
        if searching {
            list.push(div().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child("Results").into_any_element());
            list.extend(self.setting_results(&query, cx));
        }
        for (heading, pages) in NAV.iter().filter(|_| !searching) {
            list.push(div().pt_4().px_2p5().pb_1().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(*heading).into_any_element());
            for &page in *pages {
                let current = view.page == page;
                list.push(nav_item(page, current, cx).into_any_element());
                if current && !page.sections().is_empty() {
                    list.push(sub_items(page, view.section, anchors, cx).into_any_element());
                }
            }
        }
        let asking = view.asking == Some(Ask::LogOut);
        ui::settings_sidebar("settings-nav", &self.settings_nav_scroll, cx)
            .child(search)
            .children(me)
            .children(list)
            .child(div().px_2p5().py_2().flex_none().child(ui::divider(None, cx)))
            .child(self.log_out_item(cx))
            .children(asking.then(|| self.log_out_ask(cx)))
            .child(div().flex_1())
            .child(
                div()
                    .pt_4()
                    .px_2p5()
                    .pb_1()
                    .text_size(ui::TEXT_XS)
                    .line_height(ui::LINE_DETAIL)
                    .text_color(palette::muted(cx))
                    .child(format!("Waffle {} ({})", env!("CARGO_PKG_VERSION"), os_name())),
            )
    }

    fn log_out_item(&self, cx: &mut Context<Self>) -> impl IntoElement {
        ui::menu_row("nav-log-out", cx)
            .danger(cx)
            .child(Icon::new(Glyph::DoorExit).size(ui::ICON_LG))
            .child("Log out")
            .on_click(cx.listener(|this, _, _, cx| this.set_ask(Some(Ask::LogOut), cx)))
    }

    // Log out asks in place: what happens, then Stay or Log out.
    fn log_out_ask(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = ui::row().items_stretch()
            .gap_2()
            .child(ui::button("log-out-stay", Tone::Secondary, cx).small().flex_1().child("Stay").on_click(cx.listener(|this, _, _, cx| this.set_ask(None, cx))))
            .child(ui::button("log-out-yes", Tone::Danger, cx).small().flex_1().child("Log out").on_click(cx.listener(|this, _, window, cx| this.sign_out(window, cx))));
        reveal(
            "log-out-ask",
            div().py_1().child(
                ui::col()
                    .p_2p5()
                    .squircle(ui::RADIUS_MD)
                    .bg(palette::danger().opacity(0.1))
                    .gap_2()
                    .child(
                        ui::col()
                            .gap_0p5()
                            .text_size(ui::TEXT_SM)
                            .font_weight(FontWeight::NORMAL)
                            .text_color(palette::strong(cx))
                            .child("Log out of Waffle on this computer?")
                            .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child("You'll scan a new code with your phone to sign back in.")),
                    )
                    .child(buttons),
            ),
        )
    }

    fn set_ask(&mut self, ask: Option<Ask>, cx: &mut Context<Self>) {
        if let Some(view) = &mut self.settings {
            view.asking = ask;
        }
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    fn account_page(&self, cx: &mut Context<Self>) -> Vec<Section> {
        let Some(me) = self.me.clone() else { return Vec::new() };
        let fill = palette::surface(Level::Group, cx);
        let banner_fill = div().size_full().bg(gpui_kit::linear_gradient(135., gpui_kit::linear_color_stop(palette::data_color(BANNER.0), 0.), gpui_kit::linear_color_stop(palette::data_color(BANNER.1), 1.)));
        let banner_art = match &self.my_account.banner {
            Some(url) => ui::edge(crate::chat::animated_image(url.clone()).size_full().object_fit(gpui_kit::ObjectFit::Cover), ui::top(ui::RADIUS_XL)).into_any_element(),
            None => ui::edge(banner_fill, ui::top(ui::RADIUS_XL)).into_any_element(),
        };
        // Over a banner Waffle doesn't choose, so its own scrim, not a theme fill.
        let banner_button = ui::button("change-banner", Tone::OnMedia, cx).small().child("Change banner").on_click(cx.listener(|this, _, _, cx| this.pick_profile_image(profile::ProfileImage::Banner, cx)));
        let banner = div().relative().h(px(BANNER_HEIGHT)).child(banner_art).child(div().absolute().top_3().right_3().child(banner_button));
        let avatar = ui::badged(ui::avatar(me.avatar.clone(), PROFILE_AVATAR).border(px(AVATAR_RING)).border_color(fill), PROFILE_AVATAR, ui::Shape::Round, fill);
        // Your presence only once the live connection has confirmed it (see `my_mark`).
        let avatar = match self.my_mark() {
            Some(status) => avatar.mark(ui::Spot::BottomRight, ui::Badge::status(status, PROFILE_STATUS)),
            None => avatar,
        };
        let identity = ui::row()
            // lint-allow(raw-margin): the avatar rides up over the banner, which only a negative margin does
            .mt(px(-AVATAR_LIFT))
            .px_2p5()
            .items_end()
            .gap_4()
            .child(avatar)
            .child(ui::line(me.name.clone()).flex_1().pb_3().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
            .child(div().flex_none().pb_2p5().child(ui::button("change-avatar", Tone::Secondary, cx).small().child("Change avatar").on_click(cx.listener(|this, _, _, cx| this.pick_profile_image(profile::ProfileImage::Avatar, cx)))));
        let copied = self.settings.as_ref().is_some_and(|s| s.copied);
        let id = me.id.clone();
        let copy = ui::button("copy-user-id", Tone::Secondary, cx).small()
            .min_w_24()
            .when(copied, |b| b.text_color(palette::positive_ink(cx)))
            .child(Icon::new(if copied { Glyph::Checkmark } else { Glyph::Copy }).size(ui::ICON_MD))
            .child(if copied { "Copied" } else { "Copy ID" })
            .on_click(cx.listener(move |this, _, _, cx| this.copy_user_id(&id, cx)));
        let mut lines = Hairlines::default();
        let fields = ui::col()
            .pt_3()
            .px_1()
            .pb_1()
            .child(self.profile_row("acct-name", profile::ProfileField::DisplayName, "Display name", me.name.clone().into(), lines.next(), cx))
            .child(self.profile_row("acct-username", profile::ProfileField::Username, "Username", self.my_account.username.clone().into(), lines.next(), cx))
            .child(ui::settings_row("acct-id", lines.next(), cx).child(field_text("User ID", me.id.clone(), cx)).child(copy));
        let card = ui::col().squircle(ui::RADIUS_XL).bg(fill).child(banner).child(identity).child(fields);
        vec![Section::keyed("Account info", ui::settings_group("Account info", cx).child(card))]
    }

    // A field with Edit at the end, which turns the value into its input in place.
    fn profile_row(&self, id: &'static str, field: profile::ProfileField, label: &'static str, value: SharedString, line: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.settings.as_ref().and_then(|v| v.editing.as_ref()).is_some_and(|e| e.field == field);
        ui::settings_row(id, line, cx).child(self.profile_value(field, label, value, cx)).when(!editing, |row| {
            row.child(ui::button(SharedString::from(format!("{id}-edit")), Tone::Secondary, cx).small().child("Edit").on_click(cx.listener(move |this, _, window, cx| this.start_profile_edit(field, window, cx))))
        })
    }

    fn copy_user_id(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(id.to_owned()));
        if let Some(view) = &mut self.settings {
            view.copied = true;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |this, cx| {
                if let Some(view) = &mut this.settings {
                    view.copied = false;
                }
                // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
                cx.notify();
            })
            .ok();
        })
        // lint-allow(detached-task): a one-shot task that ends when its request answers, and it updates through a WeakEntity so a closed view drops the result
        .detach();
        // lint-allow(global-notify): the settings window is built in Waffle's own render_root, not in a column, so only an app redraw shows this change
        cx.notify();
    }

    fn notifications_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let mut lines = Hairlines::default();
        let rows: Vec<AnyElement> = NOTIFY_CUES.iter().map(|&(id, label, cue)| self.cue_row(id, label, cue, lines.next(), window, cx)).collect();
        vec![Section::keyed(
            "Sounds",
            ui::settings_group("Sounds", cx)
                .child(ui::settings_card(cx).children(rows))
                .child(ui::settings_caption("Sounds Waffle plays for new direct messages and for mentions. Voice sounds are under Voice and video.", cx)),
        )]
    }

    fn appearance_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let current = self.prefs.theme;
        let os_dark = matches!(window.appearance(), gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark);
        let pick = |id: &'static str, pref: ThemePref, preview: AnyElement, label: &'static str, note: Option<&'static str>, cx: &mut Context<Self>| {
            let on = current == pref;
            ui::pressable(div().id(id), 0.97)
                .flex_1()
                .min_w_0()
                .pt_1p5()
                .px_1p5()
                .pb_2p5()
                .squircle(ui::RADIUS_LG)
                .flex()
                .flex_col()
                .gap_2()
                .cursor_pointer()
                .font_weight(FontWeight::NORMAL)
                .text_color(if on { palette::strong(cx) } else { palette::text(cx) })
                .when(on, |d| d.bg(palette::selected(cx)))
                .when(!on, |d| {
                    let hover = palette::hover(cx);
                    d.hover(move |s| s.bg(hover))
                })
                .child(
                    div()
                        .squircle(ui::RADIUS_MD)
                        .shadow(vec![ring(if on { palette::accent() } else { palette::line_strong(cx) }, if on { 2. } else { 1. })])
                        .child(preview),
                )
                .child(
                    ui::col()
                        .px_1()
                        .gap_px()
                        .child(label)
                        .children(note.map(|n| div().text_size(ui::TEXT_XS).font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child(n))),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.update_prefs(cx, |prefs| prefs.theme = pref);
                    palette::sync(window, cx);
                }))
        };
        let picks = ui::row().items_stretch()
            .gap_3()
            .child(pick("theme-light", ThemePref::Light, mini_window(&MINI_LIGHT).into_any_element(), "Light", None, cx))
            .child(pick("theme-dark", ThemePref::Dark, mini_window(&MINI_DARK).into_any_element(), "Midnight", None, cx))
            .child(pick("theme-system", ThemePref::System, mini_sync(), "Sync with OS", Some(if os_dark { "Midnight now" } else { "Light now" }), cx));
        let text = ui::settings_card(cx)
            .child(self.font_row(Select::UiFont, "Interface font", Some("Menus, messages and everything else"), &self.prefs.ui_font, UI_FONT, false, cx))
            .child(self.font_row(Select::MonoFont, "Code font", Some("Code blocks and inline code"), &self.prefs.mono_font, MONO_FONT, true, cx));
        let material = crate::material::setting_name().map(|name| {
            let row = self.switch_row(SwitchRow { id: "set-native-sidebar", title: name, detail: None, on: self.prefs.native_sidebar, enabled: true, field: |p| &mut p.native_sidebar }, false, window, cx);
            Section::keyed(
                "Sidebar",
                ui::settings_group("Sidebar", cx).child(ui::settings_card(cx).child(row)).child(ui::settings_caption("Lets your wallpaper's colour through the sidebar. In full screen the sidebar stays solid.", cx)),
            )
        });
        let mut sections = vec![
            Section::keyed("Theme", ui::settings_group("Theme", cx).child(picks).child(ui::settings_caption("Midnight is the darkest theme, the one Waffle is built on.", cx)).child(self.accessibility_link(cx))),
            Section::keyed("Text", ui::settings_group("Text", cx).child(text).child(ui::settings_caption("Any font installed on this computer. Figtree and Roboto Mono come with Waffle.", cx))),
            Section::keyed("App icon", ui::settings_group("App icon", cx).child(self.app_icon_grid(cx)).child(ui::settings_caption("How Waffle looks in your Dock: the same waffle in different media. Changes right away; no restart.", cx))),
        ];
        sections.splice(1..1, material);
        sections
    }

    // A font setting: its name and detail, the chosen face's name set in that face, and the list of every
    // installed font with Waffle's bundled default first.
    #[allow(clippy::too_many_arguments)]
    fn font_row(&self, select: Select, title: &'static str, detail: Option<&'static str>, chosen: &str, default: &'static str, line: bool, cx: &mut Context<Self>) -> AnyElement {
        let options: Vec<(String, String)> = std::iter::once((default.to_owned(), format!("{default} (default)")))
            .chain(installed_fonts(cx).iter().filter(|f| f.as_str() != default).map(|f| (f.clone(), f.clone())))
            .collect();
        let value: SharedString = if chosen == default { format!("{default} (default)").into() } else { chosen.to_owned().into() };
        let field = div().w(px(FONT_SELECT_WIDTH)).flex_none().font_family(chosen.to_owned()).child(self.select_field(select, None, &options, chosen, value, 36., cx));
        ui::settings_row(SharedString::from(format!("row-{select:?}")), line, cx).child(ui::settings_text(title, detail, cx)).child(field).into_any_element()
    }

    fn app_icon_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.prefs.app_icon;
        let tiles = crate::app_icon::AppIcon::ALL.map(|icon| {
            let on = icon == current;
            let hover = palette::hover(cx);
            ui::pressable(div().id(SharedString::from(format!("app-icon-{}", icon.label()))), 0.95)
                .w(px(ICON_TILE_WIDTH))
                .pt_2p5()
                .px_1()
                .pb_2()
                .squircle(ui::RADIUS_LG)
                .flex()
                .flex_col()
                .items_center()
                .gap_1p5()
                .cursor_pointer()
                .text_size(ui::TEXT_XS)
                .font_weight(FontWeight::NORMAL)
                .text_color(if on { palette::strong(cx) } else { palette::text(cx) })
                .when(on, |d| d.bg(palette::selected(cx)).shadow(vec![inset_ring(palette::accent(), 2.)]))
                .when(!on, |d| d.hover(move |s| s.bg(hover)))
                .child(img(icon.asset()).size(px(ICON_PICTURE)))
                .child(icon.label())
                // lint-allow(raw-margin): an optical nudge that tucks the Default note up under its name, tighter than the tile's gap
                .children((icon == crate::app_icon::AppIcon::default()).then(|| div().mt_neg_1().font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child("Default")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_prefs(cx, |prefs| prefs.app_icon = icon);
                    icon.apply();
                }))
        });
        ui::row().items_stretch().flex_wrap().gap_2().children(tiles).into_any_element()
    }

}

pub(crate) enum Fill {
    Accent,
    Threshold(Option<f32>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ask {
    LogOut,
    Reset,
}

// A group on a settings page, with the nav anchor (Voice, Camera, …) that scrolls to it.
struct Section {
    anchor: Option<&'static str>,
    // The registry group it holds, for a search jump to scroll to.
    key: Option<&'static str>,
    body: AnyElement,
}

impl Section {
    fn new(body: impl IntoElement) -> Self {
        Self { anchor: None, key: None, body: body.into_any_element() }
    }

    fn keyed(key: &'static str, body: impl IntoElement) -> Self {
        Self { anchor: None, key: Some(key), body: body.into_any_element() }
    }

    fn with_anchor(anchor: &'static str, body: impl IntoElement) -> Self {
        Self { anchor: Some(anchor), key: Some(anchor), body: body.into_any_element() }
    }
}

struct SwitchRow {
    id: &'static str,
    title: &'static str,
    detail: Option<&'static str>,
    on: bool,
    enabled: bool,
    field: fn(&mut Prefs) -> &mut bool,
}

// Rows in a card are split by inset hairlines: every row but the first draws one above itself.
#[derive(Default)]
struct Hairlines(bool);

impl Hairlines {
    fn next(&mut self) -> bool {
        std::mem::replace(&mut self.0, true)
    }
}

// The profile page's avatar and the presence mark on it, larger than any face elsewhere.
// The account card's banner height, the border ringing its avatar, how far the avatar rides up over the banner, and
// how far the page drops while it fades in.
const BANNER_HEIGHT: f32 = 120.;
const AVATAR_RING: f32 = 6.;
const AVATAR_LIFT: f32 = 42.;
const PAGE_DROP: f32 = 4.;
// A font picker's width, an app icon tile's width and its picture, the nav sub-item's indent.
const FONT_SELECT_WIDTH: f32 = 240.;
const ICON_TILE_WIDTH: f32 = 88.;
const ICON_PICTURE: f32 = 56.;
const SUB_ITEM_INDENT: f32 = 38.;
const PROFILE_AVATAR: f32 = 100.;
const PROFILE_STATUS: f32 = 28.;

const COPIED_FOR: Duration = Duration::from_secs(2);
const UNDO_FOR: Duration = Duration::from_secs(8);
const WHY_VAD: &str = "Works with automatic sensitivity in voice activity mode";
const MAC_MIC_PRIVACY: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
const MAC_INPUT_PRIVACY: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
// The account banner's gradient: oklch(0.6 0.14 300) to oklch(0.45 0.15 340).
const BANNER: (u32, u32) = (0x8E6AC7, 0x88296F);

const NOTIFY_CUES: [(&str, &str, Cue); 2] = [("cue-message", "Message", Cue::Message), ("cue-mention", "Mention", Cue::Mention)];
const VOICE_CUES: [(&str, &str, Cue); 4] = [("cue-deafen", "Deafen", Cue::Deafen), ("cue-undeafen", "Undeafen", Cue::Undeafen), ("cue-mute", "Mute", Cue::Mute), ("cue-unmute", "Unmute", Cue::Unmute)];
const MORE_CUES: [(&str, &str, Cue); 16] = [
    ("cue-disconnect", "Voice disconnected", Cue::Disconnect),
    ("cue-ptt-start", "PTT activate", Cue::PttStart),
    ("cue-ptt-stop", "PTT deactivate", Cue::PttStop),
    ("cue-user-join", "User join", Cue::UserJoin),
    ("cue-user-leave", "User leave", Cue::UserLeave),
    ("cue-user-moved", "User moved", Cue::UserMoved),
    ("cue-stream-started", "Stream started", Cue::StreamStarted),
    ("cue-stream-stopped", "Stream stopped", Cue::StreamEnded),
    ("cue-viewer-join", "Viewer join", Cue::StreamUserJoined),
    ("cue-viewer-leave", "Viewer leave", Cue::StreamUserLeft),
    ("cue-activity-launch", "Activity started", Cue::ActivityLaunch),
    ("cue-activity-end", "Activity ended", Cue::ActivityEnd),
    ("cue-activity-join", "Activity player join", Cue::ActivityUserJoin),
    ("cue-activity-leave", "Activity player leave", Cue::ActivityUserLeft),
    ("cue-outgoing-ring", "Outgoing ring", Cue::CallCalling),
    // GAP: nothing plays this yet; incoming DM calls (CALL_CREATE) aren't handled.
    ("cue-incoming-ring", "Incoming ring", Cue::CallRinging),
];

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

fn nav_item(page: Page, selected: bool, cx: &mut Context<Waffle>) -> impl IntoElement {
    ui::settings_nav_item(SharedString::from(format!("nav-{page:?}")), page.glyph(), page.label(), selected, cx).on_click(cx.listener(move |this, _, window, cx| this.open_page(page, window, cx)))
}

// The open page's sections, listed under it in the nav.
fn sub_items(page: Page, current: usize, anchors: &[usize], cx: &mut Context<Waffle>) -> impl IntoElement {
    let (strong, muted, hover) = (palette::strong(cx), palette::muted(cx), palette::hover(cx));
    let anchors = anchors.to_vec();
    ui::col().pt_0p5().pb_1().gap_px().children(page.sections().iter().enumerate().map(move |(i, name)| {
        let item = anchors.get(i).copied().unwrap_or(0);
        // lint-allow(custom-button): ui::settings_nav_item has no icon-less, indented sub-entry
        ui::row()
            .id(SharedString::from(format!("sub-{name}")))
            .min_h_7()
            .pl(px(SUB_ITEM_INDENT))
            .pr_2p5()
            .squircle(ui::RADIUS_SM)
            .cursor_pointer()
            .font_weight(FontWeight::NORMAL)
            .text_color(if i == current { strong } else { muted })
            .hover(move |s| s.bg(hover).text_color(strong))
            .child(*name)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(s) = &mut this.settings {
                    s.section = i;
                    s.scroll.scroll_to_top_of_item(item);
                }
                cx.notify();
            }))
    }))
}

// The page's centred column; Profiles runs wider to fit its preview beside the fields.
fn column(page: Page, child: impl IntoElement) -> Div {
    ui::settings_column(child).when(page == Page::Profiles, |c| c.max_w(px(profiles::PAGE_WIDTH)))
}

fn card_pad(cx: &App) -> Div {
    ui::settings_card(cx).p_4().gap_4()
}

// A row whose control sits under its text, full width.
fn stack_row(id: &'static str, line: bool, cx: &App) -> Stateful<Div> {
    ui::settings_row(id, line, cx).flex_col().items_stretch().gap_3()
}

// A label over its value, as Account info lists them.
pub(super) fn field_text(label: &'static str, value: impl Into<SharedString>, cx: &App) -> Div {
    ui::col().min_h_0()
        .flex_1()
        .gap_0p5()
        .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(label))
        .child(div().font_weight(FontWeight::NORMAL).text_color(palette::strong(cx)).child(value.into()))
}

pub(super) fn field_label(text: &'static str, cx: &App) -> Div {
    div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(text)
}

// A title and description with the value it sets on the right.
fn numbered(title: &'static str, detail: &'static str, value: &str, cx: &App) -> Div {
    ui::row().items_start().justify_between().gap_4().child(ui::settings_text(title, Some(detail), cx)).child(div().whitespace_nowrap().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(value.to_owned()))
}

fn scale(stops: &[&'static str], cx: &App) -> Div {
    ui::row().items_stretch().justify_between().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).children(stops.iter().copied())
}

fn radio_row(id: &'static str, line: bool, title: &'static str, detail: &'static str, on: bool, cx: &App) -> Stateful<Div> {
    ui::settings_choice(ui::settings_row(id, line, cx), cx).items_start().child(div().pt_0p5().child(ui::radio(on, cx))).child(ui::settings_text(title, Some(detail), cx))
}

// A row that reveals more rows under it; closed, it names what's inside.
#[allow(clippy::too_many_arguments)]
fn expander_row(id: &'static str, line: bool, title: &str, summary: &str, open: bool, window: &mut Window, cx: &mut App) -> Stateful<Div> {
    let angle = turn(format!("chevron-{id}"), if open { 90. } else { 0. }, window, cx);
    let chev = ui::row()
        .size_8()
        .flex_none()
        .squircle(ui::RADIUS_SM)
        .bg(palette::selected(cx))
        .text_color(palette::strong(cx))
        .justify_center()
        .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD).rotate(angle));
    ui::settings_choice(ui::settings_row(id, line, cx), cx).child(ui::settings_text(title.to_owned(), (!open).then(|| summary.to_owned()), cx)).child(chev)
}

fn error_line(text: &'static str, cx: &App) -> Div {
    ui::row()
        .px_0p5()
        .items_start()
        .gap_1p5()
        .text_size(ui::TEXT_XS)
        .font_weight(FontWeight::NORMAL)
        .text_color(palette::danger_ink(cx))
        .child(div().pt_px().flex_none().child(Icon::new(Glyph::WarningCircle).size(ui::ICON_SM)))
        .child(text)
}

// Keycaps for a recorded binding, with the platform's own key names.
pub(crate) fn key_caps(keys: &str, cx: &App) -> Div {
    ui::row().gap_1().children(keys.split('+').map(|key| ui::kbd(key_name(key), cx)))
}

// ⌘ ⇧ ⌥ ⌃ on macOS, printed names elsewhere; arrows as arrows, letters upper-case.
pub(crate) fn key_name(key: &str) -> SharedString {
    let mac = cfg!(target_os = "macos");
    let name = match key {
        "cmd" if mac => "\u{2318}",
        "cmd" => "Win",
        "shift" if mac => "\u{21E7}",
        "shift" => "Shift",
        "alt" if mac => "\u{2325}",
        "alt" => "Alt",
        "ctrl" if mac => "\u{2303}",
        "ctrl" => "Ctrl",
        "ArrowUp" | "up" => "\u{2191}",
        "ArrowDown" | "down" => "\u{2193}",
        "ArrowLeft" | "left" => "\u{2190}",
        "ArrowRight" | "right" => "\u{2192}",
        "backspace" if mac => "\u{232B}",
        "pageup" => "Page Up",
        "pagedown" => "Page Down",
        "escape" => "Esc",
        "enter" if mac => "Return",
        "enter" => "Enter",
        "plus" => "+",
        key => {
            let key = key.strip_prefix("Key").or_else(|| key.strip_prefix("Digit")).filter(|k| k.chars().count() == 1).unwrap_or(key);
            let mut chars = key.chars();
            return chars.next().map_or(SharedString::default(), |first| format!("{}{}", first.to_uppercase(), chars.as_str()).into());
        }
    };
    name.into()
}

// Discord's labels in Title Case, read as sentences.
fn sentence(label: &str) -> String {
    let mut chars = label.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars.as_str().to_lowercase().chars()).collect())
}

// A small window in each theme, on fixed colours, so a light preview reads light in the dark theme.
// Every font family installed on this computer, once per run, alphabetical; names starting with a dot are
// the system's private UI faces. GAP: a font installed while Waffle runs shows after a restart.
fn installed_fonts(cx: &App) -> &'static Vec<String> {
    static FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FONTS.get_or_init(|| {
        let mut names: Vec<String> = cx.text_system().all_font_names().into_iter().filter(|n| !n.starts_with('.')).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    })
}

// The collapsed accordion lists its first settings: three under Custom, two otherwise (Discord `tI`).
fn advanced_summary(custom: bool) -> String {
    let custom_only = ["Automatic gain control", "Advanced voice activity"];
    let always = ["No audio detected warning", "Global attenuation", "Quality of service"];
    let shown = custom_only.iter().filter(|_| custom).chain(always.iter().filter(|t| **t != "Global attenuation" || voice::ATTENUATION_SUPPORTED));
    shown.take(if custom { 3 } else { 2 }).copied().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frecency_favours_recent_use_over_old_bulk() {
        let now = 100 * 86_400;
        let days_ago = |d: i64| now - d * 86_400;
        let old = frecency(10, &[days_ago(60); 10], now);
        let fresh = frecency(3, &[days_ago(1); 3], now);
        assert_eq!(old, 100, "ten uses two months back weigh 10 each");
        assert_eq!(fresh, 300, "three uses yesterday weigh 100 each");
        assert!(fresh > old);
        assert_eq!(frecency(7, &[], now), 7, "counts from before times were kept");
    }

    #[test]
    fn mic_test_gives_deafen_back_as_last_chosen() {
        for before in [false, true] {
            let mut deafened = before;
            let held = HeldDeafen::hold(&mut deafened);
            assert!(deafened, "deafened while the test runs");
            held.release(&mut deafened);
            assert_eq!(deafened, before, "restored after the test");
        }

        let mut deafened = true;
        let mut held = HeldDeafen::hold(&mut deafened);
        deafened = false;
        held.chosen(deafened);
        deafened = true;
        held.release(&mut deafened);
        assert!(!deafened, "undeafening during the test sticks");
    }

    #[test]
    fn prefs_round_trip_and_profiles() {
        let mut prefs = Prefs { push_to_talk: true, noise_mode: NoiseMode::Standard, ..Prefs::default() };
        prefs.set_profile(InputProfile::Studio);
        let studio = prefs.voice_settings();
        assert_eq!(studio.noise_suppression, voice::NoiseSuppression::Off);
        assert!(!studio.echo_cancellation && !studio.auto_gain);
        assert_eq!(studio.input_mode, voice::InputMode::VoiceActivity { auto: false, threshold_db: -84., advanced: false }, "Studio forces voice activity");
        prefs.set_profile(InputProfile::VoiceIsolation);
        assert_eq!(prefs.voice_settings().noise_suppression, voice::NoiseSuppression::Advanced);
        prefs.set_profile(InputProfile::Custom);
        assert_eq!(prefs.voice_settings().noise_suppression, voice::NoiseSuppression::Standard, "Custom gets its own switches back");
        assert!(matches!(prefs.voice_settings().input_mode, voice::InputMode::PushToTalk { .. }));
        prefs.push_to_talk = false;
        let text = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&text).unwrap(), prefs);
        let mut empty = serde_json::from_str::<Prefs>("{}").unwrap();
        empty.add_new_keybinds();
        assert_eq!(empty, Prefs::default());
        Slider::Sensitivity.set(&mut prefs, 0.4);
        assert_eq!(prefs.sensitivity_db, -60.);
        assert!((Slider::Sensitivity.fraction(&prefs) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn defaults_match_discord_and_old_files_migrate() {
        let settings = Prefs::default().voice_settings();
        assert_eq!(settings.input_mode, voice::InputMode::VoiceActivity { auto: true, threshold_db: -60., advanced: true });
        assert_eq!(settings.noise_suppression, voice::NoiseSuppression::Advanced);
        assert!(settings.echo_cancellation && settings.auto_gain && !settings.qos);
        assert_eq!(settings.attenuation, voice::Attenuation { strength: 0, when_self_speaks: false, when_others_speak: true });

        // A settings file from before this change: no attenuation, VAD, warning or QoS fields.
        let old = r#"{"theme":"Dark","input_profile":"Custom","sensitivity_auto":false,"sensitivity_db":-42.0,
            "noise_suppression":true,"noise_mode":"Standard","echo_cancellation":false,"auto_gain":true,"push_to_talk":false}"#;
        let prefs: Prefs = serde_json::from_str(old).unwrap();
        assert_eq!((prefs.sensitivity_db, prefs.noise_mode, prefs.echo_cancellation), (-42., NoiseMode::Standard, false), "kept");
        assert!(prefs.advanced_voice_activity && prefs.attenuate_others && prefs.silence_warning, "new fields take Discord's defaults");
        assert_eq!((prefs.attenuation, prefs.attenuate_self, prefs.qos), (0, false, false));
        assert_eq!(prefs.voice_settings().input_mode, voice::InputMode::VoiceActivity { auto: false, threshold_db: -42., advanced: true });
        let mut p = prefs;
        Slider::Attenuation.set(&mut p, 0.355);
        assert_eq!(p.attenuation, 36);
    }

    #[test]
    fn advanced_summary_lists_the_first_visible_settings() {
        assert_eq!(advanced_summary(true), "Automatic gain control, Advanced voice activity, No audio detected warning");
        let second = if voice::ATTENUATION_SUPPORTED { "Global attenuation" } else { "Quality of service" };
        assert_eq!(advanced_summary(false), format!("No audio detected warning, {second}"));
    }

    #[test]
    fn older_prefs_get_the_share_keybind_once() {
        let shares = |p: &Prefs| p.keybinds.iter().filter(|k| **k == share_keybind()).count();
        let mut old = serde_json::from_str::<Prefs>(r#"{"keybinds":[{"action":"ToggleMute","keys":"alt+m"}]}"#).unwrap();
        assert_eq!(old.keybinds_version, 0);
        old.add_new_keybinds();
        assert_eq!(shares(&old), 1);
        old.keybinds.retain(|k| k.action != Action::ToggleScreenShare);
        let mut reread = serde_json::from_str::<Prefs>(&serde_json::to_string(&old).unwrap()).unwrap();
        reread.add_new_keybinds();
        assert_eq!(shares(&reread), 0, "a deleted default stays deleted");
    }
}
