// Reporting a message or a person to Discord (docs/discord-report-re.md, design/explore-report.html). The flow is a
// menu Discord's server sends: screens (nodes) of reasons, each reason a step to the next, ending at a Submit node
// and then the thank-you node. Waffle draws whichever nodes come, in its own look; element kinds Discord only sends
// to signed-out reporters (free text, dropdowns, content URLs) and the settings upsells aren't drawn.
use std::collections::{HashMap, HashSet};

use gpui_kit::{AnyElement, Context, FontWeight, SharedString, Window, component::Icon, div, img, prelude::*, px};
use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::{
    Waffle,
    discord::{Message, Relation, User, perm},
    icons::Glyph,
    illustration::Illustration,
    overlays::backdrop,
    palette,
    ui::{self, Check, RADIUS_MD, Squircle},
};

const DIALOG_WIDTH: f32 = 480.;
const PREVIEW_AVATAR: f32 = 32.;
// The thank-you illustration's size.
const SENT_ART_WIDTH: f32 = 200.;
const SENT_ART_HEIGHT: f32 = 150.;
// Discord's copy when a submit fails for a reason it doesn't name.
const SUBMIT_FAILED: &str = "There was an error trying to submit this report. Please try again!";

#[derive(Clone)]
pub(crate) enum ReportTarget {
    Message(Box<Message>),
    // `guild_id` is the server it was reported from; None from a DM.
    User { user: Box<User>, guild_id: Option<String> },
}

impl ReportTarget {
    fn name(&self) -> &'static str {
        match self {
            Self::Message(_) => "message",
            Self::User { .. } => "user",
        }
    }

    fn person(&self) -> &User {
        match self {
            Self::Message(m) => &m.author,
            Self::User { user, .. } => user,
        }
    }

    fn ids(&self) -> Map<String, Value> {
        let mut ids = Map::new();
        match self {
            Self::Message(m) => {
                ids.insert("channel_id".into(), m.channel_id.clone().into());
                ids.insert("message_id".into(), m.id.clone().into());
            }
            Self::User { user, guild_id } => {
                ids.insert("user_id".into(), user.id.clone().into());
                if let Some(guild_id) = guild_id {
                    ids.insert("guild_id".into(), guild_id.clone().into());
                }
            }
        }
        ids
    }
}

pub(crate) struct ReportMenu {
    root: String,
    success: String,
    nodes: HashMap<String, Node>,
    version: Value,
    variant: Value,
    language: Value,
}

struct Node {
    // As Discord sent it, for the breadcrumbs. Not verified: whether it's a number or a string.
    id: Value,
    header: String,
    subheader: Option<String>,
    info: Option<String>,
    // "next", "submit", "done" or "cancel".
    button: Option<(String, Option<String>)>,
    elements: Vec<Element>,
    // (label, node it leads to)
    children: Vec<(String, String)>, // lint-allow(unbounded-growth): the reasons of one menu node, a few dozen at most
    auto_submit: bool,
}

struct Element {
    kind: String,
    name: String,
    data: Value,
    required: bool,
    require_all: bool,
}

enum Load {
    Loading,
    Failed,
    Ready(Box<ReportMenu>),
}

pub(crate) struct ReportDialog {
    target: ReportTarget,
    menu: Load,
    node: String,
    // The nodes behind this one, each with the reason picked to leave it.
    history: Vec<(String, Option<String>)>,
    // Element name to what's chosen: an array for a checkbox group, a string for a radio group.
    picks: HashMap<String, Value>, // lint-allow(unbounded-growth): one pick per form element of the node on screen, a handful
    pub(crate) also_block: bool,
    sending: bool,
    error: Option<&'static str>,
    // Follow-ups taken from the thank-you node: "block", "ignore", "delete".
    done: HashSet<&'static str>,
}

