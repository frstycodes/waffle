// The card an @role mention opens to its right (Discord's role popout): the role, its member count, and
// the members loaded here who have it, as member-list rows.
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::{AnyElement, Context, FontWeight, MouseButton, Pixels, Point, SharedString, Window, div, img, point, prelude::*, px};

use crate::{
    Waffle,
    discord::{Member, User},
    palette,
    panel::RowSpot,
    ui::{self, SURFACE_INSET},
};

const WIDTH: f32 = 260.;
// Discord refetches a server's role counts after 120 s, and a role's member ids after 10 s.
const COUNTS_FRESH: Duration = Duration::from_secs(120);
const IDS_FRESH: Duration = Duration::from_secs(10);
// GAP: Discord renders every cached member; past this many rows the rest count into "+N more".
const MAX_ROWS: usize = 100;
// Discord's user popout opens this far off the card.
const PROFILE_GAP: f32 = 14.;

#[derive(Default)]
pub(crate) struct RolePopouts {
    open: Option<Open>,
    // The role whose member ids are loading; the card opens when they land, unless something else did.
    pending: Option<(String, String)>,
    counts: HashMap<String, (Instant, HashMap<String, u32>)>,
    ids_fetched: HashMap<(String, String), Instant>,
}

struct Open {
    guild_id: String,
    // None is @everyone.
    role_id: Option<String>,
    at: Point<Pixels>,
}

impl RolePopouts {
    /// Closes the card and forgets a pending one; true if either was there.
    pub(crate) fn close(&mut self) -> bool {
        self.open.take().is_some() | self.pending.take().is_some()
    }

    /// A server's role counts as a fetch would leave them, for the demo, which stays offline.
    pub(crate) fn seed_counts(&mut self, guild_id: &str, counts: HashMap<String, u32>) {
        self.counts.insert(guild_id.to_owned(), (Instant::now(), counts));
    }
}

