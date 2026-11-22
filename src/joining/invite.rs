// The invite screen: what you see before joining from an invite link (design/explore-joining.html §0).
use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, AppContext, Context, Corners, Div, Entity, Focusable, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement, Stateful, StatefulInteractiveElement, Styled, StyledImage, Window,
    base::{Transition, transition},
    component::{
        Icon,
        input::{Input, InputState},
    },
    div,
    prelude::FluentBuilder,
    px,
};
use serde_json::{Value, json};

use super::{api, features};
use crate::server_settings::api::Call;
use crate::{
    Waffle,
    discord::{AccountSettings, Invite, Status, parse_invite},
    icons::Glyph,
    link_preview::grouped,
    motion, palette,
    ui::{self, RADIUS_XL, Squircle, Tone},
};

// Discord's limit on the nickname field.
const NICKNAME_MAX: usize = 32;
const WIDE: f32 = 880.;
const NARROW: f32 = 440.;
const LEFT: f32 = 420.;
// Below this window width the splash column drops and the card goes to one column.
const TWO_COLUMNS_FROM: f32 = 700.;

pub struct InviteView {
    code: String,
    preview: Preview,
    settings_open: bool,
    allow_dms: bool,
    show_activity: bool,
    nickname: Option<Entity<InputState>>,
}

enum Preview {
    Loading,
    Invalid,
    Ready(Invite),
}

impl InviteView {
    // Discord's defaults: DMs follow your default for new servers, activity starts on.
    fn new(code: String, preview: Preview, account: &AccountSettings) -> Self {
        InviteView { code, preview, settings_open: false, allow_dms: !account.default_guilds_restricted, show_activity: true, nickname: None }
    }
}

impl Waffle {
    // An invite link was opened: show the server before joining.
    pub(crate) fn open_invite(&mut self, code: String, cx: &mut Context<Self>) {
        self.join.invite = Some(InviteView::new(code.clone(), Preview::Loading, &self.account_settings));
        cx.notify();
        self.server_call(api::invite(&code), move |this, result, _| {
            let Some(view) = this.join.invite.as_mut().filter(|v| v.code == code) else { return };
            view.preview = preview(result);
        }, cx);
    }

    // Once a join from the invite screen goes through: the nickname and privacy picked there.
    pub(crate) fn apply_invite_settings(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(picked) = self.join.invite_settings.remove(guild_id) else { return };
        if let Some(nick) = &picked.nickname {
            self.server_call(api::nickname(guild_id, nick), |this, result, cx| {
                if let Err(error) = result {
                    this.show_toast(error, cx);
                }
            }, cx);
        }
        let Some(body) = privacy_body(&self.account_settings, guild_id, &picked) else { return };
        // Not verified: Discord's own client writes these through its protobuf settings; the legacy
        // PATCH /users/@me/settings still takes both lists.
        self.server_call(Call::new(reqwest::Method::PATCH, "/users/@me/settings".into(), Some(body.clone())), move |this, result, cx| match result {
            Ok(_) => this.account_settings.apply(&body),
            Err(error) => this.show_toast(error, cx),
        }, cx);
    }

    fn accept_invite(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.join.invite.take() else { return };
        let Preview::Ready(invite) = view.preview else { return };
        let typed = view.nickname.map(|n| n.read(cx).value().to_string()).unwrap_or_default();
        self.join.invite_settings.insert(invite.guild_id, Picked { nickname: picked_nickname(&typed), allow_dms: view.allow_dms, show_activity: view.show_activity });
        self.join_invite(view.code, cx);
        cx.notify();
    }

    fn close_invite(&mut self, cx: &mut Context<Self>) {
        self.join.invite = None;
        cx.notify();
    }

