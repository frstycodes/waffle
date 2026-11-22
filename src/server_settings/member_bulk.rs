// Bulk moderation on the members table (docs/discord-moderation-re.md §3, 134413 `lN`): pick rows with
// their checkboxes, then add a role, time out, kick or ban them all from the bar that takes the header's
// place. Each action is gated on its own permission, and one toast reports how many went through.
use chrono::Utc;
use gpui_kit::{AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px};
use serde_json::Value;

use super::{
    api::{self, Call},
    members::Member,
    perms,
    view::plural,
};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::perm,
    icons::Glyph,
    menu::item,
    ui::{self, Check, Tone},
    user_menu::{TIMEOUTS, role_choices},
};

// What the signed-in member may do to many members at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BulkPowers {
    pub ban: bool,
    pub timeout: bool,
    pub role: bool,
    pub kick: bool,
}

impl BulkPowers {
    // Bulk ban needs Manage server as well as Ban members; the rest need their one permission.
    pub fn new(has: impl Fn(u64) -> bool) -> Self {
        BulkPowers { ban: has(perms::MANAGE_GUILD) && has(perm::BAN_MEMBERS), timeout: has(perm::MODERATE_MEMBERS), role: has(perm::MANAGE_ROLES), kick: has(perm::KICK_MEMBERS) }
    }

    // Without any of them the table has no checkboxes.
    pub fn any(self) -> bool {
        self.ban || self.timeout || self.role || self.kick
    }
}

// The header checkbox: on when every row on the page is picked, mixed when some are.
pub fn page_check(selected: &[String], page: &[&str]) -> Check {
    let picked = page.iter().filter(|id| selected.iter().any(|s| s == *id)).count();
    match picked {
        0 => Check::Off,
        n if n == page.len() => Check::On,
        _ => Check::Mixed,
    }
}

// Picks every row on the page, or drops them all when they already are.
pub fn toggle_page(selected: &mut Vec<String>, page: &[&str]) {
    if page_check(selected, page) == Check::On {
        selected.retain(|s| !page.contains(&s.as_str()));
        return;
    }
    for id in page {
        if !selected.iter().any(|s| s == id) {
            selected.push((*id).to_owned());
        }
    }
}

pub fn toggle_one(selected: &mut Vec<String>, id: &str) {
    match selected.iter().position(|s| s == id) {
        Some(i) => drop(selected.remove(i)),
        None => selected.push(id.to_owned()),
    }
}

// "Banned 14 members", or "Banned 12 of 14" when some were refused.
pub fn outcome(verb: &str, done: usize, asked: usize) -> String {
    if done == asked {
        return format!("{verb} {}", plural(asked as u64, "member", "members"));
    }
    format!("{verb} {done} of {asked}")
}

// Not verified: `banned_users` as the public API documents it; an answer without it counts every id.
pub fn banned_ids(body: &Value, asked: &[String]) -> Vec<String> {
    match body["banned_users"].as_array() {
        Some(ids) => ids.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        None => asked.to_vec(),
    }
}

impl Waffle {
    fn bulk_target(&self) -> Option<(String, Vec<String>)> {
        let view = self.server_settings.as_ref()?;
        let ids = view.lists.members.selected.clone();
        (!ids.is_empty()).then(|| (view.guild_id.clone(), ids))
    }

    fn edit_selection(&mut self, edit: impl FnOnce(&mut Vec<String>, &[&str]), cx: &mut Context<Self>) {
        let Some(m) = self.server_settings.as_mut().map(|v| &mut v.lists.members) else { return };
        let page: Vec<String> = match &m.rows {
            Load::Ready(rows) => rows.iter().skip(m.page * m.page_size).take(m.page_size).map(|r| r.id.clone()).collect(),
            _ => Vec::new(),
        };
        let page: Vec<&str> = page.iter().map(String::as_str).collect();
        edit(&mut m.selected, &page);
        cx.notify();
    }

    pub(super) fn toggle_member_selected(&mut self, id: String, cx: &mut Context<Self>) {
        self.edit_selection(|selected, _| toggle_one(selected, &id), cx);
    }

    pub(super) fn toggle_page_selected(&mut self, cx: &mut Context<Self>) {
        self.edit_selection(toggle_page, cx);
    }

    fn clear_member_selection(&mut self, cx: &mut Context<Self>) {
        self.edit_selection(|selected, _| selected.clear(), cx);
    }

    fn set_bulk_busy(&mut self, busy: bool) {
        if let Some(m) = self.server_settings.as_mut().map(|v| &mut v.lists.members) {
            m.bulk_busy = busy;
        }
    }