impl Waffle {
    pub(crate) fn open_report(&mut self, target: ReportTarget, cx: &mut Context<Self>) {
        let name = target.name();
        self.report_dialog = Some(ReportDialog { target, menu: Load::Loading, node: String::new(), history: Vec::new(), picks: HashMap::new(), also_block: false, sending: false, error: None, done: HashSet::new() });
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let menu = client.call(Method::GET, format!("/reporting/menu/{name}"), None).await.ok().and_then(|v| ReportMenu::parse(&v));
            this.update(cx, |this, cx| this.report_menu_loaded(menu, cx)).ok();
        })
        .detach(); // lint-allow(detached-task): one request that ends when the menu lands, holding only a WeakEntity
    }

    pub(crate) fn report_menu_loaded(&mut self, menu: Option<ReportMenu>, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        dialog.menu = match menu {
            Some(menu) => {
                dialog.node = menu.root.clone();
                Load::Ready(Box::new(menu))
            }
            None => Load::Failed,
        };
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    fn close_report(&mut self, cx: &mut Context<Self>) {
        self.report_dialog = None;
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    // A reason or Next: on to `to`, past any node marked to skip. A node that isn't in the menu goes nowhere, as in
    // Discord.
    pub(crate) fn report_step(&mut self, to: String, label: Option<String>, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        let Load::Ready(menu) = &dialog.menu else { return };
        let Some(to) = menu.resolve(to) else { return };
        let auto = menu.nodes[&to].auto_submit;
        dialog.history.push((std::mem::replace(&mut dialog.node, to), label));
        dialog.error = None;
        if auto {
            self.submit_report(cx);
        }
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    fn report_back(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        let Some((node, _)) = dialog.history.pop() else { return };
        dialog.node = node;
        dialog.error = None;
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    fn report_pick(&mut self, name: String, value: String, many: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        if !many {
            dialog.picks.insert(name, value.into());
        } else if let Value::Array(values) = dialog.picks.entry(name).or_insert_with(|| json!([])) {
            match values.iter().position(|v| v.as_str() == Some(&value)) {
                Some(at) => _ = values.remove(at),
                None => values.push(value.into()),
            }
        }
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    fn submit_report(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        let Load::Ready(menu) = &dialog.menu else { return };
        if dialog.sending {
            return;
        }
        let body = menu.submit_body(&dialog.target, &dialog.history, &dialog.node, &dialog.picks);
        let name = dialog.target.name();
        dialog.sending = true;
        dialog.error = None;
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
        // Signed out, as in demo scenes: nothing is sent, and the flow carries on as if Discord took it.
        let Some(client) = self.client.clone() else { return self.report_sent(true, cx) };
        cx.spawn(async move |this, cx| {
            let sent = client.call(Method::POST, format!("/reporting/{name}"), Some(body)).await.is_ok();
            this.update(cx, |this, cx| this.report_sent(sent, cx)).ok();
        })
        .detach(); // lint-allow(detached-task): one request that ends when the report is sent, holding only a WeakEntity
    }

    // Also block, when ticked, goes once Discord has the report.
    pub(crate) fn report_sent(&mut self, sent: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        let Load::Ready(menu) = &dialog.menu else { return };
        dialog.sending = false;
        if !sent {
            dialog.error = Some(SUBMIT_FAILED);
            return cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
        }
        let success = menu.success.clone();
        dialog.history.push((std::mem::replace(&mut dialog.node, success), None));
        let block = dialog.also_block.then(|| dialog.target.person().clone());
        if let Some(user) = block {
            self.report_follow_up("block", user, cx);
        }
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    fn report_follow_up(&mut self, action: &'static str, user: User, cx: &mut Context<Self>) {
        let Some(dialog) = self.report_dialog.as_mut() else { return };
        let message = match &dialog.target {
            ReportTarget::Message(m) => Some((**m).clone()),
            ReportTarget::User { .. } => None,
        };
        if action == "delete" {
            dialog.done.insert(action);
        }
        let id = user.id.clone();
        match action {
            "block" if self.relation(&id) == Some(Relation::Blocked) => self.relation_optimistic(&id, None, Method::DELETE, None, cx),
            "block" => self.relation_optimistic(&id, Some((user, Relation::Blocked)), Method::PUT, Some(json!({ "type": 2 })), cx),
            "ignore" => {
                let ignored = self.ignored.contains(&id);
                self.set_ignored(id, !ignored, cx);
            }
            "delete" => {
                if let Some(m) = message {
                    self.delete(m, cx);
                }
            }
            _ => {}
        }
        cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
    }

    pub(crate) fn report_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dialog = self.report_dialog.as_ref()?;
        let close = ui::icon_button("report-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_report(cx)));
        let body = match &dialog.menu {
            Load::Loading => ui::dialog_body().child(head(SharedString::from("Report"), None, close, cx)).child(
                ui::col().gap_2().children((0..4usize).map(|i| ui::skeleton(("report-loading", i), RADIUS_MD, cx).h_11().w_full())),
            ),
            Load::Failed => ui::dialog_body().child(head(SharedString::from("Report"), None, close, cx)).child(
                ui::empty_state("report-failed", Illustration::LoadFailed, "Couldn't load the report menu", "Discord didn't send it. Close this and try again in a moment.", cx),
            ),
            Load::Ready(menu) => self.report_node(dialog, menu, close, window, cx),
        };
        Some(ui::modal(backdrop("report-backdrop", cx), ui::dialog("report-dialog", DIALOG_WIDTH, body, window, cx), cx))
    }

    fn report_node(&self, dialog: &ReportDialog, menu: &ReportMenu, close: ui::Control, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        let node = &menu.nodes[&dialog.node];
        let has = |kind: &str| node.elements.iter().any(|e| e.kind == kind);
        let success = has("success");
        let ctx = self.row_context_everywhere(&[], DIALOG_WIDTH, cx);
        let markdown = |text: &str, cx: &mut Context<Self>| crate::chat::content(&Message { content: text.to_owned(), ..Default::default() }, &ctx, cx);
        let subheader = node.subheader.as_deref().and_then(|s| markdown(s, cx));
        let mut body = ui::dialog_body();
        if success {
            body = body.child(ui::row().items_stretch().justify_center().pt_2().child(img(Illustration::ReportSent.path()).w(px(SENT_ART_WIDTH)).h(px(SENT_ART_HEIGHT))));
        }
        body = body.child(head(node.header.clone().into(), subheader, close, cx).when(success, |h| h.text_center()));
        if let Some(info) = node.info.as_deref().and_then(|s| markdown(s, cx)) {
            body = body.child(notice(info, cx));
        }
        let mut more = Vec::new();
        for (i, element) in node.elements.iter().enumerate() {
            let id = format!("report-{}-{i}", dialog.node);
            match element.kind.as_str() {
                "message_preview" => body = body.children(self.report_message_preview(dialog, cx)),
                // Discord draws each preview only for its own report type.
                "user_preview" if matches!(dialog.target, ReportTarget::User { .. }) => body = body.child(labelled("Selected user", user_preview(dialog.target.person(), cx), cx)),
                "breadcrumbs" => body = body.children(crumbs(&dialog.history, cx)),
                "checkbox" => body = body.child(self.report_choices(dialog, element, true, &id, window, cx)),
                "radio_group" => body = body.child(self.report_choices(dialog, element, false, &id, window, cx)),
                "text" => body = body.children(text_block(&element.data, cx)),
                "inline_notice" if element.data.get("visible_when").is_none_or(Value::is_null) => {
                    body = body.children(element.data["body"].as_str().and_then(|s| markdown(s, cx)).map(|b| notice(b, cx)));
                }
                "external_link" => body = body.children(link_row(&element.data, &id, cx)),
                kind @ ("block_users" | "ignore_users" | "delete_message") => more.extend(self.report_action(dialog, kind, cx)),
                _ => {}
            }
        }
        if !more.is_empty() {
            body = body.child(labelled("More you can do", ui::col().gap_0p5().mx_neg_2().children(more), cx)); // lint-allow(raw-margin): a negative margin lets the row's hover fill reach the dialog's text edge, which ui::pick_row has no option for
        }
        if !node.children.is_empty() {
            let rows = node.children.iter().enumerate().map(|(i, (label, to))| {
                let (to, label_owned) = (to.clone(), label.clone());
                ui::menu_row(SharedString::from(format!("report-child-{}-{i}", dialog.node)), cx)
                    .min_h_11()
                    .text_size(ui::TEXT_LEAD)
                    .text_color(palette::strong(cx))
                    .child(div().flex_1().min_w_0().child(label.clone()))
                    .child(div().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_LG)))
                    .on_click(cx.listener(move |this, _, _, cx| this.report_step(to.clone(), Some(label_owned.clone()), cx)))
            });
            body = body.child(ui::col().gap_0p5().mx_neg_2().children(rows)); // lint-allow(raw-margin): a negative margin lets the row's hover fill reach the dialog's text edge, which ui::pick_row has no option for
        }
        let submit = node.button.as_ref().is_some_and(|(kind, _)| kind == "submit");
        if submit && !success {
            body = body.child(legal(cx));
        }
        if let Some(error) = dialog.error {
            body = body.child(div().text_color(palette::danger_ink(cx)).child(error));
        }
        body.children(self.report_footer(dialog, node, window, cx))
    }

    fn report_footer(&self, dialog: &ReportDialog, node: &Node, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = node.button.as_ref().map(|(kind, _)| kind.as_str());
        let back = (!dialog.history.is_empty() && kind != Some("done")).then(|| ui::button("report-back", ui::Tone::Secondary, cx).child("Back").on_click(cx.listener(|this, _, _, cx| this.report_back(cx))));
        let primary = match node.button.clone() {
            Some((kind, _)) if kind == "submit" => Some(
                ui::button("report-submit", ui::Tone::Danger, cx)
                    .disabled(!node.ready(&dialog.picks))
                    .loading(dialog.sending.then_some("Submitting"))
                    .child("Submit Report")
                    .on_click(cx.listener(|this, _, _, cx| this.submit_report(cx))),
            ),
            Some((kind, Some(target))) if kind == "next" => Some(
                ui::button("report-next", ui::Tone::Primary, cx).disabled(!node.ready(&dialog.picks)).child("Next").on_click(cx.listener(move |this, _, _, cx| this.report_step(target.clone(), None, cx))),
            ),
            Some((kind, _)) if kind == "cancel" => Some(ui::button("report-cancel", ui::Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_report(cx)))),
            Some(_) => Some(ui::button("report-done", ui::Tone::Primary, cx).child("Done").on_click(cx.listener(|this, _, _, cx| this.close_report(cx)))),
            None => None,
        };
        if back.is_none() && primary.is_none() {
            return None;
        }
        let also_block = (kind == Some("submit") && self.can_also_block(dialog)).then(|| self.also_block_toggle(dialog.also_block, window, cx));
        Some(ui::row().gap_2().pt_1().children(back).child(div().flex_1()).children(also_block).children(primary).into_any_element())
    }

    // Waffle's own (better.html): block on the way out instead of a second step on the thank-you node.
    fn can_also_block(&self, dialog: &ReportDialog) -> bool {
        let person = dialog.target.person();
        !person.bot && self.relation(&person.id) != Some(Relation::Blocked)
    }

    fn also_block_toggle(&self, on: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        ui::row()
            .id("report-also-block")
            .gap_2()
            .mr_2() // lint-allow(raw-margin): spacing from the footer's neighbouring button, whose row sets no gap
            .cursor_pointer()
            .child(ui::check_live("report-also-block-check", if on { Check::On } else { Check::Off }, window, cx))
            .child("Also block")
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(dialog) = this.report_dialog.as_mut() {
                    dialog.also_block = !dialog.also_block;
                }
                cx.notify(); // lint-allow(global-notify): the report dialog is drawn by render_root, not by a column
            }))
            .into_any_element()
    }

    fn report_message_preview(&self, dialog: &ReportDialog, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ReportTarget::Message(m) = &dialog.target else { return None };
        let preview = self.forward_preview(m, cx).unwrap_or_else(|| div().into_any_element());
        let who = ui::row()
            .gap_2()
            .child(ui::avatar(m.author.avatar.clone(), PREVIEW_AVATAR / 1.5))
            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(m.author.name.clone()));
        Some(labelled("Selected message", well(cx).flex_col().items_start().gap_1p5().child(who).child(div().w_full().child(preview)), cx).into_any_element())
    }

    fn report_choices(&self, dialog: &ReportDialog, element: &Element, many: bool, id: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // A checkbox group's data is [[value, label, subtitle, checked by default], ...]; a radio group's is
        // { title, options: [{ label, value }] }.
        let options: Vec<(String, String, Option<String>)> = match many {
            true => element.data.as_array().into_iter().flatten().filter_map(|o| Some((o[0].as_str()?.to_owned(), o[1].as_str()?.to_owned(), o[2].as_str().filter(|s| !s.is_empty()).map(str::to_owned)))).collect(),
            false => element.data["options"].as_array().into_iter().flatten().filter_map(|o| Some((o["value"].as_str()?.to_owned(), o["label"].as_str()?.to_owned(), None))).collect(),
        };
        let picked = dialog.picks.get(&element.name);
        let rows = options.into_iter().enumerate().map(|(i, (value, label, detail))| {
            let on = match picked {
                Some(Value::Array(values)) => values.iter().any(|v| v.as_str() == Some(&value)),
                Some(Value::String(v)) => *v == value,
                _ => false,
            };
            let row_id = SharedString::from(format!("{id}-{i}"));
            let mark = match many {
                true => ui::check_live(SharedString::from(format!("{row_id}-mark")), if on { Check::On } else { Check::Off }, window, cx).into_any_element(),
                false => ui::radio_live(SharedString::from(format!("{row_id}-mark")), on, window, cx).into_any_element(),
            };
            let name = element.name.clone();
            ui::pick_row(row_id, mark, label, detail.map(Into::into), cx).on_click(cx.listener(move |this, _, _, cx| this.report_pick(name.clone(), value.clone(), many, cx)))
        }).collect::<Vec<_>>();
        let title = element.data["title"].as_str().map(|t| div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(t.to_owned()));
        ui::col().gap_0p5().children(title).child(ui::col().gap_0p5().mx_neg_2().children(rows)).into_any_element() // lint-allow(raw-margin): a negative margin lets the row's hover fill reach the dialog's text edge, which ui::pick_row has no option for
    }

    fn report_action(&self, dialog: &ReportDialog, kind: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let person = dialog.target.person().clone();
        let name = &person.name;
        let (action, glyph, title, detail, verb, undo) = match kind {
            "block_users" => ("block", Glyph::Prohibit, format!("Block {name}"), "Stop seeing their messages", "Block", "Unblock"),
            "ignore_users" => ("ignore", Glyph::EyeSlash, format!("Ignore {name}"), "Get space without letting them know", "Ignore", "Unignore"),
            // Discord offers it only where you could delete the message yourself.
            "delete_message" => {
                let ReportTarget::Message(m) = &dialog.target else { return None };
                if !self.can_in(&m.channel_id, perm::MANAGE_MESSAGES) {
                    return None;
                }
                ("delete", Glyph::Trash, "Delete message".into(), "Remove it from the channel", "Delete", "Deleted")
            }
            _ => return None,
        };
        let active = match action {
            "block" => self.relation(&person.id) == Some(Relation::Blocked),
            "ignore" => self.ignored.contains(&person.id),
            _ => dialog.done.contains(action),
        };
        // Block and ignore toggle so a slip can be undone here; a deleted message can't come back.
        let deleted = action == "delete" && active;
        let button = ui::button(SharedString::from(format!("report-{action}")), ui::Tone::Secondary, cx).small().disabled(deleted).child(if active { undo } else { verb });
        let button = match deleted {
            true => button,
            false => button.on_click(cx.listener(move |this, _, _, cx| this.report_follow_up(action, person.clone(), cx))),
        };
        Some(
            ui::row()
                .gap_3()
                .p_2()
                .child(ui::icon_tile(glyph, palette::selected(cx), palette::strong(cx)))
                .child(ui::col().flex_1().child(ui::line(title).text_color(palette::strong(cx))).child(div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(detail)))
                .child(button)
                .into_any_element(),
        )
    }
}