    fn edit_invite(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut InviteView)) {
        if let Some(view) = self.join.invite.as_mut() {
            f(view);
        }
        cx.notify();
    }

    pub(crate) fn invite_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.ensure_nickname_input(window, cx);
        let view = self.join.invite.as_ref()?;
        let wide = f32::from(window.viewport_size().width) >= TWO_COLUMNS_FROM;
        let card = match &view.preview {
            Preview::Loading => shell(None, wide, loading(cx), window, cx),
            Preview::Invalid => shell(None, wide, invalid(cx), window, cx),
            Preview::Ready(invite) if self.guilds.iter().any(|g| g.id == invite.guild_id) => shell(None, wide, member(invite, cx), window, cx),
            Preview::Ready(invite) => {
                let left = self.invite_column(view, invite, window, cx);
                shell(invite.splash.as_deref(), wide, left, window, cx)
            }
        };
        let backdrop = div().id("invite-overlay").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_invite(cx)));
        Some(ui::modal(backdrop, card, cx))
    }

    // The nickname field's editor, made the first time an invite that allows one is on screen.
    fn ensure_nickname_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.join.invite.as_mut() else { return };
        let allowed = matches!(&view.preview, Preview::Ready(invite) if invite.nickname_changeable);
        if !allowed || view.nickname.is_some() {
            return;
        }
        view.nickname = Some(cx.new(|cx| InputState::new(window, cx).placeholder("What should everyone call you?").validate(|text, _| text.chars().count() <= NICKNAME_MAX)));
    }

    fn invite_column(&self, view: &InviteView, invite: &Invite, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (name, avatar) = self.me.as_ref().map(|me| (me.name.clone(), me.avatar.clone())).unwrap_or_default();
        let accept = ui::button("invite-accept", Tone::Primary, cx)
            .large()
            .w_full()
            .when(!avatar.is_empty(), |b| b.child(ui::avatar(avatar, 20.)))
            .child(format!("Accept as {name}"))
            .on_click(cx.listener(|this, _, _, cx| this.accept_invite(cx)));
        let decline = ui::button("invite-decline", Tone::Ghost, cx).large().w_full().child("No Thanks").on_click(cx.listener(|this, _, _, cx| this.close_invite(cx)));
        heading(invite, "You've been invited to join", cx)
            .child(self.invite_settings(view, window, cx))
            .child(actions().child(accept).child(decline))
    }

    // "Server Settings": opens in place, the chevron turning as the body unfolds.
    fn invite_settings(&self, view: &InviteView, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let open = view.settings_open;
        let angle = transition("invite-settings-chevron", if open { 90. } else { 0. }, Transition::new(ui::DUR_POP).easing(motion::ease_out()), window, cx);
        let (strong, muted, hover) = (palette::strong(cx), palette::muted(cx), palette::hover(cx));
        let head = div()
            .id("invite-settings")
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(12.))
            .squircle(16.)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .text_color(strong)
            .child(Icon::new(Glyph::Settings).size(px(20.)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Server Settings"))
                    .child(div().text_size(px(13.)).text_color(muted).child("You can customize these at any time")),
            )
            .child(Icon::new(Glyph::ChevronRight).size(px(16.)).text_color(muted).rotate(gpui_kit::Radians(angle.to_radians())))
            .on_click(cx.listener(|this, _, _, cx| this.edit_invite(cx, |v| v.settings_open = !v.settings_open)));
        let nickname = view.nickname.as_ref().map(|state| {
            let focus = state.read(cx).focus_handle(cx);
            ui::field("Server Nickname", cx).child(ui::text_field("invite-nickname", Input::new(state).appearance(false), &focus))
        });
        let dms = ui::toggle_row("invite-dms", "Allow direct messages", None, view.allow_dms, window, cx).on_click(cx.listener(|this, _, _, cx| this.edit_invite(cx, |v| v.allow_dms = !v.allow_dms)));
        // Discord offers this one only while your activity status is shared at all.
        let activity = self.account_settings.show_current_game.then(|| {
            ui::toggle_row("invite-activity", "Show your activity status", None, view.show_activity, window, cx).on_click(cx.listener(|this, _, _, cx| this.edit_invite(cx, |v| v.show_activity = !v.show_activity)))
        });
        let body = div().px(px(14.)).pt(px(4.)).pb(px(14.)).flex().flex_col().gap(px(12.)).children(nickname).child(dms).children(activity);
        div().w_full().mt(px(20.)).squircle(16.).bg(palette::surface(palette::Level::Group, cx)).text_left().child(head).children(unfold("invite-settings-body", open, body, window, cx))
    }
}

