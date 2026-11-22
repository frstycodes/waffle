// Onboarding: the full-screen questions after joining, then the server rules (design/explore-joining.html §2).
use std::{collections::HashSet, iter, time::Duration};

use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, Hsla, ImgResourceLoader, InteractiveElement, IntoElement, ObjectFit, ParentElement, Resource, ScrollHandle, StatefulInteractiveElement, Styled, Window,
    component::{Icon, scroll::ScrollableElement as _},
    div, img, point, prelude::*, px, relative, rgb,
};

use super::{Form, Onboarding, OptionEmoji, Prompt, PromptKind, PromptOption, api, features, member_flags};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::{Guild, Status},
    icons::Glyph,
    palette,
    ui::{self, Check, Squircle, Tone},
};

// Discord's cover moves on by itself after 3 s.
const COVER_HOLD: Duration = Duration::from_millis(3000);
const SPLASH_INSET: f32 = 48.;
// The question card over a splash: room for the 640px column and the footer, no more.
const SHEET_WIDTH: f32 = 760.;
const SHEET_HEIGHT: f32 = 640.;
// An option card's title line; its emoji centres on it.
const TITLE_LINE: f32 = 22.;
const RULES_HEIGHT: f32 = 300.;

// One server's flow: where you are and what you picked, kept when closed halfway.
pub struct OnboardingView {
    guild_id: String,
    step: Step,
    picked: HashSet<String>,
    // Prompts moved past with Next or Skip.
    passed: HashSet<String>,
    rules_read: bool,
    warning: Option<Warning>,
    // Discord's error from the last Finish.
    error: Option<String>,
    sending: bool,
    body_scroll: ScrollHandle,
    rules_scroll: ScrollHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Cover,
    Prompt(usize),
    Rules,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    Go(Step),
    Finish,
}

// Why Next didn't move: Waffle's button never greys out, it says what's missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Warning {
    Required,
    Unread,
}

impl Warning {
    fn text(self) -> &'static str {
        match self {
            Warning::Required => "This is a required question.",
            Warning::Unread => "You must finish reading the rules to join.",
        }
    }
}

// The rules step: off without the gate (or with manual approval), else the form's rules once loaded.
#[derive(Clone, Debug, PartialEq)]
enum RulesStep {
    Off,
    Loading,
    Ready(Form),
}

impl OnboardingView {
    fn new(guild_id: String) -> Self {
        OnboardingView {
            guild_id,
            step: Step::Cover,
            picked: HashSet::new(),
            passed: HashSet::new(),
            rules_read: false,
            warning: None,
            error: None,
            sending: false,
            body_scroll: ScrollHandle::new(),
            rules_scroll: ScrollHandle::new(),
        }
    }

    fn has_pick(&self, prompt: &Prompt) -> bool {
        prompt.options.iter().any(|o| self.picked.contains(&o.id))
    }

    fn answered(&self, prompt: &Prompt) -> bool {
        self.passed.contains(&prompt.id) || self.has_pick(prompt)
    }

    // Single-select swaps the pick; picking the chosen one again clears it.
    fn pick(&mut self, prompt: &Prompt, option_id: &str) {
        if self.picked.remove(option_id) {
            return;
        }
        if prompt.single_select {
            for option in &prompt.options {
                self.picked.remove(&option.id);
            }
        }
        self.picked.insert(option_id.to_owned());
    }

    fn first_step(prompts: &[Prompt], rules: bool) -> Next {
        match (prompts.is_empty(), rules) {
            (false, _) => Next::Go(Step::Prompt(0)),
            (true, true) => Next::Go(Step::Rules),
            (true, false) => Next::Finish,
        }
    }

    fn next(&self, prompts: &[Prompt], rules: bool) -> Result<Next, Warning> {
        match self.step {
            Step::Cover => Ok(Self::first_step(prompts, rules)),
            Step::Prompt(i) => {
                let prompt = &prompts[i];
                if prompt.required && !self.has_pick(prompt) {
                    return Err(Warning::Required);
                }
                Ok(match (i + 1 < prompts.len(), rules) {
                    (true, _) => Next::Go(Step::Prompt(i + 1)),
                    (false, true) => Next::Go(Step::Rules),
                    (false, false) => Next::Finish,
                })
            }
            Step::Rules if !self.rules_read => Err(Warning::Unread),
            Step::Rules => Ok(Next::Finish),
        }
    }

    fn back(&self, prompts: &[Prompt]) -> Step {
        match self.step {
            Step::Prompt(i) if i > 0 => Step::Prompt(i - 1),
            Step::Rules if !prompts.is_empty() => Step::Prompt(prompts.len() - 1),
            _ => Step::Cover,
        }
    }

