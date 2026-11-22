// The rules gate: the composer bar for members who haven't agreed yet, and its dialog (design/explore-joining.html §1).
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, Window, component::Icon, div, prelude::*, px, relative};

use super::{Form, api, features, fixtures};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::{Channel, Guild},
    icons::Glyph,
    overlays::backdrop,
    palette::{self, Level},
    ui::{self, Check, Notice, RADIUS_LG, Squircle, Tone},
};

const DIALOG_WIDTH: f32 = 520.;
const RULES_HEIGHT: f32 = 260.;

pub struct RulesView {
    guild_id: String,
    agreed: bool,
    // Submit was pressed with the box unticked: the agree row says so until it's ticked.
    nudged: bool,
    submitting: bool,
    refusal: Option<Refusal>,
    // Agreed on Discord's side: the next frame closes the dialog and hands the composer focus.
    done: bool,
    rules_scroll: gpui_kit::ScrollHandle,
}

impl RulesView {
    fn new(guild_id: &str) -> Self {
        RulesView { guild_id: guild_id.to_owned(), agreed: false, nudged: false, submitting: false, refusal: None, done: false, rules_scroll: gpui_kit::ScrollHandle::new() }
    }
}

impl Waffle {
    // Replaces the composer while you can't talk in this channel's server yet.
    pub(crate) fn composer_gate(&mut self, channel: &Channel, _window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let me = self.me.as_ref().map(|u| u.id.as_str());
        let guild = self.guilds.iter().find(|g| g.channels.iter().chain(&g.threads).any(|c| c.id == channel.id))?;
        if !gated(guild, me) {
            return None;
        }
        // GAP: an application already sent (join request SUBMITTED or REJECTED) gets Discord's own bar text;
        // Waffle doesn't track join requests yet, so it keeps the default one.
        let guild_id = guild.id.clone();
        let complete = ui::button("rules-complete", Tone::Primary, cx).child("Complete").on_click(cx.listener(move |this, _, _, cx| this.open_rules(&guild_id, cx)));
        // The composer card's shape and glass (chat.rs render_composer), so swapping one for the other doesn't jump.
        let bar = div()
            .id("rules-gate")
            .occlude()
            .mx(px(16.))
            .min_h(px(48.))
            .py(px(6.))
            .pl(px(16.))
            .pr(px(6.))
            .flex()
            .items_center()
            .gap(px(12.))
            .squircle(RADIUS_LG)
            .bg(palette::surface(Level::Raised, cx).opacity(0.72))
            .backdrop_blur(px(24.))
            .backdrop_saturate(1.6)
            .shadow(ui::shadow_pop_on(gpui_kit::transparent_black(), cx))
            .border(px(0.5))
            .border_color(palette::line(cx))
            .text_size(px(14.))
            .text_color(palette::muted(cx))
            .child(Icon::new(Glyph::BookOpen).size(px(20.)))
            .child(div().flex_1().min_w_0().child("You must complete a few more steps before you can talk."))
            .child(complete);
        Some(div().absolute().left_0().right_0().bottom_0().flex().flex_col().child(self.composer_measure(cx)).child(bar).child(div().h(px(8.))).into_any_element())
    }