// The card over the scrim, popping in from its centre: the column, and the splash beside it when the
// server has one and the window is wide enough.
fn shell(splash: Option<&str>, wide: bool, left: Div, window: &mut Window, cx: &App) -> AnyElement {
    let two_columns = wide && splash.is_some();
    let room = f32::from(window.viewport_size().width) - 32.;
    let width = if two_columns { WIDE } else { NARROW }.min(room);
    let right_corners = Corners { top_left: px(0.), top_right: px(RADIUS_XL), bottom_right: px(RADIUS_XL), bottom_left: px(0.) };
    let panel = splash.filter(|_| two_columns).map(|url| {
        // A splash that fails to load leaves the plain panel.
        ui::edge(div().flex_1().relative().bg(palette::surface(palette::Level::Group, cx)), right_corners)
            .child(ui::media(url.to_owned(), right_corners).absolute().inset_0().size_full().object_fit(gpui_kit::ObjectFit::Cover))
    });
    div()
        .id("invite-card")
        .w(px(width))
        .min_h(px(420.))
        .flex()
        .squircle(RADIUS_XL)
        .overflow_hidden()
        .bg(palette::surface(palette::Level::Raised, cx))
        .shadow(ui::shadow_pop(cx))
        .text_size(px(14.))
        .text_color(palette::text(cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(left.when(two_columns, |l| l.w(px(LEFT)).flex_none()).when(!two_columns, |l| l.flex_1()))
        .children(panel)
        .with_animation("invite-pop", Animation::new(ui::DUR_PANEL), |card, t| {
            let t = motion::ease_out().sample(t);
            card.opacity(t).scale(0.95 + 0.05 * t)
        })
        .into_any_element()
}

fn column() -> Div {
    div().min_w_0().pt(px(40.)).px(px(28.)).pb(px(28.)).flex().flex_col().items_center().text_center().gap(px(6.))
}

fn actions() -> Div {
    div().w_full().mt_auto().pt(px(24.)).flex().flex_col().gap(px(8.))
}

// The server's tile, `label`, its name (with the verified mark) and its counts.
fn heading(invite: &Invite, label: &'static str, cx: &App) -> Div {
    let verified = invite.features.iter().any(|f| f == features::VERIFIED || f == features::PARTNERED);
    let count = |n: Option<u64>, label: &'static str, dot| n.map(|n| div().flex().items_center().gap(px(6.)).child(div().size(px(8.)).rounded_full().bg(dot)).child(format!("{} {label}", grouped(n))));
    let name = div()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .text_size(px(24.))
        .line_height(px(30.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::strong(cx))
        .when(verified, |d| d.child(Icon::new(Glyph::CheckCircle).size(px(22.)).flex_none().text_color(palette::positive_ink(cx))))
        .child(div().min_w_0().child(invite.name.clone()));
    column()
        .child(div().mb(px(10.)).child(ui::server_tile(invite.icon.as_deref(), &invite.name, 72., cx)))
        .child(div().text_color(palette::muted(cx)).child(label))
        .child(name)
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .gap_x(px(14.))
                .text_color(palette::muted(cx))
                .children(count(invite.online, "Online", palette::status_in(Status::Online, cx)))
                .children(count(invite.members, "Members", palette::faint(cx))),
        )
}

fn loading(cx: &App) -> Div {
    column()
        .child(ui::skeleton("invite-loading-tile", 22., cx).size(px(72.)).mb(px(10.)))
        .child(ui::skeleton("invite-loading-label", 7., cx).w(px(180.)).h(px(14.)))
        .child(ui::skeleton("invite-loading-name", 10., cx).w(px(220.)).h(px(26.)))
        .child(ui::skeleton("invite-loading-counts", 7., cx).w(px(160.)).h(px(14.)))
        .child(actions().child(ui::skeleton("invite-loading-accept", ui::RADIUS_MD, cx).w_full().h(px(44.))))
}

fn invalid(cx: &mut Context<Waffle>) -> Div {
    let tile = div().size(px(72.)).mb(px(10.)).squircle(22.).bg(palette::selected(cx)).flex().items_center().justify_center().text_color(palette::faint(cx)).child(Icon::new(Glyph::Prohibit).size(px(32.)));
    // Not verified: the button label on Discord's invalid-invite screen.
    let okay = ui::button("invite-okay", Tone::Secondary, cx).large().w_full().child("Okay").on_click(cx.listener(|this, _, _, cx| this.close_invite(cx)));
    column()
        .child(tile)
        .child(div().text_size(px(24.)).line_height(px(30.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Invite Invalid"))
        .child(div().text_color(palette::muted(cx)).child("This invite may be expired, or you might not have permission to join."))
        .child(actions().child(okay))
}

fn member(invite: &Invite, cx: &mut Context<Waffle>) -> Div {
    let guild_id = invite.guild_id.clone();
    let go = ui::button("invite-go", Tone::Primary, cx).large().w_full().child("Go to Server").on_click(cx.listener(move |this, _, _, cx| {
        this.join.invite = None;
        this.open_joined_guild(guild_id.clone(), cx);
    }));
    heading(invite, "You're already a member of", cx).child(actions().child(go))
}

// `body` unfolding to its height when `open`, folding back to nothing when not: measured each layout and
// eased to, clipped on the way.
fn unfold(id: &'static str, open: bool, body: Div, window: &mut Window, cx: &mut App) -> Option<Stateful<Div>> {
    let measured = window.use_keyed_state(format!("{id}-measured"), cx, |_, _| None::<f32>);
    let target = if open { measured.read(cx).unwrap_or(0.) } else { 0. };
    let height = transition(format!("{id}-height"), target, Transition::new(ui::DUR_POP).easing(motion::ease_out()), window, cx);
    if !open && height <= 0.5 {
        return None;
    }
    Some(
        div()
            .h(px(height))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(body.flex_none())
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(body) = bounds.first() else { return };
                let next = Some(f32::from(body.size.height));
                measured.update(cx, |h, cx| {
                    if *h != next {
                        *h = next;
                        cx.notify();
                    }
                })
            })
            .id(id),
    )
}

fn preview(result: Result<Value, String>) -> Preview {
    result.ok().as_ref().and_then(parse_invite).map_or(Preview::Invalid, Preview::Ready)
}

// The nickname to set once joined; an empty field keeps your name.
// What the invite screen's Server Settings held when you accepted, applied once the join lands.
pub struct Picked {
    nickname: Option<String>,
    allow_dms: bool,
    show_activity: bool,
}

// The account's per-server lists with this server added where a switch was turned off; None when
// nothing changes.
fn privacy_body(account: &AccountSettings, guild_id: &str, picked: &Picked) -> Option<Value> {
    let with = |list: &[String]| -> Option<Vec<String>> { (!list.iter().any(|g| g == guild_id)).then(|| list.iter().cloned().chain([guild_id.to_owned()]).collect()) };
    let mut body = serde_json::Map::new();
    if !picked.allow_dms && let Some(list) = with(&account.restricted_guilds) {
        body.insert("restricted_guilds".into(), json!(list));
    }
    if !picked.show_activity && let Some(list) = with(&account.activity_restricted_guilds) {
        body.insert("activity_restricted_guild_ids".into(), json!(list));
    }
    (!body.is_empty()).then_some(Value::Object(body))
}

fn picked_nickname(typed: &str) -> Option<String> {
    let nick = typed.trim();
    (!nick.is_empty()).then(|| nick.to_owned())
}

pub fn demo(waffle: &mut Waffle, scene: &str, _window: &mut Window, cx: &mut Context<Waffle>) {
    let plain = scene == "invite-plain";
    let night_owls = Invite {
        guild_id: "9001".into(),
        name: "Night owls".into(),
        icon: None,
        online: Some(1204),
        members: Some(8930),
        features: vec![features::VERIFIED.into()],
        // Not a real splash: it fails to load, which shows the plain panel (the offline look).
        splash: (!plain).then(|| crate::discord::splash_url("9001", "demo")),
        nickname_changeable: plain,
    };
    let preview = if scene == "invite-invalid" { Preview::Invalid } else { Preview::Ready(night_owls) };
    let mut view = InviteView::new("night-owls".into(), preview, &waffle.account_settings);
    view.settings_open = plain;
    waffle.join.invite = Some(view);
    cx.notify();
}

#[cfg(test)]
mod tests {
    #[test]
    fn privacy_adds_this_server_only_where_a_switch_is_off() {
        let account = super::AccountSettings { restricted_guilds: vec!["7".into()], ..Default::default() };
        let picked = |allow_dms, show_activity| super::Picked { nickname: None, allow_dms, show_activity };
        assert_eq!(super::privacy_body(&account, "9", &picked(true, true)), None);
        assert_eq!(super::privacy_body(&account, "9", &picked(false, true)), Some(serde_json::json!({ "restricted_guilds": ["7", "9"] })));
        assert_eq!(super::privacy_body(&account, "7", &picked(false, true)), None, "already restricted");
        assert_eq!(super::privacy_body(&account, "9", &picked(true, false)), Some(serde_json::json!({ "activity_restricted_guild_ids": ["9"] })));
    }

    use serde_json::json;

    use super::*;

    #[test]
    fn previews_a_server_invite_and_nothing_else() {
        let ok = preview(Ok(json!({ "code": "owls", "guild": { "id": "9", "name": "Night owls", "splash": "s" }, "is_nickname_changeable": true })));
        assert!(matches!(ok, Preview::Ready(i) if i.nickname_changeable && i.splash.is_some()));
        assert!(matches!(preview(Ok(json!({ "code": "gdm", "channel": { "id": "5", "type": 3 } }))), Preview::Invalid));
        assert!(matches!(preview(Err("Unknown Invite".into())), Preview::Invalid));
    }

    #[test]
    fn sets_a_nickname_only_when_one_was_typed() {
        assert_eq!(picked_nickname("  Owl "), Some("Owl".into()));
        assert_eq!(picked_nickname("   "), None);
        assert_eq!(picked_nickname(""), None);
    }
}