impl ReportMenu {
    pub(crate) fn parse(v: &Value) -> Option<Self> {
        let nodes: HashMap<String, Node> = v["nodes"].as_object()?.values().filter_map(Node::parse).map(|n| (key(&n.id), n)).collect();
        let root = key(&v["root_node_id"]);
        nodes.contains_key(&root).then(|| Self { root, success: key(&v["success_node_id"]), nodes, version: v["version"].clone(), variant: v["variant"].clone(), language: v["language"].clone() })
    }

    // Where a step to `to` lands: past nodes that only skip ahead (a `skip` element and a Next button). None when
    // it isn't in the menu.
    fn resolve(&self, mut to: String) -> Option<String> {
        for _ in 0..self.nodes.len() {
            let node = self.nodes.get(&to)?;
            match &node.button {
                Some((kind, Some(target))) if kind == "next" && node.elements.iter().any(|e| e.kind == "skip") => to = target.clone(),
                _ => return Some(to),
            }
        }
        None
    }

    // `POST /reporting/{name}`'s body (docs/discord-report-re.md §3): the nodes visited ending with this one, and
    // what was chosen on them.
    fn submit_body(&self, target: &ReportTarget, history: &[(String, Option<String>)], node: &str, picks: &HashMap<String, Value>) -> Value {
        let path: Vec<&Node> = history.iter().map(|(id, _)| id.as_str()).chain([node]).filter_map(|id| self.nodes.get(id)).collect();
        let elements: Map<String, Value> = path.iter().flat_map(|n| &n.elements).filter_map(|e| Some((e.name.clone(), picks.get(&e.name)?.clone()))).collect();
        let mut body = target.ids();
        body.insert("name".into(), target.name().into());
        body.insert("breadcrumbs".into(), path.iter().map(|n| n.id.clone()).collect());
        body.insert("elements".into(), elements.into());
        body.insert("language".into(), if self.language.is_null() { "en".into() } else { self.language.clone() });
        for (field, value) in [("version", &self.version), ("variant", &self.variant)] {
            if !value.is_null() {
                body.insert(field.into(), value.clone());
            }
        }
        body.into()
    }
}

