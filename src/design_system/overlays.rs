// Overlays: Tooltip, Menu, Popover, Hover card, Toast, Callout, Strip, Empty state. Dialog is in dialogs.rs.
use super::*;

fn variant(title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (Kind::Variants, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

pub(super) fn tooltip_frames(cx: &App) -> Vec<Frame> {
    let button = |id: &'static str, text: &'static str, _cx: &App| ui::Button::new(id).variant(ui::ButtonVariant::Secondary).child("Hover me").tooltip(move |window, cx| ui::Tooltip::new(text).build(window, cx));
    let content = row().gap_4().child(button("tt-short", "Mute", cx)).child(button("tt-long", "Turn off your microphone so nobody in Lounge can hear you", cx)).child(ui::Button::new("tt-icon").variant(ui::ButtonVariant::Ghost).size(ui::ButtonSize::IconSm).icon(Glyph::Settings).tooltip_text("User settings"));
    vec![variant("Tooltip / Content", "A one-line label in the design's look. Every icon button carries one; text that was cut off shows its full value in one.", content, cx)]
}

pub(super) fn menu_frames(cx: &App) -> Vec<Frame> {
    let lead = |g: Glyph| div().text_color(palette::muted(cx)).child(glyph(g));
    let menu = ui::PopoverContent::new(cx)
        .w(px(MENU_WIDTH))
        .child(ui::DropdownMenuItem::new("m-reply", cx).child(lead(Glyph::Reply)).child(div().flex_1().child("Reply")))
        .child(ui::DropdownMenuItem::new("m-react", cx).child(lead(Glyph::EmojiAdd)).child(div().flex_1().child("Add reaction")))
        .child(ui::DropdownMenuItem::new("m-copy", cx).child(lead(Glyph::Copy)).child(div().flex_1().child("Copy text")))
        .child(ui::DropdownMenuItem::new("m-mark", cx).child(div().flex_1().child("Mark as unread")).child(ui::Checkbox::mark(Check::On).size(ui::ICON_LG)))
        .child(ui::DropdownMenuItem::new("m-mode", cx).child(div().flex_1().child("Push to talk")).child(ui::RadioGroupItem::mark(false).scoped(18., palette::line_strong(cx))))
        .child(div().px_2().py_1().child(ui::Separator::new().w_full()))
        .child(ui::DropdownMenuItem::new("m-delete", cx).danger(cx).child(glyph(Glyph::Trash)).child("Delete message"));
    vec![variant("Menu / Row", "No fill until hovered, then the hover fill and the strong ink fade in. The danger row sits last, past a divider.", ui::col().child(menu), cx)]
}

pub(super) fn popover_frames(cx: &App) -> Vec<Frame> {
    let content = row()
        .gap_6()
        .items_start()
        .child(ui::PopoverContent::new(cx).w(px(MENU_WIDTH)).p_3().child("A surface: ringed and shadowed, content inset."))
        .child(ui::panel(cx).w_64().p_3().child("A static panel, no pointer"))
        .child(div().w_80().child(ui::Popover::bar("po-bar", cx).child(div().flex_1().child("Unsaved changes")).child(ui::Button::new("po-save").size(ui::ButtonSize::Sm).child("Save"))));
    vec![variant("Popover / Kind", "What floats takes a long soft drop and a hairline ring in place of a border; it occludes, so nothing behind it responds.", content, cx)]
}

pub(super) fn hover_card_frames(window: &mut Window, cx: &mut App) -> Vec<Frame> {
    let people = vec![ui::Entry::person("", "Mira"), ui::Entry::person("", "Theo"), ui::Entry::person("", "Ana")];
    let list = ui::HoverCard::new(div().id("hc-list").child(ui::AvatarGroup::stacked([ui::AvatarFallback::new("Mira", 24.).into_any_element(), ui::AvatarFallback::new("Theo", 24.).into_any_element()], 24., palette::surface(Level::Sidebar, cx))), "Watching", people);
    let roll = |id: &'static str, rest: &'static str, rolled: &'static str, window: &mut Window, cx: &mut App| {
        let (hovered, on_hover) = ui::hover_state(id, window, cx);
        let swap = ui::hover_roll(id, ui::line(rest), ui::line(rolled), hovered, ui::LINE_XS, window, cx);
        div().id(id).w_40().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).on_hover(on_hover).child(swap)
    };
    let content = row().gap_8().child(list).child(roll("ds-roll-status", "Online", "frsty", window, cx)).child(roll("ds-roll-voice", "Voice Connected", "Voice Details", window, cx));
    vec![variant("Hover card / Kind", "A list that opens on hover and stays up while the pointer is on it; and a line that rolls up and away as another rises in over 220ms. Hover each.", content, cx)]
}