    // Discord shows Back after the first question (there are no connection steps in Waffle).
    fn can_go_back(&self) -> bool {
        matches!(self.step, Step::Prompt(i) if i > 0) || self.step == Step::Rules
    }

    fn is_last(&self, prompts: &[Prompt], rules: bool) -> bool {
        match self.step {
            Step::Cover => false,
            Step::Prompt(i) => !rules && i + 1 == prompts.len(),
            Step::Rules => true,
        }
    }

    fn resume_at(&self, prompts: &[Prompt], rules: bool) -> Step {
        if let Some(i) = prompts.iter().position(|p| !self.answered(p)) {
            return Step::Prompt(i);
        }
        if rules {
            return Step::Rules;
        }
        prompts.len().checked_sub(1).map_or(Step::Cover, Step::Prompt)
    }

    // (answered, total) over the dots: each question, then the rules once read.
    fn progress(&self, prompts: &[Prompt], rules: bool) -> (usize, usize) {
        let answered = prompts.iter().filter(|p| self.answered(p)).count() + usize::from(rules && self.rules_read);
        (answered, prompts.len() + usize::from(rules))
    }

    fn leave_step(&mut self, prompts: &[Prompt]) {
        if let Step::Prompt(i) = self.step {
            self.passed.insert(prompts[i].id.clone());
        }
    }

    // The rules count as read once their end has scrolled into view, or when they fit without scrolling.
    fn note_rules_read(&mut self, window: &Window) {
        if self.rules_read {
            return;
        }
        let scroll = &self.rules_scroll;
        // Not laid out yet: ask again next frame, once the list has its size.
        if scroll.bounds().size.height <= px(0.) {
            return window.request_animation_frame();
        }
        let max = scroll.max_offset().y;
        if max > px(1.) && -scroll.offset().y < max - px(4.) {
            return;
        }
        self.rules_read = true;
        if self.warning == Some(Warning::Unread) {
            self.warning = None;
        }
    }
}

// Whether a just-joined server's onboarding should open: on, with questions for the flow, not yet done.
fn should_open(onboarding: &Onboarding, member_flags: u64) -> bool {
    onboarding.enabled && onboarding.onboarding_prompts().next().is_some() && member_flags & member_flags::COMPLETED_ONBOARDING == 0
}

fn rules_step(guild: &Guild, form: Option<&Load<Form>>) -> RulesStep {
    if !guild.has_feature(features::RULES_GATE) || guild.has_feature(features::MANUAL_APPROVAL) {
        return RulesStep::Off;
    }
    match form {
        Some(Load::Ready(form)) if !form.rules.is_empty() => RulesStep::Ready(form.clone()),
        Some(Load::Ready(_) | Load::Failed(_)) => RulesStep::Off,
        _ => RulesStep::Loading,
    }
}