impl Node {
    fn parse(v: &Value) -> Option<Self> {
        let text = |field: &str| v[field].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        let button = v["button"].as_object().and_then(|b| Some((b.get("type")?.as_str()?.to_owned(), b.get("target").filter(|t| !t.is_null()).map(key))));
        let elements = v["elements"].as_array().into_iter().flatten().filter_map(|e| {
            Some(Element { kind: e["type"].as_str()?.to_owned(), name: e["name"].as_str().unwrap_or_default().to_owned(), data: e["data"].clone(), required: e["should_submit_data"].as_bool() == Some(true), require_all: e["require_all_options"].as_bool() == Some(true) })
        });
        let children = v["children"].as_array().into_iter().flatten().filter_map(|c| Some((c.get(0)?.as_str()?.to_owned(), key(c.get(1)?))));
        Some(Self {
            id: v.get("id").filter(|id| !id.is_null())?.clone(),
            header: text("header").unwrap_or_default(),
            subheader: text("subheader"),
            info: text("info"),
            button,
            elements: elements.collect(),
            children: children.collect(),
            auto_submit: v["is_auto_submit"].as_bool() == Some(true),
        })
    }

    // Discord holds the button back until each required checkbox group has a pick (all of them with
    // `require_all_options`) and each required radio group has one.
    fn ready(&self, picks: &HashMap<String, Value>) -> bool {
        self.elements.iter().all(|e| {
            let picked = picks.get(&e.name);
            match e.kind.as_str() {
                "checkbox" => {
                    let chosen: Vec<&str> = picked.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                    let all = e.data.as_array().into_iter().flatten().filter_map(|o| o[0].as_str()).all(|v| chosen.contains(&v));
                    (!e.required || !chosen.is_empty()) && (!e.require_all || all)
                }
                "radio_group" => !e.required || picked.and_then(Value::as_str).is_some_and(|v| !v.is_empty()),
                _ => true,
            }
        })
    }
}