    // Applies `change` to every loaded row whose id went through, then drops the selection.
    fn bulk_settled(&mut self, done: &[String], change: impl Fn(&mut Vec<Member>, usize)) {
        self.set_bulk_busy(false);
        let Some(m) = self.server_settings.as_mut().map(|v| &mut v.lists.members) else { return };
        m.selected.clear();
        let Load::Ready(rows) = &mut m.rows else { return };
        for id in done {
            if let Some(i) = rows.iter().position(|r| &r.id == id) {
                change(rows, i);
            }
        }
    }

    // One call per member, one after another so a hundred don't land at once; one toast at the end.
    fn run_each(&mut self, verb: &'static str, calls: Vec<(String, Call)>, change: impl Fn(&mut Vec<Member>, usize) + 'static, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return self.show_toast("Not connected to Discord.", cx) };
        self.set_bulk_busy(true);
        cx.spawn(async move |this, cx| {
            let asked = calls.len();
            let mut done = Vec::new();
            for (id, call) in calls {
                if call.send(&client).await.is_ok() {
                    done.push(id);
                }
            }
            this.update(cx, |this, cx| {
                this.bulk_settled(&done, change);
                this.show_toast(outcome(verb, done.len(), asked), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ask_bulk_ban(&mut self, cx: &mut Context<Self>) {
        let Some((guild_id, ids)) = self.bulk_target() else { return };
        let n = plural(ids.len() as u64, "member", "members");
        self.confirm(format!("Ban {n}?"), "They leave now and can't rejoin with any invite until someone unbans them.", format!("Ban {n}"), move |this, _, cx| {
            this.set_bulk_busy(true);
            let ids = ids.clone();
            this.server_call(api::bulk_ban(&guild_id, &ids, 0), move |this, result, cx| {
                let done = match result {
                    Ok(body) => banned_ids(&body, &ids),
                    Err(error) => {
                        this.set_bulk_busy(false);
                        return this.show_toast(error, cx);
                    }
                };
                this.bulk_settled(&done, |rows, i| drop(rows.remove(i)));
                this.show_toast(outcome("Banned", done.len(), ids.len()), cx);
            }, cx);
        }, cx);
    }

    fn ask_bulk_kick(&mut self, cx: &mut Context<Self>) {
        let Some((guild_id, ids)) = self.bulk_target() else { return };
        let n = plural(ids.len() as u64, "member", "members");
        self.confirm(format!("Kick {n}?"), "They can rejoin with a new invite.", format!("Kick {n}"), move |this, _, cx| {
            let calls = ids.iter().map(|id| (id.clone(), api::kick(&guild_id, id))).collect();
            this.run_each("Kicked", calls, |rows, i| drop(rows.remove(i)), cx);
        }, cx);
    }

    fn bulk_timeout(&mut self, seconds: i64, cx: &mut Context<Self>) {
        let Some((guild_id, ids)) = self.bulk_target() else { return };
        let until = Utc::now() + chrono::Duration::seconds(seconds);
        let wire = until.to_rfc3339();
        let calls = ids.iter().map(|id| (id.clone(), api::timeout(&guild_id, id, Some(&wire)))).collect();
        self.run_each("Timed out", calls, move |rows, i| rows[i].timed_out_until = Some(until), cx);
    }

    fn bulk_add_role(&mut self, role_id: String, cx: &mut Context<Self>) {
        let Some((guild_id, ids)) = self.bulk_target() else { return };
        let calls = ids.iter().map(|id| (id.clone(), api::add_member_role(&guild_id, id, &role_id))).collect();
        self.run_each("Gave the role to", calls, move |rows, i| {
            if !rows[i].roles.contains(&role_id) {
                rows[i].roles.push(role_id.clone());
            }
        }, cx);
    }

    // Discord's timeout lengths, each applied to every picked member.
    fn open_bulk_timeout(&mut self, event: &ClickEvent, cx: &mut Context<Self>) {
        self.open_live_menu(event.position(), |_| TIMEOUTS.iter().map(|&(label, seconds)| item(label, move |this, _, cx| this.bulk_timeout(seconds, cx)).entry()).collect(), cx);
    }

    // The roles we can hand out: below our highest role.
    fn open_bulk_roles(&mut self, event: &ClickEvent, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.open_live_menu(event.position(), move |this| {
            let (Some(me), Some(guild)) = (this.me.as_ref(), this.guilds.iter().find(|g| g.id == guild_id)) else { return Vec::new() };
            let top = guild.top_position(&me.id);
            role_choices(guild)
                .into_iter()
                .filter(|r| r.position < top)
                .map(|role| {
                    let rid = role.id.clone();
                    item(role.name.clone(), move |this, _, cx| this.bulk_add_role(rid.clone(), cx)).dot((role.color != 0).then_some(role.color)).entry()
                })
                .collect()
        }, cx);
    }

    // Floats over the Members page while any row is picked: the count, each action we may take, and clear.
    pub(crate) fn member_bulk_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.server_settings.as_ref().filter(|v| v.page == super::access::Page::Members)?;
        let m = &view.lists.members;
        if m.selected.is_empty() {
            return None;
        }
        let powers = self.with_access(&view.guild_id, |me| BulkPowers::new(|bits| me.has(bits))).unwrap_or_default();
        let (count, busy) = (m.selected.len(), m.bulk_busy);
        let button = |id: &'static str, glyph: Glyph, label: &'static str, cx: &mut Context<Self>| ui::button(id, Tone::Secondary, cx).disabled(busy).child(gpui_kit::component::Icon::new(glyph).size(px(16.))).child(label);
        let role = powers.role.then(|| button("ss-bulk-role", Glyph::Plus, "Add role", cx).on_click(cx.listener(|this, event: &ClickEvent, _, cx| this.open_bulk_roles(event, cx))));
        let timeout = powers.timeout.then(|| button("ss-bulk-timeout", Glyph::Timer, "Time out", cx).on_click(cx.listener(|this, event: &ClickEvent, _, cx| this.open_bulk_timeout(event, cx))));
        let kick = powers.kick.then(|| button("ss-bulk-kick", Glyph::DoorExit, "Kick", cx).danger(cx).on_click(cx.listener(|this, _, _, cx| this.ask_bulk_kick(cx))));
        let too_many = count > api::BULK_BAN_MAX;
        let ban = powers.ban.then(|| {
            let ban = button("ss-bulk-ban", Glyph::Prohibit, "Ban", cx).danger(cx).disabled(busy || too_many).on_click(cx.listener(|this, _, _, cx| this.ask_bulk_ban(cx)));
            let tip = SharedString::from(format!("Ban up to {} at a time", api::BULK_BAN_MAX));
            if too_many { ban.tooltip(move |window, cx| ui::tip(tip.clone(), window, cx)) } else { ban }
        });
        let clear = ui::icon_button("ss-bulk-clear", Glyph::X, "Clear selection", cx).on_click(cx.listener(|this, _, _, cx| this.clear_member_selection(cx)));
        let bar = ui::floating_bar("ss-bulk", cx)
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(crate::palette::strong(cx)).child(format!("{count} selected")))
            .child(div().flex().flex_none().gap(px(8.)).children(role).children(timeout).children(kick).children(ban).child(clear));
        Some(ui::foot_dock(bar, "ss-bulk-in").into_any_element())
    }

    // A row's or the header's checkbox.
    pub(super) fn member_select_box(&self, id: SharedString, state: Check, on_toggle: impl Fn(&mut Waffle, &mut Context<Waffle>) + 'static, cx: &mut Context<Self>) -> AnyElement {
        div().id(id).cursor_pointer().child(ui::check(state, cx)).on_click(cx.listener(move |this, _, _, cx| on_toggle(this, cx))).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn gates_follow_discord() {
        let with = |bits: u64| BulkPowers::new(|b| bits & b != 0);
        assert_eq!(with(0), BulkPowers::default());
        assert!(!with(0).any());
        // Ban members alone isn't enough: bulk ban also needs Manage server.
        assert!(!with(perm::BAN_MEMBERS).ban);
        assert!(with(perm::BAN_MEMBERS | perms::MANAGE_GUILD).ban);
        assert_eq!(with(perm::MODERATE_MEMBERS), BulkPowers { timeout: true, ..Default::default() });
        assert_eq!(with(perm::MANAGE_ROLES), BulkPowers { role: true, ..Default::default() });
        assert_eq!(with(perm::KICK_MEMBERS), BulkPowers { kick: true, ..Default::default() });
    }

    #[test]
    fn selection() {
        let page = ["a", "b", "c"];
        let mut selected = vec!["z".to_owned()];
        assert_eq!(page_check(&selected, &page), Check::Off);
        toggle_one(&mut selected, "b");
        assert_eq!(page_check(&selected, &page), Check::Mixed);
        toggle_page(&mut selected, &page);
        assert_eq!((page_check(&selected, &page), selected.len()), (Check::On, 4));
        // All on: select-all drops the page and keeps rows picked on other pages.
        toggle_page(&mut selected, &page);
        assert_eq!(selected, vec!["z".to_owned()]);
        toggle_one(&mut selected, "z");
        assert!(selected.is_empty());
        assert_eq!(page_check(&selected, &[]), Check::Off);
    }

    #[test]
    fn outcomes() {
        assert_eq!(outcome("Banned", 14, 14), "Banned 14 members");
        assert_eq!(outcome("Banned", 1, 1), "Banned 1 member");
        assert_eq!(outcome("Banned", 12, 14), "Banned 12 of 14");
        let asked = vec!["1".to_owned(), "2".to_owned()];
        assert_eq!(banned_ids(&json!({ "banned_users": ["2"], "failed_users": ["1"] }), &asked), vec!["2".to_owned()]);
        assert_eq!(banned_ids(&json!({}), &asked), asked);
    }
}