pub(super) fn toast_frames(cx: &App) -> Vec<Frame> {
    let content = ui::col()
        .gap_3()
        .child(ui::Sonner::toast("toast", Glyph::Checkmark, palette::positive_ink(cx), "Invite link copied", None, cx))
        .child(ui::Sonner::toast("toast-detail", Glyph::Checkmark, palette::positive_ink(cx), "Invite link copied", Some("It expires in 7 days.".into()), cx))
        .child(ui::Sonner::toast_with("toast-action", Glyph::Checkmark, palette::positive_ink(cx), "Left Night Shift", None, Some(ui::Button::new("toast-undo").variant(ui::ButtonVariant::Link).size(ui::ButtonSize::Sm).child("Undo").into_any_element()), cx))
        .child(ui::Sonner::text("ds-toast-text", "Copied link", cx));
    vec![variant("Toast / Kind", "One line, rising in over 240ms from 8px lower; an action only when the thing can be undone.", content, cx)]
}

pub(super) fn callout_frames(cx: &App) -> Vec<Frame> {
    let content = ui::col()
        .gap_2()
        .child(ui::Alert::new(Notice::Info, Glyph::InfoFill, "Slowmode is on", "You can send one message every 10 seconds.", cx))
        .child(ui::Alert::new(Notice::Warning, Glyph::Warning, "Your connection is unstable", "Others may hear you cut out. Try a wired connection.", cx).child(ui::Button::new("cl-test").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).self_center().child("Run test")))
        .child(ui::Alert::new(Notice::Danger, Glyph::XCircle, "Couldn't send 2 messages", "Waffle lost the connection. They're kept below.", cx).child(ui::Button::new("cl-retry").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).self_center().child("Retry all")))
        .child(ui::Alert::new(Notice::Success, Glyph::CheckCircle, "Two-factor is on", "Keep your backup codes somewhere safe.", cx))
        .child(ui::Alert::new(Notice::Accent, Glyph::AtFill, "3 mentions while you were away", "The oldest is in #looking-for-group.", cx).child(ui::Button::new("cl-jump").size(ui::ButtonSize::Sm).self_center().child("Jump to first")));
    vec![variant("Callout / Notice", "Sits inside the view it's about; actions go under the text. Status is never colour alone: each has its own icon.", content, cx)]
}

pub(super) fn strip_frames(cx: &App) -> Vec<Frame> {
    let strip = |fill: ui::StripFill, g: Glyph, text: &'static str| ui::Alert::strip(fill, cx).child(Icon::new(g).size(ui::ICON_LG).text_color(palette::muted(cx))).child(div().flex_1().child(text));
    let content = ui::col()
        .gap_2()
        .child(strip(ui::StripFill::Raised, Glyph::Megaphone, "Follow to get this channel's updates.").child(ui::Button::new("ds-strip-follow").size(ui::ButtonSize::Sm).child("Follow")))
        .child(strip(ui::StripFill::Selected, Glyph::Megaphone, "Share with your followers!"))
        .child(strip(ui::StripFill::Accent, Glyph::CheckCircle, "Next: Say hi").child(ui::Button::new("ds-strip-go").size(ui::ButtonSize::Sm).child("Go")));
    vec![variant("Strip / Fill", "A compact notice of one line with its action at the end.", content, cx)]
}

pub(super) fn empty_state_frames(cx: &App) -> Vec<Frame> {
    let card = |el: Div| div().squircle(ui::RADIUS_LG).bg(palette::surface(Level::Page, cx)).child(el);
    let moods = div()
        .grid()
        .grid_cols(3)
        .gap_4()
        .child(card(ui::Empty::new("empty-idle", Mood::Idle, "No one's around", "None of your friends are online. Invite someone to play.", cx).child(div().pt_2().child(ui::Button::new("e-add").child("Add friend")))))
        .child(card(ui::Empty::new("empty-wait", Mood::Waiting, "Loading messages…", "Fetching the last 50 messages in #general.", cx)));
    let art = row().gap_4().children(crate::illustration::Illustration::ALL.map(|art| card(div().child(gpui_kit::img(art.path()).w(px(ART_SIZE.0)).h(px(ART_SIZE.1))))));
    vec![variant("Empty state / Art", "Orient, then one action. The mascot's mood for small spaces, an illustration where Discord leads with one.", ui::col().gap_6().child(moods).child(art), cx)]
}
