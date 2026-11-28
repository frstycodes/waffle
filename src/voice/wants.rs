// Discord's media sink wants (voice op 15): how good a picture we ask the SFU for from each sender. Every
// camera gets a level from how many are on, the one shown big gets full quality, and a camera the user
// disabled gets none, so it isn't downloaded at all.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

// Each sender's video SSRCs, from op 12 (never their audio: a want of 0 there could silence them).
pub(super) type VideoSsrcs = HashMap<String, Vec<u32>>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoWants {
    // Other people's cameras that are on and not disabled.
    pub cameras: usize,
    pub focused: Option<String>,
    pub disabled: Vec<String>,
}

const FULL: u8 = 100;

// Discord's ladder over a 720p budget: one camera at full quality, then smaller steps as the grid fills
// (2 → 90, 4 → 70, 10 → 40, 17 or more → 20).
pub(super) fn level(cameras: usize) -> u8 {
    match cameras {
        0 | 1 => FULL,
        2 => 90,
        3 => 80,
        4 => 70,
        5 | 6 => 60,
        7..=9 => 50,
        10 | 11 => 40,
        12..=16 => 30,
        _ => 20,
    }
}

// The op 15 payload: `any` for everyone, then per-SSRC overrides for the focused and disabled users.
pub(super) fn payload(wants: &VideoWants, senders: &VideoSsrcs) -> Value {
    let mut d = Map::new();
    d.insert("any".into(), json!(level(wants.cameras)));
    for (user, ssrcs) in senders {
        let want = match () {
            _ if wants.disabled.contains(user) => 0,
            _ if wants.focused.as_ref() == Some(user) => FULL,
            _ => continue,
        };
        for ssrc in ssrcs {
            d.insert(ssrc.to_string(), json!(want));
        }
    }
    Value::Object(d)
}

// Op 12's video SSRCs: the main one and each stream's, without audio or retransmission.
pub(super) fn video_ssrcs(d: &Value) -> Vec<u32> {
    let streams = d["streams"].as_array().into_iter().flatten().map(|s| &s["ssrc"]);
    let mut ssrcs: Vec<u32> = std::iter::once(&d["video_ssrc"]).chain(streams).filter_map(Value::as_u64).filter(|&ssrc| ssrc != 0).map(|ssrc| ssrc as u32).collect();
    ssrcs.sort_unstable();
    ssrcs.dedup();
    ssrcs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_falls_as_cameras_come_on() {
        let levels: Vec<u8> = [1, 2, 4, 5, 7, 10, 12, 17, 40].into_iter().map(level).collect();
        assert_eq!(levels, [100, 90, 70, 60, 50, 40, 30, 20, 20]);
    }

    #[test]
    fn focused_gets_full_and_disabled_gets_none() {
        let senders: VideoSsrcs = [("a", vec![1, 2]), ("b", vec![3]), ("c", vec![4])].into_iter().map(|(u, s)| (u.to_owned(), s)).collect();
        let wants = VideoWants { cameras: 4, focused: Some("a".into()), disabled: vec!["b".into()] };
        assert_eq!(payload(&wants, &senders), json!({ "any": 70, "1": 100, "2": 100, "3": 0 }));
        let op12 = json!({ "audio_ssrc": 9, "video_ssrc": 10, "rtx_ssrc": 11, "streams": [{ "ssrc": 10, "rtx_ssrc": 11 }, { "ssrc": 0 }] });
        assert_eq!(video_ssrcs(&op12), [10], "never audio or retransmission");
    }
}