    // Keeps the feed's foot padding in step with the bar, as the composer does with its own height.
    fn composer_measure(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (height, this) = (self.composer_height.clone(), cx.weak_entity());
        gpui_kit::canvas(
            move |bounds, _, cx| {
                let h = f32::from(bounds.size.height);
                if (h - height.get()).abs() > 0.5 {
                    height.set(h);
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    fn open_rules(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        self.fetch_form(guild_id, cx);
        self.join.rules = Some(RulesView::new(guild_id));
        cx.notify();
    }

    pub(crate) fn rules_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = self.join.rules.as_ref()?;
        if view.done {
            self.join.rules = None;
            cx.defer_in(window, |this, window, cx| this.composer.update(cx, |state, cx| state.focus(window, cx)));
            return None;
        }
        let guild = self.guilds.iter().find(|g| g.id == view.guild_id)?;
        let form = self.join.forms.get(&view.guild_id);
        let applies = guild.has_feature(features::MANUAL_APPROVAL) || matches!(form, Some(Load::Ready(f)) if f.has_questions);
        let body = ui::dialog_body().gap(px(20.)).child(rules_head(guild, applies, cx));
        let body = match form {
            // GAP: apply-to-join forms (questions beyond the rules) aren't filled in here yet; Discord takes them.
            _ if applies => body.child(apply_in_discord(&guild.id, cx)),
            Some(Load::Ready(form)) => body.child(rules_list(form, &view.rules_scroll, cx)).child(agree_row(view, window, cx)).children(view.refusal.as_ref().map(|r| ui::callout(Notice::Danger, Glyph::WarningCircle, r.title.clone(), r.detail.unwrap_or_default(), cx))).child(
                div().flex().justify_end().child(ui::button("rules-submit", Tone::Primary, cx).large().loading(view.submitting.then_some("Submitting")).child("Submit").on_click(cx.listener(|this, _, _, cx| this.submit_rules(cx)))),
            ),
            Some(Load::Failed(error)) => body.child(load_failed(&guild.id, error, cx)),
            _ => body.child(rules_skeleton(cx)),
        };
        Some(ui::modal(backdrop("rules-backdrop", cx), ui::dialog("rules-dialog", DIALOG_WIDTH, body, window, cx), cx))
    }

    fn toggle_agree(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.join.rules.as_mut() else { return };
        view.agreed = !view.agreed;
        view.nudged = false;
        cx.notify();
    }

    // Never greyed out: pressed unticked, it points at the box instead.
    fn submit_rules(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.join.rules.as_mut() else { return };
        if view.submitting {
            return;
        }
        if !view.agreed {
            view.nudged = true;
            return cx.notify();
        }
        let Some(Load::Ready(form)) = self.join.forms.get(&view.guild_id) else { return };
        let call = api::agree(&view.guild_id, form);
        (view.submitting, view.refusal) = (true, None);
        let guild_id = view.guild_id.clone();
        self.server_call(call, move |this, result, _| {
            if let Err(error) = &result
                && changed_form(error)
            {
                this.join.forms.remove(&guild_id);
            }
            if result.is_ok()
                && let Some(guild) = this.guilds.iter_mut().find(|g| g.id == guild_id)
            {
                guild.me.pending = false;
            }
            let Some(view) = this.join.rules.as_mut().filter(|v| v.guild_id == guild_id) else { return };
            view.submitting = false;
            match result {
                Ok(_) => view.done = true,
                Err(error) => view.refusal = Some(refusal(&error)),
            }
        }, cx);
        cx.notify();
    }
}

// The composer bar applies to a pending member of a server with the rules gate; the owner never is one.
fn gated(guild: &Guild, me: Option<&str>) -> bool {
    guild.has_feature(features::RULES_GATE) && guild.me.pending && me.is_none_or(|me| guild.owner_id.as_deref() != Some(me))
}

#[derive(Clone, Debug, PartialEq)]
struct Refusal {
    title: String,
    detail: Option<&'static str>,
}

// Discord's words for a refused Submit. `error` is discord::send's text: Discord's message (with the first
// field error after it), or the status line ("429 Too Many Requests") when the body had none.
fn refusal(error: &str) -> Refusal {
    // Not verified: Discord's message bodies for 429 and 403 on PUT /guilds/{id}/requests/@me; matched on the
    // status line and the messages Discord sends for these statuses elsewhere.
    if error.starts_with("429") || error.contains("rate limited") {
        return Refusal { title: "WOAH THERE. WAY TOO SPICY".into(), detail: Some("You've been sending too many Guild applications.") };
    }
    if error.starts_with("403") || error.contains("Missing Access") || error.contains("Missing Permissions") {
        return Refusal { title: "You don't have permission to join this server".into(), detail: None };
    }
    if changed_form(error) {
        return Refusal { title: "This server's requirements have changed. Please close this and try again.".into(), detail: None };
    }
    Refusal { title: if error.is_empty() { "Oops, something went wrong...".into() } else { error.to_owned() }, detail: None }
}

// Discord's `errors.version` or `errors.form_fields`: the form we sent is stale.
fn changed_form(error: &str) -> bool {
    error.contains("version") || error.contains("form_fields")
}

// The server tile, Discord's title and subtitle, and the close button.
fn rules_head(guild: &Guild, applies: bool, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let (title, subtitle): (SharedString, &str) = if applies {
        (format!("Answer a few questions to join {}", guild.name).into(), "The server admin will get back to you shortly after you submit.")
    } else {
        ("Before you can talk here...".into(), "You’ll need to complete the required steps below.")
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(ui::server_tile(guild.icon.as_deref(), &guild.name, 48., cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
                .child(div().text_size(px(14.)).text_color(palette::muted(cx)).child(subtitle)),
        )
        .child(ui::icon_button("rules-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| {
            this.join.rules = None;
            cx.notify();
        })))
}

// "Agree to rules" and its count, the description, then the numbered rules in one group that scrolls when long.
fn rules_list(form: &Form, scroll: &gpui_kit::ScrollHandle, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let count = match form.rules.len() {
        1 => "1 rule".to_owned(),
        n => format!("{n} rules"),
    };
    // GAP: rules render as plain text; Discord parses them as markdown (channel links, bold).
    let (line, strong) = (palette::line(cx), palette::strong(cx));
    let rows: Vec<_> = form.rules.iter().enumerate().map(|(i, rule)| {
        div()
            .relative()
            .p(px(12.))
            .flex()
            .items_start()
            .gap(px(12.))
            .when(i > 0, |row| row.child(div().absolute().top_0().left(px(48.)).right(px(12.)).h(px(1.)).bg(line)))
            .child(number(i + 1, cx))
            .child(div().flex_1().min_w_0().text_size(px(15.)).line_height(px(22.)).text_color(strong).child(rule.clone()))
    }).collect();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .mx(px(4.))
                .mb(px(8.))
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::strong(cx))
                .child("Agree to rules")
                .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::muted(cx)).child(count)),
        )
        .children(form.description.clone().map(|d| div().mx(px(4.)).mb(px(8.)).text_size(px(14.)).text_color(palette::text(cx)).child(d)))
        .child(div().p(px(4.)).squircle(RADIUS_LG).bg(palette::surface(Level::Group, cx)).child(ui::scroll_fade(div().id("rules-list").max_h(px(RULES_HEIGHT)).flex().flex_col().children(rows), scroll, palette::surface(Level::Group, cx))))
}

