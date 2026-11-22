// Roles: the list (highest first, @everyone as Default permissions), reordering with Undo, and one
// role's editor (Display, Permissions, Members). The editor's draft rides in the window's save bar.
use std::collections::HashMap;

use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, Focusable, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        ActiveTheme, Icon,
        input::{Input, InputEvent, InputState},
    },
    div, prelude::FluentBuilder, px,
};
use serde_json::Value;

use super::{
    access::{self, RoleLock},
    api::{self, RoleDraft},
    perms,
    view::{failed, loading_line, plural},
};
use crate::{
    Waffle,
    channel_settings::Load,
    drag::{Look, RowDrag},
    sortable::{Board, Finished, Place},
    discord::perm,
    icons::Glyph,
    palette,
    ui::{self, Notice, Tone},
};

// The roles list's sortable list and its drag (sortable.rs).
const ROLE_LIST: &str = "roles";

#[derive(Clone)]
struct RoleDrag;

fn role_board(waffle: &mut Waffle) -> &mut Board {
    &mut waffle.role_board
}

// Discord's role colour swatches.
pub const COLORS: [u32; 20] = [
    0x1ABC9C, 0x2ECC71, 0x3498DB, 0x9B59B6, 0xE91E63, 0xF1C40F, 0xE67E22, 0xE74C3C, 0x95A5A6, 0x607D8B, 0x11806A, 0x1F8B4C, 0x206694, 0x71368A, 0xAD1457, 0xC27C0E, 0xA84300, 0x992D22, 0x979C9F, 0x546E7A,
];

#[derive(Clone, Debug, PartialEq)]
pub struct RoleRow {
    pub id: String,
    pub position: i64,
    pub managed: bool,
    pub role: RoleDraft,
}

pub fn parse_role(r: &Value) -> Option<RoleRow> {
    Some(RoleRow { id: r["id"].as_str()?.to_owned(), position: r["position"].as_i64().unwrap_or(0), managed: r["managed"].as_bool().unwrap_or(false), role: RoleDraft::parse(r) })
}

// Highest first, as Discord lists them; ties by id like Discord.
pub fn sorted(mut roles: Vec<RoleRow>) -> Vec<RoleRow> {
    roles.sort_by(|a, b| b.position.cmp(&a.position).then_with(|| a.id.cmp(&b.id)));
    roles
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoleTab {
    Display,
    Permissions,
    Members,
}

pub struct RoleEdit {
    pub id: String,
    pub tab: RoleTab,
    pub saved: RoleDraft,
    pub draft: RoleDraft,
    pub members: Load<Vec<String>>,
    pub asking_delete: bool,
}

pub struct RolesView {
    pub list: Load<Vec<RoleRow>>,
    pub counts: HashMap<String, u64>,
    pub search: Entity<InputState>,
    pub perm_search: Entity<InputState>,
    pub name: Entity<InputState>,
    pub open: Option<RoleEdit>,
    // A role just created: its editor opens on the next draw, which has the window its name field needs.
    pub pending_open: Option<String>,
}

impl RolesView {
    pub fn new(window: &mut Window, cx: &mut Context<Waffle>) -> Self {
        let mut input = |placeholder: &'static str| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            cx.subscribe(&state, |_, _, _: &InputEvent, cx| cx.notify()).detach();
            state
        };
        RolesView { list: Load::Loading, counts: HashMap::new(), search: input("Search roles"), perm_search: input("Search permissions"), name: input("Role name"), open: None, pending_open: None }
    }
}

impl Waffle {
    pub(crate) fn load_roles(&mut self, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.server_call(api::roles(&guild_id), |this, result, _| {
            if let Some(view) = &mut this.server_settings {
                view.roles.list = match result {
                    Ok(body) => Load::Ready(sorted(body.as_array().into_iter().flatten().filter_map(parse_role).collect())),
                    Err(error) => Load::Failed(error),
                };
            }
        }, cx);
        self.server_call(api::role_member_counts(&guild_id), |this, result, _| {
            let Some(view) = &mut this.server_settings else { return };
            if let Ok(Value::Object(counts)) = result {
                view.roles.counts = counts.into_iter().filter_map(|(id, n)| Some((id, n.as_u64()?))).collect();
            }
        }, cx);
    }

