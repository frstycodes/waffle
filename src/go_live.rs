// Go Live: the picker (what to share, its quality and sound), and its memory: the last source we
// shared, found again next time even when its window is gone, and what the Toggle Screen Share
// keybind does with it.
use std::sync::Arc;

use gpui_kit::{
    Animation, AnimationExt, AnyElement, AppContext, Context, Div, FontWeight, Hsla, MouseButton, RenderImage, SharedString, Window,
    component::Icon,
    div, img, prelude::*, px,
};
use serde::{Deserialize, Serialize};

use crate::{
    Waffle,
    icons::Glyph,
    palette,
    ui::{self, Squircle},
    voice::{self, ShareSource, ShareableScreen, ShareableWindow},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LastShare {
    Screen { id: u32 },
    // A window id dies with its window and can come back as another app's after a restart, so the
    // app is kept too, to check the id against and to find a sibling window by.
    Window { id: u32, pid: i32, app_name: String },
}

impl LastShare {
    pub fn of(source: ShareSource, windows: &[ShareableWindow]) -> Option<Self> {
        match source {
            ShareSource::Screen(id) => Some(Self::Screen { id }),
            ShareSource::Window(id) => windows.iter().find(|w| w.id == id).map(|w| Self::Window { id, pid: w.pid, app_name: w.app_name.clone() }),
            ShareSource::App(_) => None,
        }
    }

    // What's still on offer: the same screen or window, else another window of the same process, else
    // of the same app. None when the app has no windows left or the screen is unplugged.
    pub fn find(&self, windows: &[ShareableWindow], screens: &[ShareableScreen]) -> Option<(ShareSource, String)> {
        match self {
            Self::Screen { id } => screens.iter().find(|s| s.id == *id).map(screen_choice),
            Self::Window { id, pid, app_name } => {
                let same_app = |w: &&ShareableWindow| &w.app_name == app_name;
                windows
                    .iter()
                    .filter(same_app)
                    .find(|w| w.id == *id)
                    .or_else(|| windows.iter().filter(same_app).find(|w| w.pid == *pid))
                    .or_else(|| windows.iter().find(same_app))
                    .map(window_choice)
            }
        }
    }
}

// The picker's opening selection: the remembered source as `find` resolves it, else Screen 1.
pub fn preselect(last: Option<&LastShare>, windows: &[ShareableWindow], screens: &[ShareableScreen]) -> Option<(ShareSource, String)> {
    last.and_then(|l| l.find(windows, screens)).or_else(|| screens.first().map(screen_choice))
}

fn screen_choice(screen: &ShareableScreen) -> (ShareSource, String) {
    (ShareSource::Screen(screen.id), screen.name.clone())
}

fn window_choice(window: &ShareableWindow) -> (ShareSource, String) {
    (ShareSource::Window(window.id), window.title.clone())
}

// What the Toggle Screen Share keybind shares: the detected game's window, else the whole primary screen,
// never whatever app happens to be in front. None (no screen at all) opens the picker. Takes the game's
// first listed window: a game that keeps a launcher window open may need picking by size instead.
pub fn keybind_source(game_pid: Option<i32>, windows: &[ShareableWindow], screens: &[ShareableScreen]) -> Option<(ShareSource, String)> {
    game_pid.and_then(|pid| windows.iter().find(|w| w.pid == pid)).map(window_choice).or_else(|| screens.first().map(screen_choice))
}

#[derive(Debug, PartialEq)]
pub enum KeybindMove {
    Nothing,
    Stop,
    Start,
}

pub fn keybind_move(in_call: bool, live: bool) -> KeybindMove {
    match (in_call, live) {
        (false, _) => KeybindMove::Nothing,
        (true, true) => KeybindMove::Stop,
        (true, false) => KeybindMove::Start,
    }
}

impl Waffle {
    pub(crate) fn toggle_share_keybind(&mut self, cx: &mut Context<Self>) {
        let in_call = self.call.as_ref().is_some_and(|c| c.connected);
        let live = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        match keybind_move(in_call, live) {
            KeybindMove::Nothing => {}
            KeybindMove::Stop => self.toggle_screen_share(cx),
            KeybindMove::Start => self.share_from_keybind(cx),
        }
    }

    // The game is read now at the key press; listing windows takes tens of milliseconds, so it runs off the
    // UI thread, and the call may have changed by the time it's back.
    fn share_from_keybind(&mut self, cx: &mut Context<Self>) {
        let game = crate::games::running_pid().map(|pid| pid as i32);
        let listing = cx.background_spawn(async { (voice::shareable_windows().unwrap_or_default(), voice::screens().unwrap_or_default()) });
        cx.spawn(async move |this, cx| {
            let (windows, screens) = listing.await;
            this.update(cx, |this, cx| {
                let ready = this.call.as_ref().is_some_and(|c| c.connected && c.sharing.is_none());
                if !ready {
                    return;
                }
                match keybind_source(game, &windows, &screens) {
                    Some((source, label)) => this.share_and_remember(source, label, &windows, cx),
                    None => this.open_share_picker(cx),
                }
            })
            .ok();
        })
        .detach();
    }

    // Goes live through `start_share`, then remembers the source once the stream is really up.
    pub(crate) fn share_and_remember(&mut self, source: ShareSource, label: String, windows: &[ShareableWindow], cx: &mut Context<Self>) {
        let last = LastShare::of(source, windows);
        self.share_choice_label = Some(label);
        self.start_share(source, cx);
        let live = self.call.as_ref().is_some_and(|c| c.sharing.is_some());
        if live && last.is_some() && self.prefs.last_share != last {
            self.prefs.last_share = last;
            self.prefs.save();
        }
    }
}

// The Go Live picker (design/voice.html `.dialog.golive`): what to share, its quality and sound,
// before anyone sees it.
const DIALOG_WIDTH: f32 = 600.;
// Two rows of sources before the grid scrolls.
const SOURCES_HEIGHT: f32 = 300.;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ShareTab {
    Screens,
    Windows,
    Devices,
}

pub struct SharePicker {
    tab: ShareTab,
    windows: Option<Result<Vec<ShareableWindow>, SharedString>>,
    screens: Option<Result<Vec<ShareableScreen>, SharedString>>,
    thumbs: Vec<(ShareSource, Arc<RenderImage>)>,
    icons: Vec<(i32, Arc<RenderImage>)>,
    // Whether the user has picked; the preselection lands without motion.
    picked: bool,
}

impl SharePicker {
    fn thumb(&self, source: ShareSource) -> Option<Arc<RenderImage>> {
        self.thumbs.iter().find(|(s, _)| *s == source).map(|(_, image)| image.clone())
    }

    fn icon(&self, source: ShareSource) -> Option<Arc<RenderImage>> {
        let pid = match source {
            ShareSource::Screen(_) => return None,
            ShareSource::App(pid) => pid,
            ShareSource::Window(id) => self.windows.as_ref()?.as_ref().ok()?.iter().find(|w| w.id == id)?.pid,
        };
        self.icons.iter().find(|(p, _)| *p == pid).map(|(_, image)| image.clone())
    }
}

impl Waffle {
    // Opens the picker, lists what can be shared, preselects the last source (or Screen 1), then fills
    // in icons and thumbnails off the UI thread, the preselected source's first.
    pub(crate) fn open_share_picker(&mut self, cx: &mut Context<Self>) {
        self.empty_share_picker(cx);
        let listing = cx.background_spawn(async { (voice::shareable_windows(), voice::screens()) });
        cx.spawn(async move |this, cx| {
            let (windows, screens) = listing.await;
            let mut pids: Vec<i32> = windows.iter().flatten().map(|w| w.pid).collect();
            pids.sort_unstable();
            pids.dedup();
            let mut sources: Vec<ShareSource> = screens.iter().flatten().map(|s| ShareSource::Screen(s.id)).chain(windows.iter().flatten().map(|w| ShareSource::Window(w.id))).collect();
            let open = this.update(cx, |this, cx| this.list_shareables(windows, screens, cx)).ok().flatten();
            let Some(first) = open else { return };
            sources.sort_by_key(|s| Some(*s) != first);
            let icons = cx.background_spawn(async move { pids.into_iter().filter_map(|pid| Some((pid, voice::app_icon(pid, 40)?))).collect::<Vec<_>>() }).await;
            let icons: Vec<_> = icons.into_iter().filter_map(|(pid, frame)| Some((pid, crate::render_image(Arc::new(frame))?))).collect();
            let kept = this.update(cx, |this, cx| {
                let Some(picker) = &mut this.share_picker else { return false };
                picker.icons.extend(icons.iter().cloned());
                cx.notify();
                true
            });
            if !kept.unwrap_or(false) {
                cx.update(|cx| icons.into_iter().for_each(|(_, image)| cx.drop_image(image, None)));
                return;
            }
            for source in sources {
                let Ok(frame) = cx.background_spawn(async move { voice::thumbnail(source, 400) }).await else { continue };
                let Some(image) = crate::render_image(Arc::new(frame)) else { continue };
                let kept = this.update(cx, |this, cx| {
                    let Some(picker) = &mut this.share_picker else { return false };
                    picker.thumbs.push((source, image.clone()));
                    cx.notify();
                    true
                });
                if !kept.unwrap_or(false) {
                    cx.update(|cx| cx.drop_image(image, None));
                    return;
                }
            }
        })
        .detach();
    }

    pub(crate) fn empty_share_picker(&mut self, cx: &mut Context<Self>) {
        self.close_share_picker(cx);
        self.share_picker = Some(SharePicker { tab: ShareTab::Windows, windows: None, screens: None, thumbs: Vec::new(), icons: Vec::new(), picked: false });
        self.share_choice = None;
        self.share_choice_label = None;
        cx.notify();
    }

    // Fills the open picker's lists and preselects; the selection, or None once the picker has closed.
    pub(crate) fn list_shareables(&mut self, windows: Result<Vec<ShareableWindow>, String>, screens: Result<Vec<ShareableScreen>, String>, cx: &mut Context<Self>) -> Option<Option<ShareSource>> {
        let preselected = preselect(self.prefs.last_share.as_ref(), windows.as_deref().unwrap_or_default(), screens.as_deref().unwrap_or_default());
        let picker = self.share_picker.as_mut()?;
        picker.windows = Some(windows.map_err(Into::into));
        picker.screens = Some(screens.map_err(Into::into));
        if self.share_choice.is_none()
            && let Some((source, label)) = preselected
        {
            picker.tab = if matches!(source, ShareSource::Screen(_)) { ShareTab::Screens } else { ShareTab::Windows };
            self.share_choice = Some(source);
            self.share_choice_label = Some(label);
        }
        cx.notify();
        Some(self.share_choice)
    }

    // Closes the picker and frees its thumbnails; true when it was open.
    pub(crate) fn close_share_picker(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(picker) = self.share_picker.take() else { return false };
        for image in picker.thumbs.into_iter().map(|(_, i)| i).chain(picker.icons.into_iter().map(|(_, i)| i)) {
            cx.drop_image(image, None);
        }
        true
    }

    fn pick_share(&mut self, source: ShareSource, label: String, cx: &mut Context<Self>) {
        if self.share_choice == Some(source) {
            return;
        }
        let Some(picker) = &mut self.share_picker else { return };
        picker.picked = true;
        self.share_choice = Some(source);
        self.share_choice_label = Some(label);
        cx.notify();
    }

    fn go_live_from_picker(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.share_choice.take() else { return };
        let label = self.share_choice_label.take().unwrap_or_default();
        let windows = self.share_picker.as_ref().and_then(|p| p.windows.as_ref()?.as_ref().ok().cloned()).unwrap_or_default();
        self.share_and_remember(source, label, &windows, cx);
    }

    // Points at the Toggle Screen Share keybind while one works; otherwise says who can watch.
    fn share_hint(&self) -> String {
        let keybind = self.prefs.keybinds.iter().filter(|k| k.action == crate::hotkeys::Action::ToggleScreenShare).find_map(|k| k.keys.as_ref()).filter(|keys| !self.keybind_errors.contains(keys));
        if let Some(keys) = keybind {
            return format!("Press {} in any app to start or stop", crate::hotkeys::display(keys));
        }
        match &self.call {
            Some(call) => format!("Everyone in {} can watch", call.channel.name),
            None => "Everyone in the voice channel can watch".into(),
        }
    }

    // .dialog.golive over the scrim: the head, tabs over a grid of live stills, quality, sound, and
    // Go live as the only primary.
    pub(crate) fn share_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.share_picker.as_ref()?;
        let channel = self.call.as_ref().map_or_else(|| "voice".to_owned(), |c| c.channel.name.clone());
        let head = div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(div().text_size(ui::TEXT_LG).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("Go live in {channel}")))
            .child(div().text_color(palette::muted(cx)).child("Choose a screen, a window or a capture device."));
        let body = ui::dialog_body()
            .child(head)
            .child(self.share_tabs(picker, window, cx))
            .child(self.share_sources(picker, cx))
            .child(self.share_quality(window, cx))
            .child(self.share_audio(window, cx))
            .child(self.share_footer(cx));
        let backdrop = div().id("share-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
            this.close_share_picker(cx);
            cx.notify();
        }));
        let dialog = ui::dialog("share-picker", DIALOG_WIDTH, body, window, cx);
        Some(ui::modal(backdrop, dialog, cx))
    }

    fn share_tabs(&self, picker: &SharePicker, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const TABS: [ShareTab; 3] = [ShareTab::Screens, ShareTab::Windows, ShareTab::Devices];
        let items = [ui::Tab::new("Screens").glyph(Glyph::Screen), ui::Tab::new("Windows").glyph(Glyph::AppWindow), ui::Tab::new("Devices").glyph(Glyph::Usb)];
        let selected = TABS.iter().position(|t| *t == picker.tab).unwrap_or_default();
        let this = cx.entity().downgrade();
        ui::tabs("share-tabs", &items, selected, move |i, _, cx| {
            this.update(cx, |this, cx| {
                if let Some(picker) = &mut this.share_picker {
                    picker.tab = TABS[i];
                }
                cx.notify();
            })
            .ok();
        }, window, cx)
    }

    // .srcs: three across, 12px apart; each a live still and its name.
    fn share_sources(&self, picker: &SharePicker, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = palette::muted(cx);
        let note = |text: SharedString, color: Hsla| div().py_8().flex().justify_center().text_color(color).child(text).into_any_element();
        let body: AnyElement = match picker.tab {
            ShareTab::Windows => match &picker.windows {
                None => note("Finding windows…".into(), muted),
                Some(Err(error)) => note(error.clone(), palette::danger_ink(cx)),
                Some(Ok(windows)) if windows.is_empty() => note("No windows to share".into(), muted),
                Some(Ok(windows)) => source_grid(windows.iter().map(|w| self.share_tile(picker, ShareSource::Window(w.id), w.title.clone(), cx)).collect()),
            },
            ShareTab::Screens => match &picker.screens {
                None => note("Finding screens…".into(), muted),
                Some(Err(error)) => note(error.clone(), palette::danger_ink(cx)),
                Some(Ok(screens)) => source_grid(screens.iter().map(|s| self.share_tile(picker, ShareSource::Screen(s.id), s.name.clone(), cx)).collect()),
            },
            // GAP: capture cards aren't supported by the voice engine yet.
            ShareTab::Devices => note("No capture devices found".into(), muted),
        };
        // Bled 4px past the column so a tile's hover fill and selection ring aren't clipped by the scroller.
        div().id("share-list").max_h(px(SOURCES_HEIGHT)).mx_neg_1().overflow_y_scroll().child(div().p_1().child(body))
    }

    // .src: a 16:10 still under a hairline; picked, it's ringed in accent and its name turns strong.
    fn share_tile(&self, picker: &SharePicker, source: ShareSource, name: String, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.share_choice == Some(source);
        let key = source_key(source);
        let label = name.clone();
        let (hover, muted) = (palette::hover(cx), palette::muted(cx));
        let still: AnyElement = match picker.thumb(source) {
            Some(image) => ui::media(image, ui::all(ui::RADIUS_MD)).size_full().object_fit(gpui_kit::ObjectFit::Contain).into_any_element(),
            None => Icon::new(glyph(source)).size_8().text_color(muted).into_any_element(),
        };
        let shot = div()
            .relative()
            .w_full()
            .aspect_ratio(16. / 10.)
            .squircle(ui::RADIUS_MD)
            .bg(palette::surface(palette::Level::Rail, cx))
            .border_1()
            .border_color(palette::line(cx))
            .flex()
            .items_center()
            .justify_center()
            .child(still)
            .when(selected, |d| d.child(selection_ring(&key, picker.picked, palette::surface(palette::Level::Raised, cx))));
        let icon: AnyElement = match picker.icon(source) {
            Some(image) => img(image).size_4().flex_none().into_any_element(),
            None => Icon::new(glyph(source)).size(ui::ICON_MD).flex_none().text_color(muted).into_any_element(),
        };
        div()
            .id(SharedString::from(format!("share-{key}")))
            .min_w_0()
            .p_1()
            .m_neg_1()
            .squircle(ui::RADIUS_LG)
            .flex()
            .flex_col()
            .gap_1p5()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(shot)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .min_w_0()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if selected { palette::strong(cx) } else { palette::text(cx) })
                    .child(icon)
                    .child(div().min_w_0().truncate().child(name)),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pick_share(source, label.clone(), cx)))
            .into_any_element()
    }

    // .quality: resolution and frame rate side by side, each a segmented control.
    fn share_quality(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const RESOLUTIONS: [(u32, &str); 4] = [(720, "720p"), (1080, "1080p"), (1440, "1440p"), (0, "Source")];
        const RATES: [(u32, &str); 3] = [(15, "15 fps"), (30, "30 fps"), (60, "60 fps")];
        let resolution = RESOLUTIONS.iter().position(|(v, _)| *v == self.prefs.stream_resolution).unwrap_or(1);
        let rate = RATES.iter().position(|(v, _)| *v == self.prefs.stream_fps).unwrap_or(1);
        let (this, that) = (cx.entity().downgrade(), cx.entity().downgrade());
        let resolutions = ui::segmented_live(
            "share-resolution",
            &RESOLUTIONS.map(|(_, l)| l),
            resolution,
            move |i, _, cx| {
                this.update(cx, |this, cx| this.update_prefs(cx, |p| p.stream_resolution = RESOLUTIONS[i].0)).ok();
            },
            window,
            cx,
        );
        let rates = ui::segmented_live(
            "share-rate",
            &RATES.map(|(_, l)| l),
            rate,
            move |i, _, cx| {
                that.update(cx, |this, cx| this.update_prefs(cx, |p| p.stream_fps = RATES[i].0)).ok();
            },
            window,
            cx,
        );
        let field = |label: &'static str, control: Div| div().flex_1().min_w_0().flex().flex_col().gap_1p5().child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(label)).child(control);
        div().flex().gap_4().child(field("Resolution", resolutions)).child(field("Frame rate", rates))
    }

    // The sound switch, saying whose sound viewers get.
    fn share_audio(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let what = match (self.share_choice, &self.share_choice_label) {
            (Some(ShareSource::Screen(_)) | None, _) => "Viewers hear everything except Waffle".to_owned(),
            (Some(_), Some(label)) => format!("Viewers hear {label}, not your other apps"),
            (Some(_), None) => "Viewers hear this app, not your other apps".to_owned(),
        };
        div()
            .id("stream-audio")
            .py_2p5()
            .flex()
            .items_center()
            .gap_3()
            .cursor_pointer()
            .child(div().flex_1().min_w_0().child(div().font_weight(FontWeight::NORMAL).text_color(palette::strong(cx)).child("Share audio")).child(div().truncate().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(what)))
            .child(ui::switch_live("stream-audio-switch", self.prefs.stream_audio, window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.update_prefs(cx, |p| p.stream_audio = !p.stream_audio)))
    }

    // The footer: who can watch (or the keybind), Cancel, and Go live.
    fn share_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.share_choice.is_some();
        let go = ui::button("go-live", ui::Tone::Primary, cx)
            .child(Icon::new(Glyph::StreamBold).size(ui::ICON_LG))
            .child("Go live")
            .when(!ready, |b| b.opacity(0.45).cursor(gpui_kit::CursorStyle::OperationNotAllowed))
            .when(ready, |b| b.on_click(cx.listener(|this, _, _, cx| this.go_live_from_picker(cx))));
        let cancel = ui::button("go-live-cancel", ui::Tone::Ghost, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| {
            this.close_share_picker(cx);
            cx.notify();
        }));
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().truncate().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(self.share_hint()))
            .child(cancel)
            .child(go)
    }
}