// Node ids come as numbers or strings; the map's keys are their text.
fn key(id: &Value) -> String {
    id.as_str().map_or_else(|| id.to_string(), str::to_owned)
}

fn head(title: SharedString, subheader: Option<AnyElement>, close: ui::Control, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    let text = ui::col()
        .flex_1()
        .gap_1()
        .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
        .children(subheader.map(|s| div().text_color(palette::muted(cx)).child(s)));
    ui::row().items_start().gap_3().child(text).child(close)
}

fn labelled(label: &'static str, content: impl IntoElement, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    ui::col().gap_1p5().child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(label)).child(content)
}

fn well(cx: &mut Context<Waffle>) -> gpui_kit::Div {
    ui::row().items_stretch().p_3().squircle(RADIUS_MD).bg(palette::surface(palette::Level::Page, cx))
}

fn notice(body: AnyElement, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    well(cx).items_start().gap_2p5().child(div().text_color(palette::muted(cx)).child(Icon::new(Glyph::Info).size(ui::ICON_LG))).child(div().flex_1().min_w_0().child(body))
}

fn user_preview(user: &User, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    well(cx).items_center().gap_3().child(ui::avatar(user.avatar.clone(), 40.)).child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(user.name.clone()))
}

// The reasons picked so far, under Discord's "Report Category".
fn crumbs(history: &[(String, Option<String>)], cx: &mut Context<Waffle>) -> Option<gpui_kit::Div> {
    let picked: Vec<_> = history.iter().filter_map(|(_, label)| label.clone()).collect();
    if picked.is_empty() {
        return None;
    }
    let faint = palette::faint(cx);
    let rows = picked.into_iter().map(move |label| ui::row().gap_2().child(div().text_color(faint).child(Icon::new(Glyph::Checkmark).size(ui::ICON_MD))).child(label));
    Some(labelled("Report category", well(cx).flex_col().gap_1p5().text_color(palette::strong(cx)).children(rows), cx))
}