    fn open_role(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        let Load::Ready(list) = &view.roles.list else { return };
        let Some(row) = list.iter().find(|r| r.id == id) else { return };
        view.roles.name.update(cx, |s, cx| s.set_value(row.role.name.clone(), window, cx));
        view.roles.open = Some(RoleEdit { id: id.clone(), tab: RoleTab::Display, saved: row.role.clone(), draft: row.role.clone(), members: Load::Loading, asking_delete: false });
        let guild_id = view.guild_id.clone();
        self.server_call(api::role_member_ids(&guild_id, &id), move |this, result, _| {
            let Some(edit) = this.server_settings.as_mut().and_then(|v| v.roles.open.as_mut()).filter(|e| e.id == id) else { return };
            edit.members = match result {
                Ok(body) => Load::Ready(body.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect()),
                Err(error) => Load::Failed(error),
            };
        }, cx);
        cx.notify();
    }

    fn role_current(&self, cx: &App) -> Option<(String, RoleDraft, RoleDraft)> {
        let view = self.server_settings.as_ref()?;
        let edit = view.roles.open.as_ref()?;
        let draft = RoleDraft { name: view.roles.name.read(cx).value().to_string(), ..edit.draft.clone() };
        Some((edit.id.clone(), edit.saved.clone(), draft))
    }

    pub(crate) fn role_dirty(&self, cx: &App) -> bool {
        self.role_current(cx).is_some_and(|(_, saved, draft)| !api::role_patch(&saved, &draft).is_empty())
    }

    pub(crate) fn role_save_call(&self, cx: &App) -> Option<api::Call> {
        let (id, saved, draft) = self.role_current(cx)?;
        api::edit_role(&self.server_settings.as_ref()?.guild_id, &id, &saved, &draft)
    }

    pub(crate) fn role_saved(&mut self, body: &Value) {
        let Some(fresh) = parse_role(body) else { return };
        if let Some(guild) = self.server_settings.as_ref().and_then(|v| self.guilds.iter_mut().find(|g| g.id == v.guild_id))
            && let Some(role) = guild.roles.iter_mut().find(|r| r.id == fresh.id) {
                (role.name, role.color, role.permissions) = (fresh.role.name.clone(), fresh.role.color, fresh.role.permissions);
            }
        let Some(view) = &mut self.server_settings else { return };
        if let Load::Ready(list) = &mut view.roles.list
            && let Some(row) = list.iter_mut().find(|r| r.id == fresh.id) {
                row.role = fresh.role.clone();
            }
        if let Some(edit) = view.roles.open.as_mut().filter(|e| e.id == fresh.id) {
            edit.saved = fresh.role.clone();
            edit.draft = RoleDraft { name: edit.draft.name.clone(), ..fresh.role };
        }
    }