fn source_grid(tiles: Vec<AnyElement>) -> AnyElement {
    div().grid().grid_cols(3).gap_3().children(tiles).into_any_element()
}

fn source_key(source: ShareSource) -> String {
    match source {
        ShareSource::Screen(id) => format!("screen-{id}"),
        ShareSource::App(pid) => format!("app-{pid}"),
        ShareSource::Window(id) => format!("window-{id}"),
    }
}

fn glyph(source: ShareSource) -> Glyph {
    match source {
        ShareSource::Screen(_) => Glyph::Screen,
        ShareSource::App(_) | ShareSource::Window(_) => Glyph::AppWindow,
    }
}

// .src[aria-pressed] .shot: a 2px gap in the dialog's colour, then a 2px accent ring. A user's pick
// springs out from the still's edge, overshooting a touch; the preselection lands still.
fn selection_ring(key: &str, animate: bool, gap: Hsla) -> AnyElement {
    let ring = div().absolute().border_2().border_color(palette::accent()).child(div().absolute().inset_0().squircle(ui::RADIUS_MD + 2.).border_2().border_color(gap));
    if !animate {
        return place_ring(ring, 1., 1.).into_any_element();
    }
    ring.with_animation(SharedString::from(format!("ring-{key}")), Animation::new(ui::DUR_POP), |ring, t| place_ring(ring, crate::motion::back_out(t), (t / 0.4).min(1.))).into_any_element()
}

