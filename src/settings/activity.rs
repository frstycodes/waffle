// Where I share activity (Discord's Activity Privacy, second category): the default for servers you join,
// then a switch per server bound to `activity_restricted_guild_ids` (on = not in the list).
// Presence goes out once per session (gateway op 3, no guild in it), so Waffle can't withhold your game
// from one server; Discord applies the list when it fans your presence out to each server.
use gpui_kit::{Context, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px};
use serde_json::{Value, json};

use super::{Section, Select};
use crate::{Waffle, discord::AccountSettings, palette, ui};

const GROUP: &str = "Where I share activity";

// (stored value, label), in Discord's order.
// Not verified: the proto enum's numbering (taken as 0 share everywhere, 1 restrict servers over 200, 2 restrict all).
// The width of the "When joining a server" dropdown.
const JOIN_FIELD_WIDTH: f32 = 260.;
const JOIN_DEFAULTS: [(u8, &str); 3] = [(0, "Share in all servers"), (1, "Share in servers with 200 or fewer members"), (2, "Do not share in any servers")];

impl Waffle {
    pub(super) fn activity_sharing_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let account = &self.account_settings;
        let chosen = account.default_guilds_activity_restricted;
        let options: Vec<(String, String)> = JOIN_DEFAULTS.iter().map(|(value, label)| (value.to_string(), (*label).to_owned())).collect();
        let value = JOIN_DEFAULTS.iter().find(|(v, _)| *v == chosen).map_or(JOIN_DEFAULTS[0].1, |(_, label)| label);
        let field = div().w(px(JOIN_FIELD_WIDTH)).flex_none().child(self.select_field(Select::JoinActivity, None, &options, &chosen.to_string(), value.into(), 36., cx));
        let join = ui::Item::setting("activity-join-default", false, cx).child(ui::settings_text("When joining a server", Some("Whether new servers see your activity"), cx)).child(field);
        let sharing = account.show_current_game;
        let mut card = ui::Card::new(cx).child(join);
        for guild in &self.guilds {
            let shared = !account.activity_restricted_guilds.contains(&guild.id);
            card = card.child(self.guild_activity_row(&guild.id, guild.icon.as_deref(), &guild.name, shared, sharing, window, cx));
        }
        let warning = (!sharing).then(|| ui::settings_warning("Server settings ignored when not sharing", cx));
        vec![Section::keyed(GROUP, ui::settings_group(GROUP, cx).child(card).children(warning))]
    }

    // With sharing off the rows dim but stay switchable, as in Discord: the list still saves for later.
    #[allow(clippy::too_many_arguments)]
    fn guild_activity_row(&self, id: &str, icon: Option<&str>, name: &str, shared: bool, sharing: bool, _window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let key = format!("activity-guild-{id}");
        let label = div().flex_1().min_w_0().text_color(palette::strong(cx)).child(ui::truncating(format!("{key}-name"), name.to_owned()));
        let switch = ui::Switch::new(SharedString::from(format!("{key}-switch"))).checked(shared);
        let guild_id = id.to_owned();
        ui::settings_choice(ui::Item::setting(key, true, cx), cx)
            .when(!sharing, |row| row.opacity(0.6))
            .child(ui::server_tile(icon, name, 24., cx))
            .child(label)
            .child(switch)
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_guild_activity(&guild_id, cx)))
    }

    pub(crate) fn toggle_guild_activity(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let before = self.account_settings.clone();
        let share = before.activity_restricted_guilds.iter().any(|g| g == guild_id);
        let patch = share_in_guild(&mut self.account_settings, guild_id, share);
        self.save_account_settings(before, patch, cx);
    }

    pub(super) fn set_join_activity_default(&mut self, value: u8, cx: &mut Context<Self>) {
        let before = self.account_settings.clone();
        self.account_settings.default_guilds_activity_restricted = value.min(2);
        self.save_settings_proto(before, super::proto::guild_activity_default(value), cx);
    }
}

// Takes `guild_id` off the activity list (`share`) or puts it on, and returns the settings patch.
fn share_in_guild(account: &mut AccountSettings, guild_id: &str, share: bool) -> Value {
    let list = &mut account.activity_restricted_guilds;
    list.retain(|g| g != guild_id);
    if !share {
        list.push(guild_id.to_owned());
    }
    json!({ "activity_restricted_guild_ids": list })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guild_switch_adds_or_removes_only_that_guild() {
        let mut account = AccountSettings { activity_restricted_guilds: vec!["1".into(), "2".into()], ..Default::default() };
        assert_eq!(share_in_guild(&mut account, "1", true), json!({ "activity_restricted_guild_ids": ["2"] }));
        assert_eq!(share_in_guild(&mut account, "3", false), json!({ "activity_restricted_guild_ids": ["2", "3"] }));
        assert_eq!(share_in_guild(&mut account, "3", false), json!({ "activity_restricted_guild_ids": ["2", "3"] }), "no duplicate");
        assert_eq!(share_in_guild(&mut account, "9", true), json!({ "activity_restricted_guild_ids": ["2", "3"] }), "absent stays absent");
        assert_eq!(account.activity_restricted_guilds, ["2", "3"]);
    }
}