impl Waffle {
    // Right after joining: fetch the server's onboarding and open it when it has questions for us.
    pub(crate) fn start_onboarding_if_needed(&mut self, guild_id: String, cx: &mut Context<Self>) {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return };
        if !guild.has_feature(features::COMMUNITY) || !guild.has_feature(features::ONBOARDING) {
            return;
        }
        let id = guild_id.clone();
        self.fetch_onboarding(&guild_id, move |this, cx| {
            let flags = this.guilds.iter().find(|g| g.id == id).map_or(0, |g| g.me.flags);
            let Some(Load::Ready(onboarding)) = this.join.onboarding.get(&id) else { return };
            if should_open(onboarding, flags) {
                this.begin_onboarding(OnboardingView::new(id), cx);
            }
        }, cx);
    }

    // (answered, total) when onboarding was closed halfway, for the Finish joining row.
    pub(crate) fn onboarding_progress(&self, guild_id: &str) -> Option<(usize, usize)> {
        let view = self.join.paused_onboarding.get(guild_id).or(self.join.onboard.as_ref().filter(|v| v.guild_id == guild_id))?;
        let prompts = self.onboarding_prompts(guild_id)?;
        Some(view.progress(&prompts, self.onboarding_rules(guild_id) != RulesStep::Off))
    }

    // Reopens onboarding at the first unanswered question.
    pub(crate) fn open_onboarding(&mut self, guild_id: String, cx: &mut Context<Self>) {
        let Some(prompts) = self.onboarding_prompts(&guild_id) else {
            let id = guild_id.clone();
            return self.fetch_onboarding(&guild_id, move |this, cx| {
                if this.onboarding_prompts(&id).is_some() {
                    this.open_onboarding(id, cx);
                }
            }, cx);
        };
        let rules = self.onboarding_rules(&guild_id) != RulesStep::Off;
        let mut view = self.join.paused_onboarding.remove(&guild_id).unwrap_or_else(|| OnboardingView::new(guild_id.clone()));
        view.step = view.resume_at(&prompts, rules);
        self.begin_onboarding(view, cx);
    }

    fn begin_onboarding(&mut self, view: OnboardingView, cx: &mut Context<Self>) {
        let guild_id = view.guild_id.clone();
        if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            guild.me.flags |= member_flags::STARTED_ONBOARDING;
            if guild.has_feature(features::RULES_GATE) && !guild.has_feature(features::MANUAL_APPROVAL) {
                self.fetch_form(&guild_id, cx);
            }
        }
        let on_cover = view.step == Step::Cover;
        if let Some(open) = self.join.onboard.replace(view) {
            self.join.paused_onboarding.insert(open.guild_id.clone(), open);
        }
        if on_cover {
            self.advance_cover_later(guild_id, cx);
        }
        cx.notify();
    }

    fn advance_cover_later(&mut self, guild_id: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COVER_HOLD).await;
            this.update(cx, |this, cx| {
                if this.join.onboard.as_ref().is_some_and(|v| v.guild_id == guild_id && v.step == Step::Cover) {
                    this.onboarding_next(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn onboarding_prompts(&self, guild_id: &str) -> Option<Vec<Prompt>> {
        let Some(Load::Ready(onboarding)) = self.join.onboarding.get(guild_id) else { return None };
        Some(onboarding.onboarding_prompts().cloned().collect())
    }

    fn onboarding_rules(&self, guild_id: &str) -> RulesStep {
        let Some(guild) = self.guilds.iter().find(|g| g.id == guild_id) else { return RulesStep::Off };
        rules_step(guild, self.join.forms.get(guild_id))
    }

    fn onboarding_next(&mut self, cx: &mut Context<Self>) {
        let Some(guild_id) = self.join.onboard.as_ref().map(|v| v.guild_id.clone()) else { return };
        let Some(prompts) = self.onboarding_prompts(&guild_id) else { return };
        let rules = self.onboarding_rules(&guild_id) != RulesStep::Off;
        let Some(view) = self.join.onboard.as_mut() else { return };
        match view.next(&prompts, rules) {
            Err(warning) => view.warning = Some(warning),
            Ok(next) => {
                view.leave_step(&prompts);
                view.warning = None;
                let Next::Go(step) = next else { return self.finish_onboarding(cx) };
                view.step = step;
                view.body_scroll.set_offset(point(px(0.), px(0.)));
            }
        }
        cx.notify();
    }

    fn onboarding_back(&mut self, cx: &mut Context<Self>) {
        let Some(guild_id) = self.join.onboard.as_ref().map(|v| v.guild_id.clone()) else { return };
        let prompts = self.onboarding_prompts(&guild_id).unwrap_or_default();
        let Some(view) = self.join.onboard.as_mut() else { return };
        (view.step, view.warning) = (view.back(&prompts), None);
        view.body_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn onboarding_pick(&mut self, prompt: &Prompt, option_id: &str, cx: &mut Context<Self>) {
        let Some(view) = self.join.onboard.as_mut() else { return };
        view.pick(prompt, option_id);
        view.warning = None;
        cx.notify();
    }

    // Closing keeps the picks and the step for Finish joining.
    fn close_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(mut view) = self.join.onboard.take() else { return };
        (view.warning, view.error) = (None, None);
        self.join.paused_onboarding.insert(view.guild_id.clone(), view);
        cx.notify();
    }

    // POST the answers, then agree to the rules when the flow had them.
    fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.join.onboard.as_mut().filter(|v| !v.sending) else { return };
        (view.sending, view.error) = (true, None);
        let (guild_id, picked) = (view.guild_id.clone(), view.picked.clone());
        let prompts = self.onboarding_prompts(&guild_id).unwrap_or_default();
        let form = match self.onboarding_rules(&guild_id) {
            RulesStep::Ready(form) => Some(form),
            _ => None,
        };
        let body = Onboarding::responses_body(&prompts, &picked, chrono::Utc::now().timestamp_millis());
        let responses = super::strings(&body["onboarding_responses"]);
        let id = guild_id.clone();
        self.server_call(api::answer(&guild_id, true, body), move |this, result, cx| {
            if let Err(error) = result {
                return this.onboarding_failed(&id, error);
            }
            let Some(form) = form else { return this.onboarding_done(&id, responses, false) };
            let call = api::agree(&id, &form);
            this.server_call(call, move |this, result, _| match result {
                Ok(_) => this.onboarding_done(&id, responses, true),
                Err(error) => this.onboarding_failed(&id, error),
            }, cx);
        }, cx);
        cx.notify();
    }

    fn onboarding_failed(&mut self, guild_id: &str, error: String) {
        let open = self.join.onboard.as_mut().filter(|v| v.guild_id == guild_id);
        let Some(view) = open.or(self.join.paused_onboarding.get_mut(guild_id)) else { return };
        (view.sending, view.error) = (false, Some(error));
    }

    fn onboarding_done(&mut self, guild_id: &str, responses: Vec<String>, agreed: bool) {
        if let Some(guild) = self.guilds.iter_mut().find(|g| g.id == guild_id) {
            // With manual approval (or rules we couldn't load) the member stays pending: the composer bar takes over.
            if agreed || !guild.has_feature(features::RULES_GATE) {
                guild.me.pending = false;
            }
            guild.me.flags |= member_flags::COMPLETED_ONBOARDING;
        }
        if let Some(Load::Ready(onboarding)) = self.join.onboarding.get_mut(guild_id) {
            onboarding.responses = responses;
        }
        if self.join.onboard.as_ref().is_some_and(|v| v.guild_id == guild_id) {
            self.join.onboard = None;
        }
        self.join.paused_onboarding.remove(guild_id);
    }

    pub(crate) fn onboarding_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let guild_id = self.join.onboard.as_ref()?.guild_id.clone();
        let guild = self.guilds.iter().find(|g| g.id == guild_id)?.clone();
        let prompts = self.onboarding_prompts(&guild_id)?;
        let rules = self.onboarding_rules(&guild_id);
        let view = self.join.onboard.as_mut()?;
        if view.step == Step::Rules && matches!(rules, RulesStep::Ready(_)) {
            view.note_rules_read(window);
        }
        let view = self.join.onboard.as_ref()?;
        // A splash that hasn't loaded (or failed to) leaves the plain page.
        let splash = guild.splash.clone().filter(|url| matches!(window.use_asset::<ImgResourceLoader>(&Resource::Uri(url.clone().into()), cx), Some(Ok(_))));
        let on_splash_cover = splash.is_some() && view.step == Step::Cover;
        let body = match view.step {
            Step::Cover => cover(&guild, splash.is_some(), cx),
            Step::Prompt(i) => self.prompt_step(&guild, &prompts, i, window, cx),
            Step::Rules => rules_body(&rules, &view.rules_scroll, cx),
        };
        let page = palette::surface(palette::Level::Page, cx);
        let close = if on_splash_cover { ui::round_button("onboarding-close", Glyph::X, "Close", 40.) } else { ui::icon_button("onboarding-close", Glyph::X, "Close", cx).large() };
        // Over a splash the questions are a card, so the server's image shows all around. It keeps one size
        // from step to step rather than resizing to each.
        let card = splash.is_some() && !on_splash_cover;
        let frame = div()
            .relative()
            .w_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .when(!on_splash_cover, |d| d.bg(page))
            .when(card, |d| d.max_w(px(SHEET_WIDTH)).max_h(px(SHEET_HEIGHT)).squircle(ui::RADIUS_XL).overflow_hidden().shadow(ui::shadow_pop(cx)))
            .child(ui::scroll_area(div().id("onboarding-body").child(body), &view.body_scroll))
            .child(self.onboarding_footer(&prompts, &rules, on_splash_cover, cx))
            .child(div().absolute().top(px(16.)).right(px(16.)).child(close.on_click(cx.listener(|this, _, _, cx| this.close_onboarding(cx)))));
        let root = div()
            .id("onboarding")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(page)
            .when_some(splash, |d, url| {
                // gpui has no text shadow: a dark wash over the splash keeps the cover's white text legible.
                d.p(px(SPLASH_INSET)).items_center().justify_center()
                    .child(img(url).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
                    .when(on_splash_cover, |d| d.child(div().absolute().inset_0().bg(palette::stage::bg().opacity(0.45))))
            })
            .child(frame);
        Some(root.into_any_element())
    }

    fn prompt_step(&self, guild: &Guild, prompts: &[Prompt], index: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = self.join.onboard.as_ref().expect("overlay checked the open view");
        let prompt = &prompts[index];
        let caption = div()
            .flex()
            .gap(px(6.))
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::muted(cx))
            .child(format!("Question {} of {}", index + 1, prompts.len()))
            .when(prompt.required, |d| d.child(div().text_color(palette::accent_ink(cx)).child("· Required")));
        let help = if prompt.single_select { "Pick one." } else { "Choose all that apply." };
        let options = match prompt.kind {
            PromptKind::MultipleChoice => option_grid(guild, prompt, &view.picked, cx),
            PromptKind::Dropdown => dropdown(prompt, &view.picked, window, cx),
        };
        let picked = prompt.options.iter().filter(|o| view.picked.contains(&o.id));
        let sentence = adds_of(guild, picked);
        let column = column()
            .child(caption)
            .child(heading(prompt.title.clone(), cx))
            .child(div().mb(px(16.)).text_size(px(14.)).text_color(palette::muted(cx)).child(help))
            .child(options)
            .child(
                div()
                    .mt(px(16.))
                    .min_h(px(24.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .text_size(px(14.))
                    .text_color(palette::muted(cx))
                    .children(gives(sentence.channels, sentence.roles).into_iter().flat_map(|p| piece(p, cx))),
            );
        centered(column)
    }

    fn onboarding_footer(&self, prompts: &[Prompt], rules: &RulesStep, on_splash_cover: bool, cx: &mut Context<Self>) -> Div {
        let view = self.join.onboard.as_ref().expect("overlay checked the open view");
        let has_rules = *rules != RulesStep::Off;
        let picked_here = matches!(view.step, Step::Prompt(i) if view.has_pick(&prompts[i]));
        let (label, tone) = match view.step {
            _ if view.is_last(prompts, has_rules) => ("Finish 🎉", Tone::Primary),
            Step::Prompt(_) if !picked_here => ("Skip", Tone::Secondary),
            _ => ("Next", Tone::Primary),
        };
        let read_hint = (view.step == Step::Rules && view.rules_read).then_some("By clicking \"Finish\", you have read and agreed to the Server Rules.");
        let (hint, alarm) = match (view.warning, &view.error) {
            (Some(warning), _) => (Some(warning.text().to_owned()), true),
            (None, Some(error)) => (Some(error.clone()), true),
            (None, None) => (read_hint.map(str::to_owned), false),
        };
        let back = view.can_go_back().then(|| {
            ui::button("onboarding-back", Tone::Ghost, cx)
                .large()
                .child(Icon::new(Glyph::ArrowLeft).size(px(18.)))
                .child("Back")
                .on_click(cx.listener(|this, _, _, cx| this.onboarding_back(cx)))
        });
        let next = ui::button("onboarding-next", tone, cx).large().loading(view.sending.then_some("Finishing…")).child(label).on_click(cx.listener(|this, _, _, cx| this.onboarding_next(cx)));
        let dots = (view.step != Step::Cover).then(|| progress_dots(view.step, prompts.len() + usize::from(has_rules), cx));
        // The hint gets its own line over the buttons, ending under the button it's about, instead of a
        // third of the row.
        let hint = hint.map(|hint| div().text_right().text_size(px(14.)).text_color(if alarm { palette::danger_ink(cx) } else { palette::muted(cx) }).child(hint));
        let row = div().flex().items_center().gap(px(12.)).child(div().flex_1().flex().children(back)).children(dots).child(div().flex_1().flex().justify_end().child(next));
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(24.))
            .py(px(16.))
            .when(!on_splash_cover, |d| d.border_t_1().border_color(palette::line(cx)))
            .children(hint)
            .child(row)
    }
}

// The cover: the server, "Welcome to …", over the splash in white or on the plain page.
fn cover(guild: &Guild, on_splash: bool, cx: &App) -> AnyElement {
    let (strong, soft) = if on_splash { (palette::stage::strong(), palette::stage::text()) } else { (palette::strong(cx), palette::muted(cx)) };
    div()
        .min_h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .px(px(24.))
        .py(px(56.))
        .text_center()
        // The initials' tile is a translucent wash: over the splash it needs the page under it to read.
        .child(div().mb(px(20.)).squircle(40.).bg(palette::surface(palette::Level::Page, cx)).child(ui::server_tile(guild.icon.as_deref(), &guild.name, 88., cx)))
        .child(div().mb(px(6.)).text_size(px(28.)).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(format!("Welcome to {}", guild.name)))
        .child(div().text_size(px(16.)).text_color(soft).child("Let's customize your experience"))
        .child(div().mt(px(40.)).text_size(px(12.)).text_color(if on_splash { soft } else { palette::faint(cx) }).child("Your responses will be treated in accordance with Discord's Privacy Policy"))
        .into_any_element()
}

fn rules_body(rules: &RulesStep, scroll: &ScrollHandle, cx: &App) -> AnyElement {
    let list = match rules {
        RulesStep::Ready(form) => rules_list(&form.rules, scroll, cx).into_any_element(),
        _ => ui::progress_indeterminate("onboarding-rules-loading", cx),
    };
    centered(
        column()
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child("One last step!"))
            .child(heading("Read & Agree to Server Rules".into(), cx))
            .child(div().mt(px(12.)).child(list)),
    )
}

// Numbered rows in one group, as the rules dialog draws them.
fn rules_list(rules: &[String], scroll: &ScrollHandle, cx: &App) -> Div {
    let last = rules.len().saturating_sub(1);
    let rows = rules.iter().enumerate().map(|(i, rule)| {
        div()
            .relative()
            .flex()
            .items_start()
            .gap(px(12.))
            .p(px(12.))
            .child(
                div()
                    .size(px(24.))
                    .flex_none()
                    .rounded_full()
                    .bg(palette::accent_soft(cx))
                    .text_color(palette::accent_ink(cx))
                    .text_size(px(12.))
                    .font_weight(FontWeight::BOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child((i + 1).to_string()),
            )
            .child(div().flex_1().min_w_0().text_size(px(15.)).line_height(relative(1.45)).text_color(palette::strong(cx)).child(rule.clone()))
            .when(i < last, |d| d.child(ui::inset_rule(12., cx).left(px(48.))))
    });
    let list = div().id("onboarding-rules").max_h(px(RULES_HEIGHT)).overflow_y_scroll().track_scroll(scroll).flex().flex_col().children(rows);
    div().relative().squircle(ui::RADIUS_LG).bg(palette::surface(palette::Level::Group, cx)).p(px(4.)).child(list).vertical_scrollbar(scroll)
}

fn column() -> Div {
    div().w_full().max_w(px(640.)).flex().flex_col().gap(px(6.))
}

fn centered(column: Div) -> AnyElement {
    div().w_full().flex().justify_center().px(px(24.)).pt(px(56.)).pb(px(24.)).child(column).into_any_element()
}

fn heading(text: String, cx: &App) -> Div {
    div().mb(px(4.)).text_size(px(28.)).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(text)
}

fn progress_dots(step: Step, total: usize, cx: &App) -> Div {
    let now = match step {
        Step::Prompt(i) => i,
        _ => total.saturating_sub(1),
    };
    div().flex_none().flex().gap(px(6.)).children((0..total).map(|i| {
        let fill = match i.cmp(&now) {
            std::cmp::Ordering::Less => palette::accent(),
            std::cmp::Ordering::Equal => palette::strong(cx),
            std::cmp::Ordering::Greater => palette::line_strong(cx),
        };
        div().size(px(8.)).rounded_full().bg(fill)
    }))
}

fn option_grid(guild: &Guild, prompt: &Prompt, picked: &HashSet<String>, cx: &mut Context<Waffle>) -> AnyElement {
    let cards: Vec<AnyElement> = prompt
        .options
        .iter()
        .map(|option| {
            let on = picked.contains(&option.id);
            let adds = adds_of(guild, iter::once(option));
            let ink = if on { palette::accent_ink(cx) } else { palette::muted(cx) };
            let gets = adds.channels.iter().map(|c| div().min_w_0().max_w_full().truncate().child(format!("#{c}")).into_any_element()).chain(adds.roles.iter().map(|(name, color)| ui::role_chip(name.clone(), role_color(*color, cx), cx).into_any_element()));
            let gets: Vec<AnyElement> = gets.enumerate().flat_map(|(i, el)| (i > 0).then(|| div().child("·").into_any_element()).into_iter().chain(iter::once(el))).collect();
            let text = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(15.)).line_height(px(TITLE_LINE)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(option.title.clone()))
                .when_some(option.description.clone(), |d, desc| d.child(div().text_size(px(14.)).text_color(palette::muted(cx)).child(desc)))
                .when(!gets.is_empty(), |d| d.child(div().mt(px(2.)).flex().flex_wrap().items_center().gap(px(4.)).text_size(px(12.)).text_color(ink).children(gets)));
            let edge = if on { palette::accent().opacity(0.7) } else { palette::line(cx) };
            let hover_edge = if on { edge } else { palette::line_strong(cx) };
            let card = div()
                .id(gpui_kit::SharedString::from(format!("onboarding-option-{}", option.id)))
                .min_h(px(56.))
                .flex()
                // The emoji sits on the title's line, not mid-card, so it stays put as descriptions wrap.
                .items_start()
                .gap(px(12.))
                .px(px(14.))
                .py(px(10.))
                .squircle(ui::RADIUS_MD)
                .border_1()
                .border_color(edge)
                .bg(if on { palette::accent_soft(cx) } else { palette::surface(palette::Level::Raised, cx) })
                .hover(move |s| s.border_color(hover_edge))
                .cursor_pointer()
                .children(option.emoji.as_ref().and_then(option_emoji))
                .child(text);
            let listener = cx.listener({
                let (prompt, id) = (prompt.clone(), option.id.clone());
                move |this: &mut Waffle, _: &gpui_kit::ClickEvent, _, cx| this.onboarding_pick(&prompt, &id, cx)
            });
            ui::pressable(card.on_click(listener), 0.98).into_any_element()
        })
        .collect();
    div().grid().grid_cols(2).gap(px(8.)).children(cards).into_any_element()
}

fn dropdown(prompt: &Prompt, picked: &HashSet<String>, window: &mut Window, cx: &mut Context<Waffle>) -> AnyElement {
    let rows: Vec<AnyElement> = prompt
        .options
        .iter()
        .map(|option| {
            let state = if picked.contains(&option.id) { Check::On } else { Check::Off };
            let id = format!("onboarding-option-{}", option.id);
            let mark = ui::check_live(gpui_kit::SharedString::from(format!("{id}-check")), state, window, cx);
            let title = match option.emoji.as_ref().filter(|e| e.id.is_none()).and_then(|e| e.name.as_deref()) {
                Some(emoji) => format!("{emoji} {}", option.title),
                None => option.title.clone(),
            };
            let listener = cx.listener({
                let (prompt, option_id) = (prompt.clone(), option.id.clone());
                move |this: &mut Waffle, _: &gpui_kit::ClickEvent, _, cx| this.onboarding_pick(&prompt, &option_id, cx)
            });
            ui::pick_row(gpui_kit::SharedString::from(id), mark, title, option.description.clone().map(Into::into), cx).on_click(listener).into_any_element()
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(6.))
        .squircle(ui::RADIUS_MD)
        .border_1()
        .border_color(palette::line(cx))
        .bg(palette::surface(palette::Level::Raised, cx))
        .children(rows)
        .into_any_element()
}

// A unicode emoji as text, a custom one as its image.
// One title line tall, centred on it.
fn option_emoji(emoji: &OptionEmoji) -> Option<AnyElement> {
    let slot = div().w(px(28.)).h(px(TITLE_LINE)).flex_none().flex().items_center().justify_center();
    if let Some(url) = emoji.url() {
        return Some(slot.child(img(url).size(px(22.)).object_fit(ObjectFit::Contain)).into_any_element());
    }
    let name = emoji.name.clone()?;
    // The emoji font draws above the line box's middle: nudged down to the title's optical centre.
    Some(slot.child(div().relative().top(px(2.)).text_size(px(20.)).line_height(px(TITLE_LINE)).child(name)).into_any_element())
}

fn role_color(color: u32, cx: &App) -> Hsla {
    if color == 0 {
        return palette::status_in(Status::Offline, cx);
    }
    rgb(color).into()
}

// What options add: channel names we can see, then roles (name, colour), each once.
#[derive(Default)]
struct Adds {
    channels: Vec<String>,
    roles: Vec<(String, u32)>,
}

fn adds_of<'a>(guild: &Guild, options: impl Iterator<Item = &'a PromptOption>) -> Adds {
    let mut adds = Adds::default();
    for option in options {
        for name in option.channel_ids.iter().filter_map(|id| guild.channel(id)).map(|c| c.name.clone()) {
            if !adds.channels.contains(&name) {
                adds.channels.push(name);
            }
        }
        for role in option.role_ids.iter().filter_map(|id| guild.roles.iter().find(|r| &r.id == id)) {
            if !adds.roles.iter().any(|(name, _)| *name == role.name) {
                adds.roles.push((role.name.clone(), role.color));
            }
        }
    }
    adds
}

// A run of the "gives" sentence: words, a bold channel, a role chip.
#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Text(String),
    Channel(String),
    Role(String, u32),
}

