// Picture-in-picture: the call's floating player, as each kind of stream and camera shows it.
use super::*;
use crate::call::Viewer;

const NAMES: [&str; 7] = ["Wren", "toma reed", "moss lantern", "kiwi", "ferris", "juniper", "pip"];

fn frame(kind: Kind, title: &'static str, lede_text: &'static str, content: Div, cx: &App) -> Frame {
    (kind, section(title, lede_text, cx).child(board(cx).child(content)).into_any_element())
}

pub(super) fn pip_frames(window: &mut Window, cx: &mut App) -> Vec<Frame> {
    let owners = row()
        .gap_6()
        .flex_wrap()
        .child(captioned(ui::pip_at("ds-pip-own", picture(None), own(3, "Lounge", "kiwi"), 1., cx), "Your stream", cx))
        .child(captioned(ui::pip_at("ds-pip-theirs", picture(None), theirs("Lounge"), 1., cx), "Someone's stream or camera", cx));
    let chrome = row()
        .gap_6()
        .flex_wrap()
        .child(captioned(ui::pip_at("ds-pip-hidden", picture(None), own(3, "Lounge", "kiwi"), 0., cx), "Hidden · at rest", cx))
        .child(captioned(ui::pip_at("ds-pip-easing", picture(None), own(3, "Lounge", "kiwi"), 0.5, cx), "Easing · header and foot 4px from home", cx))
        .child(captioned(ui::pip("ds-pip-live", picture(None), own(3, "Lounge", "kiwi"), window, cx), "Live · hover it", cx));
    let viewers = row()
        .gap_6()
        .flex_wrap()
        .child(captioned(ui::pip_at("ds-pip-none", picture(None), own(0, "Lounge", "kiwi"), 1., cx), "No one watching · no stack", cx))
        .child(captioned(ui::pip_at("ds-pip-few", picture(None), own(3, "Lounge", "kiwi"), 1., cx), "Three · every face", cx))
        .child(captioned(ui::pip_at("ds-pip-many", picture(None), own(7, "Lounge", "kiwi"), 1., cx), "Seven · three faces and +4", cx));
    let edges = row()
        .gap_6()
        .flex_wrap()
        .child(captioned(ui::pip_at("ds-pip-long", picture(None), own(99, "a-channel-name-long-enough-to-run-out-of-room", "a member whose display name never ends"), 1., cx), "Long names · both cut short", cx))
        .child(captioned(ui::pip_at("ds-pip-live-wait", picture(Some("You're live")), own(0, "Lounge", "kiwi"), 1., cx), "Ours before its preview", cx))
        .child(captioned(ui::pip_at("ds-pip-joining", picture(Some("Joining moss lantern's stream…")), theirs("Lounge"), 1., cx), "Theirs while joining", cx));
    vec![
        frame(Kind::Variants, "Picture-in-picture / Owner", "Our own stream leads back to the call, shows who's watching and our name, and holds Stream Settings and Stop Streaming. Anyone else's stream or camera has Disconnect.", owners, cx),
        frame(Kind::States, "Picture-in-picture / Chrome", "Hovered, the header and foot fade in over 200ms on the ease-out curve while sliding 8px in from their edges, and leave the same way.", chrome, cx),
        frame(Kind::States, "Picture-in-picture / Viewers", "The call stage's stack of faces, capped at four with +n, the full list on hover.", viewers, cx),
        frame(Kind::Edges, "Picture-in-picture / Worst case", "Names never push the corners off; 99 viewers still fit one slot.", edges, cx),
    ]
}

fn captioned(player: impl IntoElement, caption: &'static str, cx: &App) -> Div {
    ui::col().gap_2().child(player).child(note(caption, cx))
}

// The stream's ground, or a line over it while there's no picture yet.
fn picture(waiting: Option<&'static str>) -> AnyElement {
    crate::call::stream_fill(0.).absolute().inset_0().flex().items_center().justify_center().text_size(ui::TEXT_XS).text_color(gpui_kit::white().opacity(0.7)).children(waiting).into_any_element()
}

fn own(viewers: usize, channel: &'static str, name: &'static str) -> ui::PipChrome {
    let viewers = (0..viewers).map(|i| Viewer { name: NAMES[i % NAMES.len()].into(), avatar: String::new() }).collect();
    let controls = row().gap_1().child(ui::pip_button("ds-pip-settings", Glyph::StreamSettings, "Stream Settings")).child(ui::pip_button("ds-pip-stop", Glyph::ScreenOff, "Stop Streaming"));
    ui::PipChrome {
        top_left: Some(ui::pip_title(channel).into_any_element()),
        top_right: crate::pip::spectators(viewers).map(IntoElement::into_any_element),
        bottom_left: Some(ui::pip_label(name).into_any_element()),
        bottom_right: Some(controls.into_any_element()),
    }
}

fn theirs(channel: &'static str) -> ui::PipChrome {
    ui::PipChrome {
        top_left: Some(ui::pip_title(channel).into_any_element()),
        bottom_right: Some(ui::round_button("ds-pip-disconnect", Glyph::CallEnd, "Disconnect", ui::AVATAR_LG).into_any_element()),
        ..Default::default()
    }
}