// A rule's number: an accent-soft disc in the 24px column the agree row's checkbox shares.
fn number(n: usize, cx: &gpui_kit::App) -> impl IntoElement + use<> {
    div()
        .size(px(24.))
        .mt(px(-1.))
        .flex_none()
        .rounded_full()
        .bg(palette::accent_soft(cx))
        .text_color(palette::accent_ink(cx))
        .text_size(px(12.))
        .font_weight(FontWeight::BOLD)
        .flex()
        .items_center()
        .justify_center()
        .child(n.to_string())
}

// The full-width agree card: ticked takes the accent tint and edge; Submit pressed unticked gives it a danger
// edge and says why under it.
fn agree_row(view: &RulesView, window: &mut Window, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let group = palette::surface(Level::Group, cx);
    let (fill, edge) = match (view.agreed, view.nudged) {
        (true, _) => (palette::accent_soft(cx), palette::accent().opacity(0.55)),
        (false, true) => (group, palette::danger()),
        (false, false) => (group, gpui_kit::transparent_black()),
    };
    let hover = if view.agreed { fill } else { ui::mix(group, palette::strong(cx), 0.04) };
    let check = ui::check_live("rules-agree-check", if view.agreed { Check::On } else { Check::Off }, window, cx);
    let row = div()
        .id("rules-agree")
        .w_full()
        .py(px(14.))
        .px(px(16.))
        .flex()
        .items_center()
        .gap(px(12.))
        .squircle(RADIUS_LG)
        .bg(fill)
        .hover(move |s| s.bg(hover))
        .border(px(1.))
        .border_color(edge)
        .cursor_pointer()
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::strong(cx))
        .child(div().w(px(24.)).flex_none().flex().justify_center().child(check))
        .child("I have read and agree to the rules")
        .on_click(cx.listener(|this, _, _, cx| this.toggle_agree(cx)));
    let warning = view.nudged.then(|| {
        div()
            .mt(px(8.))
            .mx(px(4.))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(14.))
            .text_color(palette::danger_ink(cx))
            .child(Icon::new(Glyph::WarningCircle).size(px(16.)))
            .child("Tick the box to agree to the rules first.")
    });
    div().flex().flex_col().child(row).children(warning)
}

