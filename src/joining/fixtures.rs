// The demo server "1" as a fresh member of a community: onboarding, rules and a Server Guide (debug demos only).
use serde_json::json;

use super::{Form, Onboarding, Welcome, features};
use crate::{Waffle, channel_settings::Load, discord::Membership};

pub fn install(waffle: &mut Waffle) {
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    guild.features = [features::COMMUNITY, features::ONBOARDING, features::ONBOARDING_HAS_PROMPTS, features::SERVER_GUIDE, features::RULES_GATE, features::VERIFIED].map(str::to_owned).to_vec();
    guild.rules_channel_id = Some("general".into());
    guild.me = Membership { pending: true, flags: 0, joined_at: Some(chrono::Utc::now().to_rfc3339()) };
    for (id, name, color) in [("r-ranked", "Ranked", 0xE8A33Du32), ("r-clips", "Clipper", 0xD9475B), ("r-eu", "EU", 0x4F8FE0), ("r-na", "NA", 0x3FAE7A), ("r-apac", "APAC", 0xA46AD8)] {
        guild.roles.push(crate::discord::Role { id: id.into(), name: name.into(), color, position: 1, permissions: 0, managed: false });
    }
    waffle.join.forms.insert("1".into(), Load::Ready(form()));
    waffle.join.onboarding.insert("1".into(), Load::Ready(onboarding()));
    waffle.join.welcome.insert("1".into(), Load::Ready(welcome()));
}

pub fn form() -> Form {
    Form::parse(&json!({ "version": "2026-11-01T00:00:00Z", "form_fields": [{ "field_type": "TERMS", "label": "Read and agree to the server rules", "required": true, "values": [
        "Be kind. No harassment, slurs or dogpiling.",
        "Keep it in the right channel: clips in #📷-media, everything else in #👋-general.",
        "No spoilers for two weeks after a release.",
        "No self-promotion without asking a moderator.",
        "Use English in the main channels so everyone can follow along, and keep other languages to threads.",
        "Voice: push-to-talk in hangout after midnight."
    ]}]}))
}

pub fn onboarding() -> Onboarding {
    Onboarding::parse(&json!({ "enabled": true, "default_channel_ids": ["general"], "responses": [], "prompts": [
        { "id": "p-why", "title": "What brings you to Rust & gpui?", "type": 0, "single_select": true, "required": true, "in_onboarding": true, "options": [
            { "id": "o-ranked", "title": "Ranked", "description": "Climbing together", "emoji": { "name": "🏆" }, "role_ids": ["r-ranked"], "channel_ids": ["general"] },
            { "id": "o-clips", "title": "Making clips", "description": "Edits and highlights", "emoji": { "name": "🎬" }, "role_ids": ["r-clips"], "channel_ids": ["media"] },
            { "id": "o-chat", "title": "Hanging out", "description": "Just here to chat", "emoji": { "name": "💬" }, "channel_ids": ["long"] },
            { "id": "o-group", "title": "Finding a group for tonight's late-night co-op session across three time zones", "description": "Squads, duos, and anyone who wants to queue up after work", "emoji": { "name": "👥" }, "channel_ids": ["lounge", "hub"] }
        ]},
        { "id": "p-games", "title": "Which games do you play?", "type": 0, "single_select": false, "required": false, "in_onboarding": true, "options": [
            { "id": "o-hades", "title": "Hades II", "emoji": { "name": "🔥" }, "channel_ids": ["media"] },
            { "id": "o-rift", "title": "Rift", "emoji": { "name": "⚔️" }, "channel_ids": ["long"], "role_ids": ["r-ranked"] },
            { "id": "o-peaks", "title": "Ashen Peaks", "emoji": { "name": "🏔️" } },
            { "id": "o-desert", "title": "Glass Desert", "emoji": { "name": "🏜️" } },
            { "id": "o-harbor", "title": "Drowned Harbor", "emoji": { "name": "⚓" } }
        ]},
        { "id": "p-region", "title": "What region are you in?", "type": 1, "single_select": true, "required": false, "in_onboarding": true, "options": [
            { "id": "o-eu", "title": "Europe", "role_ids": ["r-eu"] },
            { "id": "o-na", "title": "North America", "role_ids": ["r-na"] },
            { "id": "o-apac", "title": "Asia Pacific", "role_ids": ["r-apac"] }
        ]},
        { "id": "p-pings", "title": "Which announcements do you want pings for?", "type": 0, "single_select": false, "required": false, "in_onboarding": false, "options": [
            { "id": "o-events", "title": "Events", "emoji": { "name": "📅" } },
            { "id": "o-patches", "title": "Patch notes", "emoji": { "name": "📝" } }
        ]}
    ]}))
}

pub fn welcome() -> Welcome {
    Welcome::parse(&json!({ "enabled": true,
        "welcome_message": { "author_ids": ["4194304"], "message": "Hey night owl! We play ranked most nights after 9. Pick your games so you see the right channels, and say hi in #👋-general." },
        "new_member_actions": [
            { "channel_id": "general", "action_type": 1, "title": "Introduce yourself", "description": "Say hi and what you play", "emoji": { "name": "👋" } },
            { "channel_id": "media", "action_type": 0, "title": "Check out the clips", "emoji": { "name": "🎬" } },
            { "channel_id": "long", "action_type": 0, "title": "Read the long channel with the very long name that keeps going", "emoji": { "name": "📖" } }
        ],
        "resource_channels": [
            { "channel_id": "general", "title": "FAQ", "description": "Ranks, roles, and how scrims work." },
            { "channel_id": "media", "title": "Patch notes digest", "description": "The week's changes, in one post." }
        ]}))
}
