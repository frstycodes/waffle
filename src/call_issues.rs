// What went wrong with a stream or camera, in Waffle's words: Discord's AV error codes as a tile's line or tooltip.
use gpui_kit::{Div, InteractiveElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, component::Icon};

use crate::{
    icons::Glyph,
    palette::stage,
    ui::{self, RADIUS_XS, Squircle},
    voice,
};

pub(crate) fn failure_line(code: u16) -> &'static str {
    match code {
        crate::STREAM_FAILED_TO_START => "Discord didn't connect us to it in time.",
        _ => "Something went wrong with this stream.",
    }
}

// What each of Discord's AV errors means here, as a tile or tooltip says it.
pub(crate) fn issue_line(code: u16) -> &'static str {
    use voice::issues::*;
    match code {
        SOUNDSHARE_FAILED => "Your stream's sound stopped.",
        crate::STREAM_VIEW_LOW_FPS => "The stream is arriving at a low frame rate.",
        STREAM_VIEW_HIGH_PACKET_LOSS => "Over 10% of this stream is being lost on the way.",
        STREAM_SEND_HIGH_PACKET_LOSS => "Viewers are losing over 10% of your stream.",
        STREAM_SEND_LOW_FPS => "Your computer is encoding the stream under its frame rate.",
        STREAM_SEND_NETWORK_QUALITY => "Your connection to the stream server is poor.",
        SENDER_READY_TIMEOUT => "Your video hasn't sent a picture yet.",
        RECEIVER_READY_TIMEOUT => "Their video hasn't arrived yet.",
        CAMERA_SEND_LOW_FPS => "Your camera is sending under 10 frames a second.",
        SCREENSHARE_OS_ERROR => "Your system stopped the screen capture.",
        _ => "Something is wrong with this video.",
    }
}

// A warning mark for a tile's corner; hovering lists each issue with its code.
pub(crate) fn issue_chip(id: SharedString, codes: Vec<u16>) -> gpui_kit::Stateful<Div> {
    ui::row()
        .id(id)
        .size_6()
        .squircle(RADIUS_XS)
        .justify_center()
        .bg(gpui_kit::black().opacity(0.6))
        .child(Icon::new(Glyph::Warning).size(ui::ICON_SM).text_color(stage::idle()))
        .tooltip(move |window, cx| {
            let tip = codes.iter().map(|code| format!("{} Error code: {code}", issue_line(*code))).collect::<Vec<_>>().join("\n");
            ui::tip(tip, window, cx)
        })
}
