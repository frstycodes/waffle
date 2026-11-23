use crate::icons::Glyph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Account,
    Profiles,
    ContentSocial,
    Appearance,
    Accessibility,
    VoiceVideo,
    Chat,
    Notifications,
    Keybinds,
    Advanced,
    Updates,
    ActivityPrivacy,
}

impl Page {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Page::Account => "My account",
            Page::Profiles => "Profiles",
            Page::ContentSocial => "Content and social",
            Page::Appearance => "Appearance",
            Page::Accessibility => "Accessibility",
            Page::VoiceVideo => "Voice and video",
            Page::Chat => "Chat",
            Page::Notifications => "Notifications",
            Page::Keybinds => "Keybinds",
            Page::Advanced => "Advanced",
            Page::Updates => "Updates",
            Page::ActivityPrivacy => "Activity privacy",
        }
    }

    pub(super) fn glyph(self) -> Glyph {
        match self {
            Page::Account => Glyph::UserCircle,
            Page::Profiles => Glyph::Id,
            Page::ContentSocial => Glyph::Shield,
            Page::Appearance => Glyph::Palette,
            Page::Accessibility => Glyph::Eye,
            Page::VoiceVideo => Glyph::Mic,
            Page::Chat => Glyph::ChatRound,
            Page::Notifications => Glyph::Bell,
            Page::Keybinds => Glyph::Keyboard,
            Page::Advanced => Glyph::Settings,
            Page::Updates => Glyph::Download,
            Page::ActivityPrivacy => Glyph::Game,
        }
    }

    // In-page anchors listed under the open page.
    pub(super) fn sections(self) -> &'static [&'static str] {
        match self {
            Page::VoiceVideo => &["Voice", "Camera", "Sounds", "Advanced"],
            _ => &[],
        }
    }
}

// Discord's three headings. My account sits on its own as the identity row at the top of the nav.
pub(super) const NAV: [(&str, &[Page]); 3] = [
    ("User settings", &[Page::Profiles, Page::ContentSocial]),
    ("App settings", &[Page::Appearance, Page::Accessibility, Page::VoiceVideo, Page::Chat, Page::Notifications, Page::Keybinds, Page::Advanced, Page::Updates]),
    ("Activity settings", &[Page::ActivityPrivacy]),
];