    pub(crate) fn reset_role_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        let Some(edit) = &mut view.roles.open else { return };
        edit.draft = edit.saved.clone();
        let name = edit.saved.name.clone();
        view.roles.name.update(cx, |s, cx| s.set_value(name, window, cx));
    }

    fn edit_role_draft(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut RoleDraft)) {
        if let Some(edit) = self.server_settings.as_mut().and_then(|v| v.roles.open.as_mut()) {
            change(&mut edit.draft);
        }
        if let Some(view) = &mut self.server_settings {
            view.save = super::view::Save::Idle;
        }
        cx.notify();
    }

    pub(crate) fn ss_roles_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if let Some(id) = self.server_settings.as_mut().and_then(|v| v.roles.pending_open.take()) {
            self.open_role(id, window, cx);
        }
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        if view.roles.open.is_some() {
            return self.ss_role_editor(window, cx);
        }
        let list = match &view.roles.list {
            Load::Loading => return vec![loading_line("Loading roles…", cx)],
            Load::Failed(error) => return vec![failed("Couldn't load roles", error, cx)],
            Load::Ready(list) => list.clone(),
        };
        let guild_id = view.guild_id.clone();
        let query = view.roles.search.read(cx).value().to_lowercase();
        let counts = view.roles.counts.clone();
        let dark = cx.theme().is_dark();
        let everyone = list.iter().find(|r| r.id == guild_id).cloned();
        let ranked: Vec<RoleRow> = list.iter().filter(|r| r.id != guild_id).cloned().collect();

        let default = everyone.map(|r| {
            let id = r.id.clone();
            ui::settings_choice(ui::settings_row("ss-role-everyone", false, cx).child(ui::icon_tile(Glyph::Users, palette::selected(cx), palette::strong(cx)).flex_none()).child(ui::settings_text("Default permissions", Some("@everyone · applies to all server members"), cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)), cx)
                .on_click(cx.listener(move |this, _, window, cx| this.open_role(id.clone(), window, cx)))
        });
        let create = ui::button("ss-role-create", Tone::Primary, cx).small().child(Icon::new(Glyph::Plus).size(ui::ICON_MD)).child("Create role").on_click(cx.listener(|this, _, _, cx| this.create_role(cx)));
        let search = ui::search_field("ss-role-search", &view.roles.search, cx);
        // Drag to reorder while the whole list shows; a filtered list has gaps a drop can't name.
        let sorting = query.is_empty();
        let order: Vec<String> = ranked.iter().map(|r| r.id.clone()).collect();
        let sortable = self.role_board.list(ROLE_LIST, order, window, cx);
        let rows: Vec<AnyElement> = ranked
            .iter()
            .enumerate()
            .filter(|(_, r)| query.is_empty() || r.role.name.to_lowercase().contains(&query))
            .map(|(i, r)| {
                let lock = self.with_access(&guild_id, |me| access::role_lock(me, r.position, r.managed)).unwrap_or(RoleLock::Above);
                let draggable = sorting && self.with_access(&guild_id, |me| access::can_drag_role(me, r.position, false)).unwrap_or(false);
                let color = if r.role.color == 0 { palette::faint(cx) } else { palette::role_ink(r.role.color, dark) };
                let count = counts.get(&r.id).map(|n| plural(*n, "member", "members"));
                let id = r.id.clone();
                // A grip on the rows that drag; the rest keep its slot so every name lines up.
                let grip = div().w_4().flex_none().flex().justify_center().when(draggable, |g| g.cursor_grab().text_color(palette::faint(cx)).child(Icon::new(Glyph::Grip).size(ui::ICON_MD)));
                let row = ui::settings_row(SharedString::from(format!("ss-role-{}", r.id)), i > 0 || default_shown(&list, &guild_id), cx)
                    .child(grip)
                    .child(div().size_3().m_0p5().rounded_full().bg(color).flex_none())
                    .child(ui::settings_text(r.role.name.clone(), count.or_else(|| lock.reason().map(str::to_owned)), cx))
                    .child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD));
                let key = format!("ss-role-{}", r.id);
                let carried = self.drag_motion.row(&key) != RowDrag::Rest;
                let row = ui::settings_choice(row, cx)
                    .children(self.drag_motion.spot(&key))
                    .when(carried, |r| r.opacity(0.))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_role(id.clone(), window, cx)));
                let row = match draggable {
                    true => {
                        let size = self.role_board.slot(&r.id).map_or(gpui_kit::point(px(480.), px(56.)), |b| gpui_kit::point(b.size.width, b.size.height));
                        let ghost = crate::drag::pick_up(cx.weak_entity(), key.into(), Look::Label { dot: color, text: r.role.name.clone().into() }, size, 1.03, 0., true);
                        let row = row.on_drag(RoleDrag, Board::lift(cx.weak_entity(), role_board, r.id.clone(), ghost));
                        Board::target(row, role_board, ROLE_LIST, r.id.clone(), |_: &RoleDrag| false, cx)
                    }
                    false => row,
                };
                sortable.item(&r.id, row)
            })
            .collect();
        let empty = rows.is_empty().then(|| ui::settings_caption(if query.is_empty() { "No roles yet. Create one to group members and give them permissions." } else { "No roles match." }, cx));
        vec![
            ui::settings_card(cx).children(default).into_any_element(),
            div().flex().gap_3().items_center().child(div().flex_1().child(search)).child(create).into_any_element(),
            ui::settings_group("Roles", cx).child(ui::settings_card(cx).id("ss-roles").children(rows)).children(empty).child(ui::settings_caption("Members use the colour of their highest role. You can only move roles below your own highest role.", cx)).into_any_element(),
        ]
    }

    fn create_role(&mut self, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.server_call(api::create_role(&guild_id), move |this, result, cx| match result {
            Ok(body) => {
                let Some(row) = parse_role(&body) else { return };
                let id = row.id.clone();
                if let Some(view) = &mut this.server_settings {
                    if let Load::Ready(list) = &mut view.roles.list {
                        list.push(row);
                        *list = sorted(std::mem::take(list));
                    }
                    view.roles.pending_open = Some(id);
                }
            }
            Err(error) => this.show_toast(error, cx),
        }, cx);
    }

    // A role dragged in the list, dropped beside another.
    pub(crate) fn finish_role_sort(&mut self, done: Finished, cx: &mut Context<Self>) {
        let Some(hint) = done.hint.filter(|h| h.list == ROLE_LIST) else { return };
        let Some(target) = hint.target else { return };
        self.move_role_beside(&done.dragged, &target, hint.place, cx);
    }

    // Moves the role `id` just above (Before) or below (After) `target`, never under @everyone; Discord's
    // answer sticks, Undo puts it back.
    fn move_role_beside(&mut self, id: &str, target: &str, place: Place, cx: &mut Context<Self>) {
        let Some(view) = &mut self.server_settings else { return };
        let guild_id = view.guild_id.clone();
        let Load::Ready(list) = &mut view.roles.list else { return };
        if id == target || target == guild_id {
            return;
        }
        let before: Vec<(String, i64)> = list.iter().map(|r| (r.id.clone(), r.position)).collect();
        let Some(i) = list.iter().position(|r| r.id == id) else { return };
        let moved = list.remove(i);
        let Some(at) = list.iter().position(|r| r.id == target) else {
            list.insert(i, moved);
            return;
        };
        let j = if place == Place::After { at + 1 } else { at };
        list.insert(j, moved);
        if list.iter().map(|r| r.id.as_str()).eq(before.iter().map(|(id, _)| id.as_str())) {
            return;
        }
        let order: Vec<(&str, i64)> = list.iter().map(|r| (r.id.as_str(), r.position)).collect();
        let Some(call) = api::reorder_roles(&guild_id, &order) else { return };
        let bottom = list.len() as i64 - 1;
        for (k, row) in list.iter_mut().enumerate() {
            row.position = bottom - k as i64;
        }
        let now: HashMap<String, i64> = list.iter().map(|r| (r.id.clone(), r.position)).collect();
        let back: Vec<(&str, i64)> = before.iter().map(|(id, _)| (id.as_str(), now[id])).collect();
        let undo = api::reorder_roles(&guild_id, &back);
        let name = list[j].role.name.clone();
        cx.notify();
        self.server_call(call, move |this, result, cx| match result {
            Ok(_) => {
                if let Some(undo) = undo {
                    this.offer_undo(format!("Moved {name}"), undo, cx);
                }
            }
            Err(error) => {
                this.show_toast(error, cx);
                this.load_roles(cx);
            }
        }, cx);
    }

    fn ss_role_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let Some(edit) = view.roles.open.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let is_everyone = edit.id == guild_id;
        let row = match &view.roles.list {
            Load::Ready(list) => list.iter().find(|r| r.id == edit.id).cloned(),
            _ => None,
        };
        let Some(row) = row else { return Vec::new() };
        let lock = self.with_access(&guild_id, |me| access::role_lock(me, row.position, row.managed)).unwrap_or(RoleLock::Above);
        let tab = edit.tab;
        let back = ui::button("ss-role-back", Tone::Ghost, cx).small().child(Icon::new(Glyph::ChevronLeft).size(ui::ICON_MD)).child("Roles").on_click(cx.listener(|this, _, window, cx| {
            this.reset_role_draft(window, cx);
            if let Some(view) = &mut this.server_settings {
                view.roles.open = None;
            }
            cx.notify();
        }));
        let title = div().text_size(ui::TEXT_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).truncate().child(if is_everyone { "@everyone".to_owned() } else { format!("Edit role · {}", row.role.name) });
        let mut tabs = vec![ui::Tab::new("Permissions")];
        if !is_everyone {
            tabs.insert(0, ui::Tab::new("Display"));
            tabs.push(ui::Tab::new("Members"));
        }
        let all = if is_everyone { vec![RoleTab::Permissions] } else { vec![RoleTab::Display, RoleTab::Permissions, RoleTab::Members] };
        let tab = if all.contains(&tab) { tab } else { all[0] };
        let selected = all.iter().position(|t| *t == tab).unwrap_or(0);
        let this = cx.entity();
        let strip = ui::tabs("ss-role-tabs", &tabs, selected, move |i, _, cx| {
            let pick = all[i];
            this.update(cx, |this, cx| {
                if let Some(edit) = this.server_settings.as_mut().and_then(|v| v.roles.open.as_mut()) {
                    edit.tab = pick;
                }
                cx.notify();
            })
        }, window, cx);
        let mut out = vec![div().flex().flex_col().gap_2().child(div().flex().items_center().gap_2().child(back).child(title)).child(strip).into_any_element()];
        if let Some(reason) = lock.reason().filter(|_| lock == RoleLock::Above) {
            out.push(ui::callout(Notice::Info, Glyph::Shield, "View only", reason, cx).into_any_element());
        }
        let editable = lock != RoleLock::Above;
        out.extend(match tab {
            RoleTab::Display => self.ss_role_display(&row, editable, lock == RoleLock::Managed, window, cx),
            RoleTab::Permissions => self.ss_role_permissions(editable, window, cx),
            RoleTab::Members => self.ss_role_members(&row, editable && lock != RoleLock::Managed, cx),
        });
        out
    }

    fn ss_role_display(&mut self, row: &RoleRow, editable: bool, managed: bool, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let Some(edit) = view.roles.open.as_ref() else { return Vec::new() };
        let draft = edit.draft.clone();
        let guild_id = view.guild_id.clone();
        let gradient = self.with_access(&guild_id, |me| me.feature(access::ENHANCED_ROLE_COLORS.0)).unwrap_or(false);
        let focus = view.roles.name.read(cx).focus_handle(cx);
        let name = ui::field("Role name", cx).child(ui::text_field("ss-role-name", Input::new(&view.roles.name).appearance(false), &focus).disabled(!editable));
        let ring = palette::surface(palette::Level::Group, cx);
        let swatches = |slot: u8, current: Option<u32>, cx: &mut Context<Self>| {
            let none = ui::swatch(SharedString::from(format!("ss-swatch-{slot}-none")), palette::faint(cx), current.is_none_or(|c| c == 0), ring).on_click(cx.listener(move |this, _, _, cx| this.edit_role_draft(cx, |d| set_color(d, slot, None))));
            let rest = COLORS.map(|c| ui::swatch(SharedString::from(format!("ss-swatch-{slot}-{c:06x}")), gpui_kit::rgb(c).into(), current == Some(c), ring).on_click(cx.listener(move |this, _, _, cx| this.edit_role_draft(cx, |d| set_color(d, slot, Some(c))))).into_any_element());
            // Ten to a row, as Discord lays them out.
            div().max_w(px(400.)).flex().flex_wrap().gap_2().child(none).children(rest)
        };
        let primary = swatches(0, Some(draft.color), cx);
        let mut colour = ui::settings_card(cx).p_4().gap_3().child(ui::settings_text("Colour", Some("Members show the colour of their highest coloured role"), cx)).child(primary);
        if gradient {
            colour = colour.child(ui::settings_text("Gradient", Some("A second colour fades the name from one to the other"), cx)).child(swatches(1, draft.secondary_color, cx));
        } else {
            colour = colour.child(ui::settings_caption("Gradient and holographic styles unlock at boost level 2.", cx));
        }
        let hoist = ui::settings_switch("ss-role-hoist", "Show members separately", "They get their own group in the member list", draft.hoist, false, window, cx).on_click(cx.listener(|this, _, _, cx| this.edit_role_draft(cx, |d| d.hoist = !d.hoist)));
        let mention = ui::settings_switch("ss-role-mention", "Let anyone @mention this role", "Members with Mention @everyone can always mention it", draft.mentionable, true, window, cx).on_click(cx.listener(|this, _, _, cx| this.edit_role_draft(cx, |d| d.mentionable = !d.mentionable)));
        let mut out = vec![
            ui::settings_card(cx).p_4().child(name).into_any_element(),
            ui::settings_group("Style", cx).child(colour).into_any_element(),
            ui::settings_group("Member list", cx).child(ui::settings_card(cx).child(hoist).child(mention)).into_any_element(),
        ];
        if editable && !managed {
            out.push(self.ss_role_delete(row, cx));
        }
        out
    }

    fn ss_role_delete(&self, row: &RoleRow, cx: &mut Context<Self>) -> AnyElement {
        let asking = self.server_settings.as_ref().and_then(|v| v.roles.open.as_ref()).is_some_and(|e| e.asking_delete);
        let ask = cx.listener(|this, _, _, cx| {
            if let Some(edit) = this.server_settings.as_mut().and_then(|v| v.roles.open.as_mut()) {
                edit.asking_delete = !edit.asking_delete;
            }
            cx.notify();
        });
        if !asking {
            return ui::settings_card(cx).child(ui::settings_row("ss-role-delete", false, cx).child(ui::settings_text("Delete role", Some("Everyone who has it loses it"), cx)).child(ui::button("ss-role-delete-ask", Tone::Danger, cx).small().child("Delete role").on_click(ask))).into_any_element();
        }
        let id = row.id.clone();
        let keep = ui::button("ss-role-keep", Tone::Secondary, cx).small().child("Keep role").on_click(ask);
        let delete = ui::button("ss-role-delete-yes", Tone::Danger, cx).small().child(format!("Delete {}", row.role.name)).on_click(cx.listener(move |this, _, _, cx| {
            let Some(guild_id) = this.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
            let id = id.clone();
            this.server_call(api::delete_role(&guild_id, &id), move |this, result, cx| match result {
                Ok(_) => {
                    if let Some(view) = &mut this.server_settings {
                        view.roles.open = None;
                        if let Load::Ready(list) = &mut view.roles.list {
                            list.retain(|r| r.id != id);
                        }
                    }
                    if let Some(guild) = this.guilds.iter_mut().find(|g| g.id == guild_id) {
                        guild.roles.retain(|r| r.id != id);
                    }
                }
                Err(error) => this.show_toast(error, cx),
            }, cx);
        }));
        let count = self.server_settings.as_ref().and_then(|v| v.roles.counts.get(&row.id).copied()).unwrap_or(0);
        ui::callout_with(Notice::Danger, Glyph::Trash, format!("Delete {}?", row.role.name), format!("{} lose it. This can't be undone.", plural(count, "member", "members")), div().flex().gap_2().child(keep).child(delete), cx).into_any_element()
    }

    fn ss_role_permissions(&mut self, editable: bool, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let Some(edit) = view.roles.open.as_ref() else { return Vec::new() };
        let (bits, guild_id) = (edit.draft.permissions, view.guild_id.clone());
        let query = view.roles.perm_search.read(cx).value().to_string();
        let search = ui::search_field("ss-perm-search", &view.roles.perm_search, cx);
        let clear = ui::button("ss-perm-clear", Tone::Ghost, cx).small().disabled(!editable || bits == 0).child("Clear permissions").on_click(cx.listener(|this, _, _, cx| this.edit_role_draft(cx, |d| d.permissions = 0)));
        let mut out = vec![div().flex().items_center().gap_3().child(div().flex_1().child(search)).child(clear).into_any_element()];
        let groups = perms::search(&query);
        if groups.is_empty() {
            out.push(ui::settings_caption("No permissions match.", cx).into_any_element());
        }
        for (title, items) in groups {
            let rows: Vec<AnyElement> = items
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let (bit, on) = (p.bit, bits & p.bit != 0);
                    let allowed = editable && self.with_access(&guild_id, |me| access::can_toggle_permission(me, bit)).unwrap_or(false);
                    let detail = if allowed || !editable { p.detail.to_owned() } else { format!("{} You can only grant permissions you have.", p.detail) };
                    let row = ui::settings_row(SharedString::from(format!("ss-perm-{bit}")), i > 0, cx).child(ui::settings_text(p.name, Some(detail), cx)).child(ui::switch_live(SharedString::from(format!("ss-perm-switch-{bit}")), on, window, cx).opacity(if allowed { 1. } else { 0.5 }));
                    if allowed {
                        ui::settings_choice(row, cx).on_click(cx.listener(move |this, _, _, cx| this.edit_role_draft(cx, |d| d.permissions ^= bit))).into_any_element()
                    } else {
                        row.into_any_element()
                    }
                })
                .collect();
            out.push(ui::settings_group(title, cx).child(ui::settings_card(cx).children(rows)).into_any_element());
        }
        if bits & perm::ADMINISTRATOR != 0 {
            out.insert(1, ui::callout(Notice::Warning, Glyph::Warning, "Administrator is on", "This role has every permission and ignores channel overrides.", cx).into_any_element());
        }
        out
    }

    fn ss_role_members(&mut self, row: &RoleRow, editable: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let Some(edit) = view.roles.open.as_ref() else { return Vec::new() };
        let guild_id = view.guild_id.clone();
        let ids = match &edit.members {
            Load::Loading => return vec![loading_line("Loading members…", cx)],
            Load::Failed(error) => return vec![failed("Couldn't load this role's members", error, cx)],
            Load::Ready(ids) => ids.clone(),
        };
        if ids.is_empty() {
            return vec![ui::settings_caption("Nobody has this role yet. Give it to members from their profile or the Members page.", cx).into_any_element()];
        }
        let known = self.members.get(&guild_id).cloned().unwrap_or_default();
        let rows: Vec<AnyElement> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let user = known.get(id);
                let name = user.map(|u| u.name.clone()).unwrap_or_else(|| format!("Member {id}"));
                let avatar: AnyElement = match user.map(|u| u.avatar.clone()).filter(|a| !a.is_empty()) {
                    Some(url) => ui::avatar(url, 28.).flex_none().into_any_element(),
                    None => ui::monogram(&name, 28.).flex_none().into_any_element(),
                };
                let (user_id, role_id, role_name, name_for, guild_id) = (id.clone(), row.id.clone(), row.role.name.clone(), name.clone(), guild_id.clone());
                let remove = editable.then(|| {
                    ui::icon_button(SharedString::from(format!("ss-role-member-x-{id}")), Glyph::X, format!("Remove {name} from {}", row.role.name), cx).on_click(cx.listener(move |this, _, _, cx| {
                        let (gid, uid, rid) = (guild_id.clone(), user_id.clone(), role_id.clone());
                        let label = format!("Removed {name_for} from {role_name}");
                        this.server_call(api::remove_member_role(&gid, &uid, &rid), move |this, result, cx| match result {
                            Ok(_) => {
                                if let Some(Load::Ready(ids)) = this.server_settings.as_mut().and_then(|v| v.roles.open.as_mut()).map(|e| &mut e.members) {
                                    ids.retain(|i| *i != uid);
                                }
                                this.offer_undo(label, api::add_member_role(&gid, &uid, &rid), cx);
                            }
                            Err(error) => this.show_toast(error, cx),
                        }, cx);
                    }))
                });
                ui::settings_row(SharedString::from(format!("ss-role-member-{id}")), i > 0, cx).child(avatar).child(ui::settings_text(name, None::<&str>, cx)).children(remove).into_any_element()
            })
            .collect();
        vec![ui::settings_group(plural(ids.len() as u64, "member", "members"), cx).child(ui::settings_card(cx).children(rows)).into_any_element()]
    }
}

