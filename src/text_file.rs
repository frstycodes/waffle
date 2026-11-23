// Text attachments (.txt, .md, .log, code, and the message.txt Discord makes of a long message) read in
// place. A card fetches its file when it first renders, which the chat list does only for rows in view.
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, MouseButton, Overflow, SharedString, Stateful, StyleRefinement, WeakEntity, Window,
    component::text::{TextView, TextViewStyle},
    div, linear_color_stop, linear_gradient, prelude::*, px, relative,
};

use crate::{
    Waffle,
    discord::{File, runtime},
    icons::Glyph,
    palette::{self, Level},
    part::Column,
    ui::{self, RADIUS_LG, RADIUS_MD, Squircle},
};

// What a preview reads of the file; the footer says so when the file is longer.
const PREVIEW_BYTES: usize = 100 << 10;
// Copy contents fetches at most this much: past it the clipboard gets the start and a toast says so.
const COPY_BYTES: usize = 16 << 20;
// Collapsed clips here under a fade and never scrolls; expanded scrolls inside the taller cap.
const COLLAPSED: f32 = 200.;
const EXPANDED: f32 = 560.;
const FADE: f32 = 88.;
// The widest an inline card gets.
const INLINE_MAX: f32 = 640.;

#[derive(Clone)]
pub enum TextFile {
    Loading,
    Ready { text: SharedString, truncated: bool },
    // Binary, not UTF-8, or the fetch failed: it shows as a plain file card.
    Unreadable,
}

#[derive(Default)]
pub struct TextFiles {
    pub files: HashMap<String, TextFile>,
    pub expanded: HashSet<String>,
    // The file open full size over the window.
    pub viewer: Option<File>,
}

pub fn is_text_name(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else { return false };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "jsonc" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "env" | "xml" | "html" | "htm" | "css" | "scss"
            | "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "rs" | "py" | "rb" | "go" | "java" | "kt" | "kts" | "swift" | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "php"
            | "lua" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "sql" | "diff" | "patch" | "gradle" | "vue" | "svelte" | "dart" | "zig" | "nix" | "ex" | "exs"
            | "hs" | "ml" | "r" | "pl" | "tex" | "srt" | "vtt"
    )
}

// None for anything that isn't UTF-8 text. `truncated` bytes stop at a cap, which can cut the last character in half.
pub fn decode(bytes: &[u8], truncated: bool) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) if truncated && e.error_len().is_none() => std::str::from_utf8(&bytes[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    Some(text.trim_start_matches('\u{feff}').replace("\r\n", "\n"))
}

// The first `cap` bytes of the file, and whether there was more.
async fn fetch(url: String, cap: usize) -> Result<(Vec<u8>, bool), String> {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    let mut res = HTTP.get_or_init(reqwest::Client::new).get(&url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > cap {
            bytes.truncate(cap);
            return Ok((bytes, true));
        }
    }
    Ok((bytes, false))
}

// The file as one fenced code block, so TextView gives it the mono face, wrapping, selection and its own
// scroll. The fence is longer than any run of backticks inside, so nothing in the file can close it.
fn fenced(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}\n{}\n{fence}", text.trim_end_matches('\n'))
}

