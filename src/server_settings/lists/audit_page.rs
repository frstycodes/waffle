// The audit log page: its filters, the entries as rows that open to show what changed, and Load more.
use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    component::Icon,
    div, px,
};

use crate::{
    Waffle,
    icons::Glyph,
    palette,
    ui::{self, Tone},
};

use super::super::{
    audit::{self, Directory},
    view::{Select, failed, loading_line, sentence_spans, snowflake_time},
};

// An audit entry's detail lines hang under its title: indented past the icon.
const DETAIL_INDENT: f32 = 30.;
// The width of an audit detail's label column.
const DETAIL_LABEL_WIDTH: f32 = 150.;

impl Waffle {
    pub(super) fn ss_audit_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let a = &view.lists.audit;
        let mut users: Vec<(String, String)> = vec![(String::new(), "All members".into())];
        let mut known: Vec<(String, String)> = a.names.users.iter().map(|(id, n)| (id.clone(), n.clone())).collect();
        known.sort_by_key(|x| x.1.to_lowercase());
        users.extend(known);
        let mut actions: Vec<(String, String)> = vec![(String::new(), "All actions".into())];
        actions.extend(audit::ACTIONS.iter().map(|(n, l)| (n.to_string(), (*l).to_owned())));
        let user = a.query.user_id.clone().unwrap_or_default();
        let action = a.query.action_type.map(|n| n.to_string()).unwrap_or_default();
        let filters = ui::row().items_stretch()
            .gap_3()
            .child(div().flex_1().child(ui::field("Filter by member", cx).child(self.ss_select(Select::AuditUser, &users, &user, cx))))
            .child(div().flex_1().child(ui::field("Filter by action", cx).child(self.ss_select(Select::AuditAction, &actions, &action, cx))));
        let Some(view) = self.server_settings.as_ref() else { return Vec::new() };
        let a = &view.lists.audit;
        let mut out = vec![filters.into_any_element()];
        if let Some(error) = &a.error {
            out.push(failed("Couldn't load the audit log", error, cx));
        }
        if a.entries.is_empty() && a.loading {
            out.push(loading_line("Loading the audit log…", cx));
            return out;
        }
        if a.entries.is_empty() && a.error.is_none() {
            out.push(ui::settings_caption("Nothing here. Discord keeps 45 days of changes.", cx).into_any_element());
            return out;
        }
        let rows: Vec<AnyElement> = a.entries.iter().enumerate().map(|(i, e)| self.ss_audit_row(e, &a.names, a.open.contains(&e.id), i > 0, cx)).collect();
        out.push(ui::settings_card(cx).id("ss-audit").children(rows).into_any_element());
        if a.next.is_some() {
            out.push(ui::row().items_stretch().justify_center().child(ui::button("ss-audit-more", Tone::Secondary, cx).loading(a.loading.then_some("Loading")).child("Load more").on_click(cx.listener(|this, _, _, cx| {
                if let Some(l) = this.lists() {
                    l.audit.query.before = l.audit.next.take();
                    l.audit.loading = true;
                }
                this.load_audit(cx);
            }))).into_any_element());
        }
        out
    }

    fn ss_audit_row(&self, e: &audit::Entry, names: &Directory, open: bool, line: bool, cx: &mut Context<Self>) -> AnyElement {
        let (glyph, ink) = match audit::tone(e.action) {
            audit::Tone::Create => (Glyph::PlusCircle, palette::status_in(crate::discord::Status::Online, cx)),
            audit::Tone::Update => (Glyph::Pen, palette::muted(cx)),
            audit::Tone::Delete => (Glyph::MinusBold, palette::danger_ink(cx)),
        };
        let details = audit::details(e, names);
        let id = e.id.clone();
        let chevron = (!details.is_empty()).then(|| div().text_color(palette::muted(cx)).child(Icon::new(if open { Glyph::ChevronDown } else { Glyph::ChevronRight }).size(ui::ICON_MD)));
        let head = ui::row()
            .items_start()
            .gap_3()
            .w_full()
            .child(div().pt_0p5().text_color(ink).flex_none().child(Icon::new(glyph).size(ui::ICON_LG)))
            .child(ui::col().min_h_0().flex_1().gap_0p5().child(sentence_spans(&audit::sentence(e, names), cx)).child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(snowflake_time(&e.id))))
            .children(chevron);
        let body = open.then(|| {
            ui::col().pl(px(DETAIL_INDENT)).pt_2().gap_1().children(details.into_iter().map(|d| {
                let change = match (&d.old, &d.new) {
                    (Some(old), Some(new)) => ui::row().gap_1p5().child(div().text_color(palette::muted(cx)).line_through().child(old.clone())).child(Icon::new(Glyph::ArrowRight).size(ui::ICON_XS)).child(div().text_color(palette::strong(cx)).child(new.clone())),
                    (None, Some(new)) => div().text_color(palette::strong(cx)).child(new.clone()),
                    (Some(old), None) => div().text_color(palette::muted(cx)).line_through().child(old.clone()),
                    (None, None) => div().text_color(palette::muted(cx)).child("Cleared"),
                };
                ui::row().items_stretch().gap_2().text_size(ui::TEXT_SM).child(div().w(px(DETAIL_LABEL_WIDTH)).flex_none().text_color(palette::muted(cx)).child(d.label)).child(div().flex_1().min_w_0().child(change))
            }))
        });
        let row = ui::settings_row(SharedString::from(format!("ss-audit-{}", e.id)), line, cx).flex_col().items_stretch().child(head).children(body);
        ui::settings_choice(row, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(l) = this.lists()
                    && !l.audit.open.remove(&id) {
                        l.audit.open.insert(id.clone());
                    }
                cx.notify(); // lint-allow(global-notify): the server settings window is built by the root render over the columns, so every column's draw reads it
            }))
            .into_any_element()
    }
}