// What the picks open up, said the way a person would: "Nice, that opens up #a and #b for you, and you'll
// wear the Ranked role."
fn gives(channels: Vec<String>, roles: Vec<(String, u32)>) -> Vec<Piece> {
    let text = |s: &str| Piece::Text(s.to_owned());
    let wear = (!roles.is_empty()).then(|| {
        let noun = if roles.len() > 1 { " roles" } else { " role" };
        [vec![text("the ")], and_list(roles.into_iter().map(|(name, color)| Piece::Role(name, color)).collect()), vec![text(noun)]].concat()
    });
    let channels = and_list(channels.into_iter().map(Piece::Channel).collect());
    match (channels.is_empty(), wear) {
        (true, None) => Vec::new(),
        (false, Some(wear)) => [vec![text("Nice, that opens up ")], channels, vec![text(" for you, and you'll wear ")], wear, vec![text(".")]].concat(),
        (false, None) => [vec![text("Nice, that opens up ")], channels, vec![text(" for you.")]].concat(),
        (true, Some(wear)) => [vec![text("Nice, you'll wear ")], wear, vec![text(".")]].concat(),
    }
}

// "a", "a and b", "a, b and c".
fn and_list(items: Vec<Piece>) -> Vec<Piece> {
    let n = items.len();
    items
        .into_iter()
        .enumerate()
        .flat_map(|(i, item)| {
            let joint = match n - i {
                1 => None,
                2 => Some(" and "),
                _ => Some(", "),
            };
            iter::once(item).chain(joint.map(|j| Piece::Text(j.to_owned())))
        })
        .collect()
}