fn text_block(data: &Value, cx: &mut Context<Waffle>) -> Option<gpui_kit::Div> {
    let header = data["header"].as_str().filter(|s| !s.is_empty()).map(|h| div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(h.to_owned()));
    let body = data["body"].as_str().filter(|s| !s.is_empty()).map(|b| div().text_color(palette::text(cx)).child(b.to_owned()));
    (header.is_some() || body.is_some()).then(|| ui::col().gap_1().children(header).children(body))
}

fn link_row(data: &Value, id: &str, cx: &mut Context<Waffle>) -> Option<AnyElement> {
    let url = data["url"].as_str()?.to_owned();
    let text = data["link_text"].as_str().unwrap_or(&url).to_owned();
    let detail = data["link_description"].as_str().filter(|s| !s.is_empty()).map(|d| SharedString::from(d.to_owned()));
    let row = ui::pick_row(SharedString::from(id.to_owned()), div().text_color(palette::muted(cx)).child(Icon::new(Glyph::Open).size(ui::ICON_LG)), text, detail, cx).on_click(move |_, _, cx| cx.open_url(&url));
    Some(labelled("Learn more", div().mx_neg_2().child(row), cx).into_any_element()) // lint-allow(raw-margin): a negative margin lets the row's hover fill reach the dialog's text edge, which ui::pick_row has no option for
}

