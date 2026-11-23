// The boxes drawn around each typed option, behind the composer's text.

use super::*;

// OPTION BOXES

/// The transparent space an option's name chip leaves before itself, so neighbouring boxes don't touch.
pub(crate) const OPTION_GAP: f32 = 8.;
// How far a box runs past its value's last glyph: the start of its end chip.
pub(super) const VALUE_PAD: f32 = 5.;
/// An end chip's width: the box's padding after its value, then a gap before what follows it.
pub(crate) const END_CHIP: f32 = VALUE_PAD + 3.;
pub(super) const BOX_RADIUS: f32 = 6.;
pub(super) const PLACEHOLDER_SIZE: f32 = 13.;

// One option drawn as a box: its name chip, then its value, as byte offsets into the composer's text.
pub(super) struct OptionBox {
    pub(super) start: usize,
    pub(super) key_end: usize,
    pub(super) value_end: usize,
    pub(super) active: bool,
    pub(super) invalid: bool,
    // Drawn in an empty value: an attachment's "Please attach a file".
    pub(super) placeholder: Option<SharedString>,
}

impl Waffle {
    /// Each typed option's box, drawn over the composer's text after it lays out: the text field can't
    /// border a text range itself, so this reads where its range landed and strokes around it. Discord's
    /// "+N optional" trails the last box.
    pub(crate) fn option_boxes(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        let typed = self.typed_command(cx)?;
        let active = typed.line.arg_at(typed.caret);
        let errored = &self.command_picker.errored;
        let boxes: Vec<OptionBox> = typed
            .line
            .args
            .iter()
            .enumerate()
            .map(|(n, arg)| {
                let option = typed.option(arg);
                let value = self.arg_value(&typed, arg);
                // Discord shows an error only on a box that isn't selected.
                let invalid = active != Some(n) && match command_line::resolve(option, &value) {
                    Err(_) => true,
                    Ok(None) => errored.contains(&arg.option),
                    Ok(Some(_)) => false,
                };
                let placeholder = (option.kind == OptionKind::Attachment && arg.value.is_empty()).then(|| SharedString::from(ATTACH_FILE));
                OptionBox { start: arg.key.start, key_end: arg.key.end, value_end: arg.inner().end, active: active == Some(n), invalid, placeholder }
            })
            .collect();
        let unset = typed.command().options.iter().enumerate().filter(|(i, o)| !o.required && !typed.line.has(*i)).count();
        let trailing = (unset > 0 && !typed.line.args.is_empty()).then(|| (typed.text().len(), SharedString::from(format!("+{unset} optional"))));
        let composer = self.composer.clone();
        let (line, accent, ring, danger, faint) = (palette::line_strong(cx), palette::accent_ink(cx), palette::accent_soft(cx), palette::danger_ink(cx), palette::faint(cx));
        let paint = move |bounds: gpui_kit::Bounds<gpui_kit::Pixels>, _: (), window: &mut Window, cx: &mut App| {
            // Read every position first: painting text needs `cx` back.
            let state = composer.read(cx);
            let at = |offset: usize| state.range_to_bounds(&(offset..offset));
            let placed: Vec<_> = boxes.iter().filter_map(|b| Some((b, at(b.start)?, at(b.key_end)?, at(b.value_end)?))).collect();
            let trailing = trailing.as_ref().and_then(|(offset, text)| Some((at(*offset)?, text)));
            let shaped = |text: &SharedString, window: &mut Window| {
                let run = gpui_kit::TextRun { color: faint, ..window.text_style().to_run(text.len()) };
                window.text_system().shape_line(text.clone(), px(PLACEHOLDER_SIZE), &[run], None)
            };
            window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                for (b, start, key_end, end) in placed {
                    let placeholder = b.placeholder.as_ref().map(|p| shaped(p, window));
                    // An empty value's room is the key chip's own padding and this one.
                    let right = match &placeholder {
                        Some(p) => key_end.left() + p.width + px(VALUE_PAD + 2.),
                        None => end.left() + px(VALUE_PAD),
                    };
                    let edge = if b.invalid { danger } else if b.active { accent } else { line };
                    for rect in box_rows(start, right, end, bounds) {
                        if b.active {
                            window.paint_quad(gpui_kit::quad(rect.dilate(px(FOCUS_RING)), gpui_kit::Corners::all(px(BOX_RADIUS + FOCUS_RING)), gpui_kit::transparent_black(), px(FOCUS_RING), ring, gpui_kit::BorderStyle::Solid));
                        }
                        window.paint_quad(gpui_kit::quad(rect, gpui_kit::Corners::all(px(BOX_RADIUS)), gpui_kit::transparent_black(), ui::HAIRLINE, edge, gpui_kit::BorderStyle::Solid));
                    }
                    if let Some(p) = placeholder {
                        p.paint(point(key_end.left() + px(PLACEHOLDER_NUDGE), key_end.top()), key_end.size.height, gpui_kit::TextAlign::Left, None, window, cx).ok();
                    }
                }
                if let Some((end, text)) = trailing {
                    shaped(text, window).paint(point(end.left() + px(OPTION_GAP), end.top()), end.size.height, gpui_kit::TextAlign::Left, None, window, cx).ok();
                }
            });
        };
        Some(gpui_kit::canvas(|_, _, _| {}, paint).absolute().inset_0())
    }
}

// An option's box, one rectangle per line it wraps across, a pixel outside its line so the border clears
// the name chip, which starts `OPTION_GAP` into its token.
pub(super) fn box_rows(start: gpui_kit::Bounds<gpui_kit::Pixels>, right: gpui_kit::Pixels, end: gpui_kit::Bounds<gpui_kit::Pixels>, clip: gpui_kit::Bounds<gpui_kit::Pixels>) -> Vec<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let row = |left: gpui_kit::Pixels, right: gpui_kit::Pixels, top: gpui_kit::Pixels, height: gpui_kit::Pixels| {
        gpui_kit::Bounds::from_corners(point(left - ui::HAIRLINE, top - ui::HAIRLINE), point(right, top + height + ui::HAIRLINE))
    };
    let left = start.left() + px(OPTION_GAP);
    let height = start.size.height;
    if (end.top() - start.top()).abs() < height / 2. {
        return vec![row(left, right, start.top(), height)];
    }
    let mut rows = vec![row(left, clip.right(), start.top(), height)];
    let mut top = start.top() + height;
    while top + height / 2. < end.top() {
        rows.push(row(clip.left(), clip.right(), top, height));
        top += height;
    }
    rows.push(row(clip.left(), right, end.top(), height));
    rows
}