fn place_ring(ring: Div, grow: f32, alpha: f32) -> Div {
    let offset = 1. - 5. * grow;
    ring.top(px(offset)).left(px(offset)).right(px(offset)).bottom(px(offset)).squircle(ui::RADIUS_MD - offset).opacity(alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: u32, pid: i32, app: &str) -> ShareableWindow {
        ShareableWindow { id, title: format!("{app} {id}"), app_name: app.into(), pid }
    }

    fn screens() -> Vec<ShareableScreen> {
        vec![ShareableScreen { id: 7, name: "Screen 1".into(), width: 0, height: 0 }, ShareableScreen { id: 9, name: "Screen 2".into(), width: 0, height: 0 }]
    }

    #[test]
    fn remembered_window_falls_back_to_its_app() {
        let last = LastShare::Window { id: 10, pid: 100, app_name: "Figma".into() };
        let pick = |windows: &[ShareableWindow]| last.find(windows, &screens()).map(|(source, _)| source);
        assert_eq!(pick(&[window(11, 100, "Figma"), window(10, 100, "Figma")]), Some(ShareSource::Window(10)), "the same window first");
        assert_eq!(pick(&[window(20, 200, "Figma"), window(12, 100, "Figma")]), Some(ShareSource::Window(12)), "then the same process");
        assert_eq!(pick(&[window(1, 300, "Safari"), window(21, 200, "Figma")]), Some(ShareSource::Window(21)), "then the same app");
        assert_eq!(pick(&[window(10, 300, "Safari")]), None, "a reused id from another app isn't ours");
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn picker_preselects_the_remembered_source_or_screen_one() {
        let windows = [window(10, 100, "Figma")];
        let screen = LastShare::Screen { id: 9 };
        assert_eq!(preselect(Some(&screen), &windows, &screens()), Some((ShareSource::Screen(9), "Screen 2".into())));
        let unplugged = LastShare::Screen { id: 4 };
        assert_eq!(preselect(Some(&unplugged), &windows, &screens()), Some((ShareSource::Screen(7), "Screen 1".into())));
        let closed = LastShare::Window { id: 30, pid: 300, app_name: "Notes".into() };
        assert_eq!(preselect(Some(&closed), &windows, &screens()).map(|(s, _)| s), Some(ShareSource::Screen(7)));
        assert_eq!(preselect(None, &windows, &screens()).map(|(s, _)| s), Some(ShareSource::Screen(7)));
        assert_eq!(preselect(None, &windows, &[]), None);
        assert_eq!(LastShare::of(ShareSource::Window(10), &windows), Some(LastShare::Window { id: 10, pid: 100, app_name: "Figma".into() }));
    }

    #[test]
    fn keybind_stops_or_starts_in_a_call() {
        assert_eq!(keybind_move(false, false), KeybindMove::Nothing);
        assert_eq!(keybind_move(false, true), KeybindMove::Nothing);
        assert_eq!(keybind_move(true, true), KeybindMove::Stop);
        assert_eq!(keybind_move(true, false), KeybindMove::Start);
    }

    #[test]
    fn keybind_shares_the_game_else_the_screen() {
        let windows = [window(10, 100, "Figma"), window(20, 200, "Valorant")];
        let source = |game| keybind_source(game, &windows, &screens()).map(|(s, _)| s);
        let screen = screens()[0].id;
        assert_eq!(source(Some(200)), Some(ShareSource::Window(20)), "the game's window");
        assert_eq!(source(None), Some(ShareSource::Screen(screen)), "no game: the whole screen, not the app in front");
        assert_eq!(source(Some(1)), Some(ShareSource::Screen(screen)), "a game with no window on offer: the screen");
        assert_eq!(keybind_source(None, &windows, &[]).map(|(s, _)| s), None, "no screen at all: the picker");
    }
}