impl Waffle {
    /// Opens the card for a role (None: @everyone) beside `at`, after loading its members like Discord does.
    pub(crate) fn open_role_popout(&mut self, role_id: Option<String>, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(guild) = self.guild() else { return };
        let guild_id = guild.id.clone();
        // The @everyone role's id is the server's.
        let role_id = role_id.filter(|id| *id != guild_id);
        if role_id.as_ref().is_some_and(|id| !guild.roles.iter().any(|r| &r.id == id)) {
            return;
        }
        self.fetch_role_counts(&guild_id, cx);
        let open = Open { guild_id: guild_id.clone(), role_id: role_id.clone(), at };
        let Some(role_id) = role_id else { return self.show_role_popout(open, cx) };
        let key = (guild_id.clone(), role_id.clone());
        if fresh(self.role_popout.ids_fetched.get(&key), IDS_FRESH) {
            return self.show_role_popout(open, cx);
        }
        let Some(client) = self.client.clone() else { return self.show_role_popout(open, cx) };
        self.role_popout.pending = Some(key.clone());
        cx.spawn(async move |this, cx| {
            let ids = client.role_member_ids(&guild_id, &role_id).await;
            this.update(cx, |this, cx| {
                if let Ok(ids) = ids {
                    this.role_popout.ids_fetched.insert(key.clone(), Instant::now());
                    this.request_unknown_members(&guild_id, ids);
                }
                if this.role_popout.pending.as_ref() == Some(&key) {
                    this.role_popout.pending = None;
                    this.show_role_popout(open, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn show_role_popout(&mut self, open: Open, cx: &mut Context<Self>) {
        self.role_popout.open = Some(open);
        cx.notify();
    }

    // Members land through GUILD_MEMBERS_CHUNK, which re-renders the open card.
    fn request_unknown_members(&mut self, guild_id: &str, ids: Vec<String>) {
        let Some(client) = &self.client else { return };
        let known = self.member_roles.get(guild_id);
        let missing: Vec<String> = ids
            .into_iter()
            .filter(|id| !known.is_some_and(|k| k.contains_key(id)))
            .filter(|id| self.requested_members.insert((guild_id.to_owned(), id.clone())))
            .collect();
        for chunk in missing.chunks(100) {
            client.request_members(guild_id, chunk.to_vec());
        }
    }

    fn fetch_role_counts(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        if fresh(self.role_popout.counts.get(guild_id).map(|(at, _)| at), COUNTS_FRESH) {
            return;
        }
        let Some(client) = self.client.clone() else { return };
        let guild_id = guild_id.to_owned();
        cx.spawn(async move |this, cx| {
            let Ok(counts) = client.role_member_counts(&guild_id).await else { return };
            this.update(cx, |this, cx| {
                this.role_popout.counts.insert(guild_id, (Instant::now(), counts));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn role_popout_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.role_popout.open.as_ref()?;
        let guild = self.guilds.iter().find(|g| g.id == open.guild_id)?;
        let role = match &open.role_id {
            Some(id) => Some(guild.roles.iter().find(|r| &r.id == id)?),
            None => None,
        };
        let count = role.and_then(|r| self.role_popout.counts.get(&guild.id)?.1.get(&r.id).copied());
        let roles = self.member_roles.get(&guild.id);
        // Members and their presence come with READY; before it, listing them would claim they're offline.
        let users = role_rows(self.members.get(&guild.id).filter(|_| self.live()), roles, role.map(|r| r.id.as_str()));
        let shown = &users[..users.len().min(MAX_ROWS)];
        let more = more_count(count, shown.len());
        let title = role.map_or("@everyone", |r| r.name.as_str());
        let spot = RowSpot {
            width: WIDTH - 2. * SURFACE_INSET,
            surface: palette::surface(palette::Level::Overlay, cx),
            profile_x: Some(open.at.x + px(WIDTH + PROFILE_GAP)),
        };
        let rows = shown.iter().map(|user| {
            let member = Member {
                user: (*user).clone(),
                roles: roles.and_then(|r| r.get(&user.id)).cloned().unwrap_or_default(),
                presence: self.presences.get(&user.id).cloned().unwrap_or_default(),
                boosting_since: user.boosting_since.clone(),
            };
            self.member_row(&member, spot, cx)
        });
        let rows: Vec<AnyElement> = rows.collect();
        let head = div()
            .flex()
            .items_center()
            .gap_1p5()
            .px_2()
            .pt_1p5()
            .pb_2()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::strong(cx))
            .children(role.and_then(|r| r.icon.clone()).map(|icon| img(icon).size_4().flex_none()))
            .child(div().min_w_0().truncate().child(title.to_owned()))
            .children(count.map(|n| div().flex_none().font_weight(FontWeight::NORMAL).text_color(palette::faint(cx)).child(format!("\u{a0}— {}", crate::link_preview::grouped(n.into())))));
        let top = crate::layout::TITLE_HEIGHT + 8.;
        let max_height = f32::from(window.viewport_size().height) - top - 8.;
        let card = ui::surface(cx)
            .id("role-popout")
            .occlude()
            .w(px(WIDTH))
            .max_h(px(max_height))
            .gap_0p5()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(head)
            .child(div().id("role-popout-rows").flex_shrink(1.).min_h_0().overflow_y_scroll().flex().flex_col().gap_0p5().children(rows))
            .children(more.map(|n| div().px_2().py_1p5().text_size(ui::TEXT_SM).text_color(palette::faint(cx)).child(format!("+{} more", crate::link_preview::grouped(n.into())))));
        let key = SharedString::from(format!("role-popout-{}", open.role_id.as_deref().unwrap_or("everyone")));
        // lint-allow(raw-floating): placed inside the overlay backdrop, with a measured top margin floating_at can't take.
        let placed = gpui_kit::anchored()
            .position(open.at)
            .snap_to_window_with_margin(gpui_kit::Edges { top: px(top), right: px(8.), bottom: px(8.), left: px(8.) })
            .child(ui::rise(card, key, Duration::ZERO));
        Some(crate::overlays::backdrop("role-popout-backdrop", cx).child(placed).into_any_element())
    }
}

/// Where the card opens for a mention drawn at `bounds`: just off its right edge, level with its top.
pub(crate) fn beside(bounds: gpui_kit::Bounds<Pixels>) -> Point<Pixels> {
    point(bounds.right() + px(8.), bounds.top())
}

fn fresh(fetched: Option<&Instant>, ttl: Duration) -> bool {
    fetched.is_some_and(|at| at.elapsed() < ttl)
}

// The loaded members with the role (everyone for @everyone), by the name the server shows them under:
// Discord sorts on nickname-or-display-name in plain string order, case and all. A member whose roles
// aren't known yet is left out of a role's list.
fn role_rows<'a>(members: Option<&'a HashMap<String, User>>, roles: Option<&HashMap<String, Vec<String>>>, role_id: Option<&str>) -> Vec<&'a User> {
    let has_role = |user: &User| match role_id {
        None => true,
        Some(id) => roles.and_then(|r| r.get(&user.id)).is_some_and(|theirs| theirs.iter().any(|r| r == id)),
    };
    let mut rows: Vec<&User> = members.into_iter().flat_map(|m| m.values()).filter(|u| has_role(u)).collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

// "+N more" under the rows, only while the count runs past them.
fn more_count(count: Option<u32>, shown: usize) -> Option<u32> {
    count?.checked_sub(shown as u32).filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, name: &str) -> User {
        User { id: id.into(), name: name.into(), ..Default::default() }
    }

    #[test]
    fn rows_keep_the_role_and_sort_by_shown_name() {
        let members: HashMap<String, User> = [user("1", "mira"), user("2", "Theo"), user("3", "Ana"), user("4", "Zed"), user("5", "bob")].into_iter().map(|u| (u.id.clone(), u)).collect();
        let roles: HashMap<String, Vec<String>> = [("1", vec!["mod"]), ("2", vec!["mod", "x"]), ("3", vec!["mod"]), ("4", vec!["x"]), ("5", vec!["mod"])].into_iter().map(|(id, r)| (id.into(), r.into_iter().map(Into::into).collect())).collect();
        let names = |role| role_rows(Some(&members), Some(&roles), role).into_iter().map(|u| u.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names(Some("mod")), ["Ana", "Theo", "bob", "mira"], "plain string order: capitals first");
        assert_eq!(names(None), ["Ana", "Theo", "Zed", "bob", "mira"], "@everyone lists everyone loaded");
        assert!(names(Some("none")).is_empty());
    }

    #[test]
    fn unknown_roles_drop_the_member() {
        let members: HashMap<String, User> = [(String::from("1"), user("1", "Ana"))].into();
        assert!(role_rows(Some(&members), None, Some("mod")).is_empty());
        assert_eq!(role_rows(Some(&members), None, None).len(), 1, "but @everyone still lists them");
        assert!(role_rows(None, None, None).is_empty());
    }

    #[test]
    fn more_counts_only_what_runs_past_the_rows() {
        assert_eq!(more_count(Some(14), 5), Some(9));
        assert_eq!(more_count(Some(5), 5), None);
        assert_eq!(more_count(Some(3), 5), None, "a stale count under the rows shows nothing");
        assert_eq!(more_count(None, 5), None, "@everyone has no count");
    }

    #[test]
    fn counts_go_stale() {
        assert!(fresh(Some(&Instant::now()), IDS_FRESH));
        assert!(!fresh(None, IDS_FRESH));
        assert!(!fresh(Instant::now().checked_sub(Duration::from_secs(11)).as_ref(), IDS_FRESH));
    }
}
