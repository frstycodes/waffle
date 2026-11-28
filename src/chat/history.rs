//! Older history while it loads: placeholder rows above the oldest loaded message that the reader
//! can keep scrolling into, and where the reader lands once the real messages replace them.

use super::*;

// How many placeholder rows sit above the oldest message while there's more history, and each one's
// height. Fixed, so the block's height is known when older messages replace it.
const HISTORY_ROWS: usize = 12;
const HISTORY_ROW: f32 = 64.;
// The placeholder lines' widths, cycled down the rows so they read like a conversation.
const HISTORY_WIDTHS: [f32; 5] = [360., 220., 420., 280., 180.];
// Older history starts loading once the reader is within this many rows of the oldest loaded message,
// so the next page is usually in before the placeholders reach the screen.
pub(crate) const PREFETCH_ROWS: usize = 15;

fn history_height() -> f32 {
    HISTORY_ROWS as f32 * HISTORY_ROW
}

// Rows shaped like messages above the oldest one, while older history loads.
pub(crate) fn history_skeleton(_cx: &App) -> AnyElement {
    let row = |i: usize| {
        let width = HISTORY_WIDTHS[i % HISTORY_WIDTHS.len()];
        ui::row()
            .h(px(HISTORY_ROW))
            .flex_none()
            .items_start()
            .gap_4()
            .pt_2()
            .child(ui::Skeleton::circle(("history-skeleton-avatar", i), AVATAR))
            .child(
                ui::col()
                    .flex_1()
                    .gap_2()
                    .pt_1()
                    .child(ui::Skeleton::new(("history-skeleton-name", i)).radius(ui::RADIUS_SM).w(gpui_kit::relative(0.2)).h_3())
                    .child(ui::Skeleton::new(("history-skeleton-line", i)).radius(ui::RADIUS_SM).w(px(width)).max_w_full().h_3()),
            )
    };
    // lint-allow(unvirtualized-list): a fixed twelve placeholder rows inside one list item
    ui::col().w_full().px_4().children((0..HISTORY_ROWS).map(row)).into_any_element()
}

// Where the reader lands after `added` older messages go in above the oldest loaded one, given where
// they were (`top`, with the placeholders still drawn atop item 0). Above item 0 nothing moves but
// the index. Inside the old first message, the placeholders above it are gone. Inside the
// placeholders, the reader lands as many messages above the old first one as there were rows
// between them and it, so scrolling carries on through the real history rather than jumping back.
pub(crate) fn anchor_after_older(top: ListOffset, added: usize) -> ListOffset {
    if top.item_ix > 0 {
        return ListOffset { item_ix: top.item_ix + added, offset_in_item: top.offset_in_item };
    }
    let into_first = top.offset_in_item - px(history_height());
    if into_first >= Pixels::ZERO {
        return ListOffset { item_ix: added, offset_in_item: into_first };
    }
    let rows_above = (-into_first / px(HISTORY_ROW)).ceil() as usize;
    ListOffset { item_ix: added.saturating_sub(rows_above), offset_in_item: Pixels::ZERO }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(item_ix: usize, offset: f32) -> ListOffset {
        ListOffset { item_ix, offset_in_item: px(offset) }
    }

    #[test]
    fn reading_below_the_first_message_keeps_its_place() {
        assert_eq!(anchor_after_older(at(7, 12.), 50), at(57, 12.));
    }

    #[test]
    fn reading_the_first_message_drops_the_placeholders_above_it() {
        assert_eq!(anchor_after_older(at(0, history_height() + 30.), 50), at(50, 30.));
    }

    #[test]
    fn reading_the_placeholders_lands_in_the_new_history() {
        // Three rows above the old first message: three messages above it.
        assert_eq!(anchor_after_older(at(0, history_height() - 3. * HISTORY_ROW), 50), at(47, 0.));
        // At the very top of the placeholders, a short page: its oldest message.
        assert_eq!(anchor_after_older(at(0, 0.), 5), at(0, 0.));
    }
}