// Text splits into words so the sentence wraps around its chips.
fn piece(piece: Piece, cx: &App) -> Vec<AnyElement> {
    match piece {
        Piece::Text(text) => text.split_inclusive(' ').map(|word| div().whitespace_nowrap().child(word.to_owned()).into_any_element()).collect(),
        Piece::Channel(name) => vec![div().min_w_0().max_w_full().truncate().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(format!("#{name}")).into_any_element()],
        Piece::Role(name, color) => vec![ui::role_chip(name, role_color(color, cx), cx).mx(px(2.)).into_any_element()],
    }
}

// Splash from a public server (Discord Developers), so the demo shows a real image over the network.
const DEMO_SPLASH: (&str, &str) = ("613425648685547541", "56394bca8b4f60282ea833e8703b5fcf");

pub fn demo(waffle: &mut Waffle, scene: &str, _window: &mut Window, cx: &mut Context<Waffle>) {
    let Some(guild) = waffle.guilds.iter_mut().find(|g| g.id == "1") else { return };
    guild.splash = (scene != "onboarding-plain").then(|| crate::discord::splash_url(DEMO_SPLASH.0, DEMO_SPLASH.1));
    let mut view = OnboardingView::new("1".into());
    match scene {
        "onboarding-question" => {
            view.step = Step::Prompt(0);
            view.picked.insert("o-ranked".into());
        }
        "onboarding-dropdown" => view.step = Step::Prompt(2),
        "onboarding-rules" => view.step = Step::Rules,
        "onboarding-required" => (view.step, view.warning) = (Step::Prompt(0), Some(Warning::Required)),
        _ => {}
    }
    waffle.begin_onboarding(view, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::joining::fixtures;

    fn plain(pieces: &[Piece]) -> String {
        pieces
            .iter()
            .map(|p| match p {
                Piece::Text(t) => t.clone(),
                Piece::Channel(c) => format!("#{c}"),
                Piece::Role(r, _) => format!("[{r}]"),
            })
            .collect()
    }

    #[test]
    fn gives_reads_like_a_sentence() {
        let roles = |names: &[&str]| names.iter().map(|n| (n.to_string(), 0)).collect::<Vec<_>>();
        let chans = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(plain(&gives(chans(&["a", "b"]), roles(&["Ranked"]))), "Nice, that opens up #a and #b for you, and you'll wear the [Ranked] role.");
        assert_eq!(plain(&gives(chans(&["a"]), vec![])), "Nice, that opens up #a for you.");
        assert_eq!(plain(&gives(vec![], roles(&["EU", "NA"]))), "Nice, you'll wear the [EU] and [NA] roles.");
        assert_eq!(plain(&gives(chans(&["a", "b", "c"]), vec![])), "Nice, that opens up #a, #b and #c for you.");
        assert!(gives(vec![], vec![]).is_empty());
    }

    #[test]
    fn steps_validate_and_resume() {
        let prompts: Vec<Prompt> = fixtures::onboarding().onboarding_prompts().cloned().collect();
        assert_eq!(prompts.len(), 3);
        let mut view = OnboardingView::new("1".into());
        assert_eq!(view.next(&prompts, true), Ok(Next::Go(Step::Prompt(0))));
        view.step = Step::Prompt(0);
        assert_eq!(view.next(&prompts, true), Err(Warning::Required));
        view.pick(&prompts[0], "o-ranked");
        view.pick(&prompts[0], "o-clips");
        assert!(view.picked.contains("o-clips") && !view.picked.contains("o-ranked"), "single select swaps");
        assert_eq!(view.next(&prompts, true), Ok(Next::Go(Step::Prompt(1))));
        view.leave_step(&prompts);
        view.step = Step::Prompt(2);
        assert!(!view.is_last(&prompts, true));
        assert!(view.is_last(&prompts, false));
        assert_eq!(view.next(&prompts, false), Ok(Next::Finish));
        assert_eq!(view.next(&prompts, true), Ok(Next::Go(Step::Rules)));
        view.step = Step::Rules;
        assert_eq!(view.next(&prompts, true), Err(Warning::Unread));
        view.rules_read = true;
        assert_eq!(view.next(&prompts, true), Ok(Next::Finish));
        assert_eq!(view.back(&prompts), Step::Prompt(2));
        assert_eq!(view.progress(&prompts, true), (2, 4));
        assert_eq!(view.resume_at(&prompts, true), Step::Prompt(1));
    }

    #[test]
    fn opens_only_when_there_is_something_to_answer() {
        let onboarding = fixtures::onboarding();
        assert!(should_open(&onboarding, 0));
        assert!(should_open(&onboarding, member_flags::STARTED_ONBOARDING));
        assert!(!should_open(&onboarding, member_flags::COMPLETED_ONBOARDING));
        assert!(!should_open(&Onboarding { enabled: false, ..onboarding.clone() }, 0));
        let prompts = onboarding.prompts.iter().cloned().map(|p| Prompt { in_onboarding: false, ..p }).collect();
        assert!(!should_open(&Onboarding { prompts, ..onboarding }, 0));
    }
}