impl Waffle {
    pub(crate) fn load_text_file(&mut self, url: String, cx: &mut Context<Self>) {
        if self.text_files.borrow().files.contains_key(&url) {
            return;
        }
        self.text_files.borrow_mut().files.insert(url.clone(), TextFile::Loading);
        let task = runtime().spawn(fetch(url.clone(), PREVIEW_BYTES));
        cx.spawn(async move |this, cx| {
            let state = match task.await {
                Ok(Ok((bytes, truncated))) => decode(&bytes, truncated).map_or(TextFile::Unreadable, |text| TextFile::Ready { text: text.into(), truncated }),
                _ => TextFile::Unreadable,
            };
            this.update(cx, |this, cx| {
                this.text_files.borrow_mut().files.insert(url, state);
                // The card is drawn in the feed and in the panel's search hits.
                this.redraw(Column::Main, cx);
                this.redraw(Column::Panel, cx);
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the fetch finishes, or at once if the app is gone
        .detach();
    }

    // The whole file, fetched again when the preview stopped short.
    // GAP: no progress shown while a long file downloads for copying.
    fn copy_text_file(&mut self, url: String, cx: &mut Context<Self>) {
        let whole = match self.text_files.borrow().files.get(&url) {
            Some(TextFile::Ready { text, truncated: false }) => Some(text.to_string()),
            _ => None,
        };
        if let Some(text) = whole {
            return self.copy_code(text, url, cx);
        }
        let task = runtime().spawn(fetch(url.clone(), COPY_BYTES));
        cx.spawn(async move |this, cx| {
            let fetched = task.await.ok().and_then(Result::ok).and_then(|(bytes, truncated)| Some((decode(&bytes, truncated)?, truncated)));
            this.update(cx, |this, cx| {
                let Some((text, truncated)) = fetched else {
                    return this.show_toast("Couldn't copy the file. Check your connection and try again.", cx);
                };
                if truncated {
                    this.show_toast("Copied the first 16 MB. Download the file for the rest.", cx);
                }
                this.copy_code(text, url, cx);
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the fetch finishes, or at once if the app is gone
        .detach();
    }

    // The preview over the whole window at its full height; Esc or a click outside closes it.
    pub(crate) fn text_viewer_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let file = self.text_files.borrow().viewer.clone()?;
        let TextFile::Ready { text, truncated } = self.text_files.borrow().files.get(&file.url)?.clone() else { return None };
        let viewport = window.viewport_size();
        let width = (f32::from(viewport.width) - 160.).clamp(320., 960.);
        let max_h = f32::from(viewport.height) - 208.;
        let copied = self.copied.as_deref() == Some(file.url.as_str());
        let card = card(Card { id: "text-viewer".into(), file: &file, text: Some((&text, truncated)), max_h: Some(max_h), expanded: None, copied, full: true, this: cx.weak_entity() }, cx)
            .w(px(width))
            .shadow(ui::shadow_pop(cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        // The card keeps the 8px of room above it that inline cards have.
        Some(ui::modal(crate::overlays::backdrop("text-viewer-backdrop", cx), div().pt_2().child(card), cx))
    }
}

pub fn inline(id: String, file: &File, state: Option<&TextFile>, expanded: bool, copied: bool, this: WeakEntity<Waffle>, cx: &App) -> AnyElement {
    let text = match state {
        Some(TextFile::Ready { text, truncated }) => Some((text, *truncated)),
        _ => None,
    };
    // Roughly what fits in the collapsed height; anything shorter has nothing to expand.
    let long = text.is_some_and(|(t, _)| t.lines().count() > 7 || t.len() > 600);
    let card = card(Card { id: id.into(), file, text, max_h: long.then_some(if expanded { EXPANDED } else { COLLAPSED }), expanded: long.then_some(expanded), copied, full: false, this }, cx);
    div().pt_2().child(card).into_any_element()
}

struct Card<'a> {
    id: SharedString,
    file: &'a File,
    // None while loading.
    text: Option<(&'a SharedString, bool)>,
    // None: short enough to show whole.
    max_h: Option<f32>,
    // Some(open) when the card can expand: inline, and long enough to need it.
    expanded: Option<bool>,
    copied: bool,
    full: bool,
    this: WeakEntity<Waffle>,
}

// The file's text over one hairline and a footer: expand, name, size, then Copy contents, Open full size, Download.
fn card(card: Card, cx: &App) -> Stateful<Div> {
    let Card { id, file, text, max_h, expanded, copied, full, this } = card;
    let line = palette::line(cx);
    // Two tones: the frame and footer in the file card's tone, the text in an inset well of its own.
    let (frame, well) = (palette::surface(Level::Sidebar, cx), palette::surface(Level::Raised, cx));
    let clipped = expanded == Some(false);
    let body = match text {
        Some((text, _)) => {
            let mut block = StyleRefinement::default().p_3().pl_3p5().border_0().bg(gpui_kit::transparent_black()).text_size(ui::TEXT_SM).line_height(relative(1.5)).text_color(palette::text(cx));
            if let Some(max_h) = max_h {
                block = block.max_h(px(max_h));
                // TextView scrolls the block inside its own height only when asked this way, and keeps the wheel from the chat.
                block.overflow.y = Some(if clipped { Overflow::Hidden } else { Overflow::Scroll });
            }
            div()
                .relative()
                .size_full()
                .child(TextView::markdown(SharedString::from(format!("{id}-text")), fenced(text)).selectable(true).style(TextViewStyle::default().code_block(block)))
                // lint-allow(custom-mark): ui::more_below follows a scroll handle, and this preview never scrolls while clipped; its fade is always on
                .when(clipped, |d| d.child(div().absolute().left_0().right_0().bottom_0().h(px(FADE)).squircle(RADIUS_MD).bg(linear_gradient(180., linear_color_stop(well.opacity(0.), 0.), linear_color_stop(well, 1.)))))
                .into_any_element()
        }
        None => div().h_16().into_any_element(),
    };
    // Inset 6px inside the frame's 20px corner, so 14px keeps the corners concentric.
    let body = div().px_1p5().pt_1p5().child(div().squircle(RADIUS_MD).overflow_hidden().bg(well).child(body));
    let size = match text.is_some_and(|(_, truncated)| truncated) {
        true => format!("{} · showing the first 100 KB", crate::chat::format_size(file.size)),
        false => crate::chat::format_size(file.size),
    };
    let (open, download, copy_url, viewer_file) = (file.url.clone(), file.url.clone(), file.url.clone(), file.clone());
    let (expand_this, copy_this, open_this, close_this) = (this.clone(), this.clone(), this.clone(), this);
    let button = |name: &str, glyph: Glyph, tip: &'static str| ui::icon_button(SharedString::from(format!("{id}-{name}")), glyph, tip, cx).small();
    ui::col()
        .id(id.clone())
        .w_full()
        .when(!full, |d| d.max_w(px(INLINE_MAX)))
        .squircle(RADIUS_LG)
        .overflow_hidden()
        .bg(frame)
        .border_1()
        .border_color(line)
        .child(body)
        .child(
            ui::row()
                .gap_2()
                .px_2()
                .py_1p5()
                .children(expanded.map(|open| {
                    let url = file.url.clone();
                    button("expand", if open { Glyph::ChevronUp } else { Glyph::ChevronDown }, if open { "Collapse" } else { "Expand" }).on_click(move |_, _, cx| { cx.stop_propagation();
                        expand_this
                            .update(cx, |this, cx| {
                                let mut files = this.text_files.borrow_mut();
                                if !files.expanded.remove(&url) {
                                    files.expanded.insert(url.clone());
                                }
                                drop(files);
                                cx.notify();
                            })
                            .ok();
                    })
                }))
                .child(
                    ui::line(file.name.clone())
                        .id(SharedString::from(format!("{id}-name")))
                        .cursor_pointer()
                        .text_size(ui::TEXT_SM)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::strong(cx))
                        .hover(|s| s.underline())
                        .on_click(move |_, _, cx| { cx.stop_propagation(); cx.open_url(&open) }),
                )
                .child(ui::line(size).flex_1().text_size(ui::TEXT_XS).text_color(palette::faint(cx)))
                .when(text.is_some(), |d| {
                    d.child(
                        button("copy", if copied { Glyph::Checkmark } else { Glyph::Copy }, if copied { "Copied" } else { "Copy contents" })
                            .when(copied, |b| b.text_color(palette::positive_ink(cx)))
                            .on_click(move |_, _, cx| { cx.stop_propagation();
                                copy_this.update(cx, |this, cx| this.copy_text_file(copy_url.clone(), cx)).ok();
                            }),
                    )
                })
                .when(!full && text.is_some(), |d| {
                    d.child(button("open", Glyph::Fullscreen, "Open full size").on_click(move |_, _, cx| { cx.stop_propagation();
                        open_this
                            .update(cx, |this, cx| {
                                this.text_files.borrow_mut().viewer = Some(viewer_file.clone());
                                cx.notify();
                            })
                            .ok();
                    }))
                })
                .child(button("download", Glyph::Download, "Download").on_click(move |_, _, cx| { cx.stop_propagation(); cx.open_url(&download) }))
                .when(full, |d| {
                    d.child(button("close", Glyph::X, "Close").on_click(move |_, _, cx| { cx.stop_propagation();
                        close_this
                            .update(cx, |this, cx| {
                                this.text_files.borrow_mut().viewer = None;
                                cx.notify();
                            })
                            .ok();
                    }))
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_names() {
        assert!(is_text_name("message.txt"));
        assert!(is_text_name("Main.RS"));
        assert!(!is_text_name("clip.mp4"));
        assert!(!is_text_name("Makefile"), "no extension stays a file card");
    }

    #[test]
    fn decodes_text_and_refuses_binary() {
        assert_eq!(decode(b"\xef\xbb\xbfhi\r\nthere", false).as_deref(), Some("hi\nthere"), "BOM and CRLF dropped");
        assert_eq!(decode(b"PK\x03\x04\x00\x00", false), None, "a NUL means binary");
        assert_eq!(decode(b"caf\xe9", false), None, "Latin-1 isn't UTF-8");
    }

    #[test]
    fn a_cap_through_a_character_keeps_what_came_before() {
        let bytes = "naïve".as_bytes();
        assert_eq!(decode(&bytes[..3], true).as_deref(), Some("na"), "ï cut in half at the cap");
        assert_eq!(decode(&bytes[..3], false), None, "a whole file ending mid-character is broken");
    }

    #[test]
    fn fence_outlasts_backticks_inside() {
        assert_eq!(fenced("a"), "```\na\n```");
        assert_eq!(fenced("x ```` y"), "`````\nx ```` y\n`````");
    }
}