// Apply to join: the questions are Discord's to show, so the dialog sends you there.
fn apply_in_discord(guild_id: &str, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let url = format!("https://discord.com/channels/{guild_id}");
    div()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(ui::callout(Notice::Info, Glyph::InfoFill, "This server asks a few questions Waffle can't show yet.", "Answer them in Discord to apply.", cx))
        .child(
            div()
                .flex()
                .justify_end()
                .child(ui::button("rules-open-discord", Tone::Primary, cx).large().child(Icon::new(Glyph::Open).size(px(16.))).child("Open in Discord").on_click(move |_, _, cx| cx.open_url(&url))),
        )
}

fn load_failed(guild_id: &str, error: &str, cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let guild_id = guild_id.to_owned();
    let retry = ui::button("rules-retry", Tone::Secondary, cx).small().child("Try again").on_click(cx.listener(move |this, _, _, cx| this.fetch_form(&guild_id, cx)));
    ui::callout_with(Notice::Danger, Glyph::WarningCircle, "Couldn't load the rules", error.to_owned(), div().mt(px(8.)).child(retry), cx)
}

// The form on its way: the head, numbered rows of shimmer, and the agree card's place.
fn rules_skeleton(cx: &mut Context<Waffle>) -> impl IntoElement + use<> {
    let rows = [0.9, 0.7, 0.8].into_iter().enumerate().map(|(i, width)| {
        div()
            .p(px(12.))
            .flex()
            .items_center()
            .gap(px(12.))
            .child(ui::skeleton_circle(SharedString::from(format!("rules-skeleton-n{i}")), 24., cx))
            .child(ui::skeleton(SharedString::from(format!("rules-skeleton-{i}")), 6., cx).h(px(14.)).w(relative(width)))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(20.))
        .child(div().p(px(4.)).squircle(RADIUS_LG).bg(palette::surface(Level::Group, cx)).children(rows))
        .child(ui::skeleton("rules-skeleton-agree", RADIUS_LG, cx).w_full().h(px(52.)))
}

pub fn demo(waffle: &mut Waffle, scene: &str, _window: &mut Window, _cx: &mut Context<Waffle>) {
    // The fixture server is ours; someone else owns it here so the gate applies.
    if let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") {
        guild.owner_id = Some("8388608".into());
    }
    if scene == "rules" {
        return;
    }
    let mut view = RulesView::new("1");
    match scene {
        "rules-error" => view.nudged = true,
        "rules-long" => {
            let mut form = fixtures::form();
            let long = "Respect everyone's time zones and schedules: don't ping people outside the hours they list in their profile, and never mass-ping roles to find a group for tonight.";
            form.rules = (0..14).map(|n| if n % 2 == 0 { long.to_owned() } else { fixtures::form().rules[n % 6].clone() }).collect();
            form.description = Some("These keep the late-night crowd friendly. Moderators act on reports within a day.".into());
            waffle.join.forms.insert("1".into(), Load::Ready(form));
        }
        _ => {}
    }
    waffle.join.rules = Some(view);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord::Membership;

    fn guild(features: &[&str], pending: bool, owner: &str) -> Guild {
        Guild { features: features.iter().map(|f| (*f).to_owned()).collect(), owner_id: Some(owner.into()), me: Membership { pending, ..Default::default() }, ..Default::default() }
    }

    #[test]
    fn gate_needs_the_feature_pending_and_not_owner() {
        assert!(gated(&guild(&[features::RULES_GATE], true, "owner"), Some("me")));
        assert!(!gated(&guild(&[features::RULES_GATE], false, "owner"), Some("me")));
        assert!(!gated(&guild(&[features::COMMUNITY], true, "owner"), Some("me")));
        assert!(!gated(&guild(&[features::RULES_GATE], true, "me"), Some("me")));
    }

    #[test]
    fn refusals_keep_discords_words() {
        assert_eq!(refusal("429 Too Many Requests").title, "WOAH THERE. WAY TOO SPICY");
        assert_eq!(refusal("You are being rate limited.").detail, Some("You've been sending too many Guild applications."));
        assert_eq!(refusal("Missing Access").title, "You don't have permission to join this server");
        assert_eq!(refusal("403 Forbidden").title, "You don't have permission to join this server");
        assert_eq!(refusal("Invalid Form Body: version: Must be the current version").title, "This server's requirements have changed. Please close this and try again.");
        assert_eq!(refusal("Invalid Form Body: form_fields.0.response: Required").title, "This server's requirements have changed. Please close this and try again.");
        assert_eq!(refusal("Unknown Guild"), Refusal { title: "Unknown Guild".into(), detail: None });
    }
}