fn default_shown(list: &[RoleRow], guild_id: &str) -> bool {
    !list.iter().any(|r| r.id == guild_id)
}

// Slot 0 is the main colour; slot 1 the gradient's second. Clearing the main colour clears the gradient too.
fn set_color(draft: &mut RoleDraft, slot: u8, color: Option<u32>) {
    match slot {
        0 => {
            draft.color = color.unwrap_or(0);
            if color.is_none() {
                (draft.secondary_color, draft.tertiary_color) = (None, None);
            }
        }
        _ => draft.secondary_color = color,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn roles_sort_highest_first() {
        let roles: Vec<RoleRow> = [("a", 1), ("b", 3), ("g", 0), ("c", 3)].iter().filter_map(|(id, p)| parse_role(&json!({ "id": id, "position": p, "name": id, "permissions": "0" }))).collect();
        assert_eq!(sorted(roles).iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["b", "c", "a", "g"]);
    }

    #[test]
    fn clearing_colour_clears_gradient() {
        let mut d = RoleDraft { color: 5, secondary_color: Some(6), ..Default::default() };
        set_color(&mut d, 1, Some(7));
        assert_eq!(d.secondary_color, Some(7));
        set_color(&mut d, 0, None);
        assert_eq!((d.color, d.secondary_color), (0, None));
    }
}
