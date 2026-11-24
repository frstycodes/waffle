// Waffle's spot art, where Discord leads a screen with an illustration (docs/illustrations.md). Each is a PNG in
// assets/illustrations/, drawn by Codex from the prompt saved beside it and scaled down to 480px wide.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Illustration {
    FriendsOnline,
    FriendsAll,
    FriendsPending,
    FriendsBlocked,
    PinsEmpty,
    SearchEmpty,
    InboxEmpty,
    ForumEmpty,
    LoadFailed,
    ReportSent,
    // The waffle the Windows installer shows while it works.
    InstallerWaffle,
}

impl Illustration {
    pub const ALL: [Self; 11] = [Self::FriendsOnline, Self::FriendsAll, Self::FriendsPending, Self::FriendsBlocked, Self::PinsEmpty, Self::SearchEmpty, Self::InboxEmpty, Self::ForumEmpty, Self::LoadFailed, Self::ReportSent, Self::InstallerWaffle];

    pub fn path(self) -> &'static str {
        ASSETS[self as usize].0
    }
}

// In `Illustration`'s order.
pub const ASSETS: [(&str, &[u8]); 11] = [
    ("illustrations/friends-online.png", include_bytes!("../assets/illustrations/friends-online.png")),
    ("illustrations/friends-all.png", include_bytes!("../assets/illustrations/friends-all.png")),
    ("illustrations/friends-pending.png", include_bytes!("../assets/illustrations/friends-pending.png")),
    ("illustrations/friends-blocked.png", include_bytes!("../assets/illustrations/friends-blocked.png")),
    ("illustrations/pins-empty.png", include_bytes!("../assets/illustrations/pins-empty.png")),
    ("illustrations/search-empty.png", include_bytes!("../assets/illustrations/search-empty.png")),
    ("illustrations/inbox-empty.png", include_bytes!("../assets/illustrations/inbox-empty.png")),
    ("illustrations/forum-empty.png", include_bytes!("../assets/illustrations/forum-empty.png")),
    ("illustrations/load-failed.png", include_bytes!("../assets/illustrations/load-failed.png")),
    ("illustrations/report-sent.png", include_bytes!("../assets/illustrations/report-sent.png")),
    ("illustrations/installer-waffle.png", include_bytes!("../assets/illustrations/installer-waffle.png")),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_illustration_maps_to_its_own_png() {
        for art in Illustration::ALL {
            let name: String = format!("{art:?}").chars().enumerate().flat_map(|(i, c)| if c.is_uppercase() && i > 0 { vec!['-', c.to_ascii_lowercase()] } else { vec![c.to_ascii_lowercase()] }).collect();
            assert_eq!(art.path(), format!("illustrations/{name}.png"));
            assert!(ASSETS[art as usize].1.starts_with(b"\x89PNG"), "{name} is not a PNG");
        }
    }
}