fn legal(cx: &mut Context<Waffle>) -> AnyElement {
    let link = div().id("report-guidelines").text_color(palette::accent_ink(cx)).font_weight(FontWeight::SEMIBOLD).cursor_pointer().child("Community Guidelines").on_click(|_, _, cx| cx.open_url("https://discord.com/guidelines"));
    ui::row().items_stretch()
        .flex_wrap()
        .gap_x_1()
        .text_size(ui::TEXT_XS)
        .text_color(palette::muted(cx))
        .child("By submitting this report you confirm that it is truthful and made in good faith. Read the")
        .child(link)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> ReportMenu {
        ReportMenu::parse(&json!({
            "version": "1.0", "variant": null, "language": "en", "root_node_id": 1, "success_node_id": 90,
            "nodes": {
                "1": { "id": 1, "header": "What's wrong?", "children": [["Spam", 2], ["Gone", 404]], "elements": [] },
                "2": { "id": 2, "header": "skip me", "elements": [{ "type": "skip", "name": "s" }], "button": { "type": "next", "target": 3 } },
                "3": { "id": 3, "header": "Pick", "button": { "type": "submit" }, "elements": [
                    { "type": "checkbox", "name": "kind", "should_submit_data": true, "data": [["ads", "Ads", "", false], ["links", "Links", "", false]] }
                ] },
                "90": { "id": 90, "header": "Thanks", "button": { "type": "done" }, "elements": [{ "type": "success", "name": "s" }] }
            }
        }))
        .unwrap()
    }

    #[test]
    fn steps_skip_ahead_and_missing_nodes_go_nowhere() {
        let menu = menu();
        assert_eq!(menu.resolve("2".into()).as_deref(), Some("3"));
        assert_eq!(menu.resolve("404".into()), None);
    }

    #[test]
    fn submit_waits_for_a_required_pick() {
        let menu = menu();
        let mut picks = HashMap::new();
        assert!(!menu.nodes["3"].ready(&picks));
        picks.insert("kind".to_owned(), json!(["ads"]));
        assert!(menu.nodes["3"].ready(&picks));
    }

    #[test]
    fn body_carries_the_path_its_picks_and_the_ids() {
        let menu = menu();
        let target = ReportTarget::Message(Box::new(Message { id: "m".into(), channel_id: "c".into(), ..Default::default() }));
        let picks = HashMap::from([("kind".to_owned(), json!(["links"])), ("elsewhere".to_owned(), json!("x"))]);
        let body = menu.submit_body(&target, &[("1".into(), Some("Spam".into()))], "3", &picks);
        assert_eq!(
            body,
            json!({ "name": "message", "channel_id": "c", "message_id": "m", "breadcrumbs": [1, 3], "elements": { "kind": ["links"] }, "language": "en", "version": "1.0" })
        );
    }
}
