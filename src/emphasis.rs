// Text with some runs emphasised: names in a typing line, the person a reply goes to.
use gpui_kit::{FontWeight, HighlightStyle, Hsla, StyledText};

// One line of text with some runs in strong semibold, as the design's <b>.
pub(crate) fn emphasised(parts: &[(String, bool)], strong: Hsla) -> StyledText {
    let mut text = String::new();
    let mut runs = Vec::new();
    for (part, bold) in parts {
        let start = text.len();
        text.push_str(part);
        if *bold {
            runs.push((start..text.len(), HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::MEDIUM), ..Default::default() }));
        }
    }
    StyledText::new(text).with_highlights(runs)
}
