// Finishing what the composer's typed query started: a mention chip or a text completion swapped in for the
// query that ends the draft, and the draft rewritten the way Discord takes it.
use gpui_kit::{Context, Window};

use crate::{ComposerState, InlineToken, InputContent, command_picker};

// Swaps the `@`/`#` query that ends the draft for a mention chip, then a space, leaving the caret after both.
pub(crate) fn complete_mention(state: &mut ComposerState, query: &str, chip: InlineToken, window: &mut Window, cx: &mut Context<ComposerState>) {
    let end = state.value().len();
    if !state.value().ends_with(query) {
        return;
    }
    state.set_selected_range(end - query.len()..end, cx);
    if state.replace_with_token(chip, window, cx).is_ok() {
        state.insert(" ", window, cx);
    }
}

// Replaces the `query` that ends the draft with `text` and a space.
pub(crate) fn complete_text(state: &mut ComposerState, query: &str, text: &str, window: &mut Window, cx: &mut Context<ComposerState>) {
    let end = state.value().len();
    if !state.value().ends_with(query) {
        return;
    }
    state.set_selected_range(end - query.len()..end, cx);
    // `replace` swaps out the selection; `insert` would add at the caret and leave the typed code behind.
    state.replace(format!("{text} "), window, cx);
}

// The draft as Discord takes it: each mention chip becomes its `<@id>` / `<#id>`, kept as the token's id.
// A slash command's end chip is the space it shows.
pub(crate) fn wire_text(content: &InputContent) -> String {
    let mut text = content.text().to_string();
    for span in content.tokens().iter().rev() {
        let token = span.token();
        let wire = if command_picker::is_end_token(token) { token.text() } else { token.id() };
        text.replace_range(span.range(), wire);
    }
    text
}
