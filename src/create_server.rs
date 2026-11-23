// Add a Server, Discord's dialog: Create My Own or a template, the intent question, name and icon, or Join a Server.
use std::{collections::HashSet, path::PathBuf};

use futures::channel::mpsc;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, FocusHandle, Focusable as _, FontWeight, Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState},
    },
    div, img, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _,
    px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    discord::perm,
    icons::Glyph,
    palette,
    server_settings::{api::image_data, perms::MANAGE_GUILD_EXPRESSIONS, view::mime_of},
    ui::{self, Notice, Squircle as _, Tone},
};

pub(crate) struct CreateServer {
    step: Step,
    // None is Create My Own.
    template: Option<usize>,
    expanded: HashSet<usize>,
    name: Entity<InputState>,
    invite: Entity<InputState>,
    icon: Option<PickedIcon>,
    busy: bool,
    error: Option<String>,
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Templates,
    Intent,
    Customize,
    Join,
}

struct PickedIcon {
    path: PathBuf,
    data: String,
}

#[derive(Clone, Copy)]
enum Chan {
    Category(&'static str),
    Text(&'static str),
    Voice(&'static str),
}

struct Template {
    label: &'static str,
    code: &'static str,
    glyph: Glyph,
    hue: f32,
    channels: &'static [Chan],
    officers: bool,
}

use Chan::{Category as C, Text as T, Voice as V};

// Discord's template table (docs/discord-create-server-re.md), in the order the dialog lists them. The hidden
// Clubs template is left out, as Discord leaves it out of the list.
const TEMPLATES: [Template; 6] = [
    Template { label: "Gaming", code: "hvtBQMfw6uSJ", glyph: Glyph::Game, hue: 255., officers: false, channels: &[C("Text Channels"), T("general"), T("clips-and-highlights"), C("Voice Channels"), V("Lobby"), V("Gaming")] },
    Template { label: "Friends", code: "hgM48av5Q69A", glyph: Glyph::Heart, hue: 55., officers: false, channels: &[C("Text Channels"), T("general"), T("games"), T("music"), C("Voice Channels"), V("Lounge"), V("Stream Room")] },
    Template {
        label: "Study Group",
        code: "FbwUwRp4j8Es",
        glyph: Glyph::Lightbulb,
        hue: 155.,
        officers: false,
        channels: &[C("Information"), T("welcome-and-rules"), T("notes-resources"), C("Text Channels"), T("general"), T("homework-help"), T("session-planning"), T("off-topic"), C("Voice Channels"), V("Lounge"), V("Study Room 1"), V("Study Room 2")],
    },
    Template {
        label: "School Club",
        code: "Ctg7PUHcQmZu",
        glyph: Glyph::Flag,
        hue: 25.,
        officers: true,
        channels: &[C("Information"), T("welcome-and-rules"), T("announcements"), T("resources"), C("Text Channels"), T("general"), T("meeting-plans"), T("off-topic"), C("Voice Channels"), V("Lounge"), V("Meeting Room 1"), V("Meeting Room 2")],
    },
    Template {
        label: "Local Community",
        code: "64UDvRNCC52Y",
        glyph: Glyph::Leaf,
        hue: 205.,
        officers: false,
        channels: &[C("Information"), T("welcome-and-rules"), T("announcements"), T("resources"), C("Text Channels"), T("general"), T("meeting-plans"), T("off-topic"), C("Voice Channels"), V("Lounge"), V("Meeting Room")],
    },
    Template {
        label: "Artists & Creators",
        code: "6exdzMgjZgah",
        glyph: Glyph::Palette,
        hue: 305.,
        officers: false,
        channels: &[C("Information"), T("welcome-and-rules"), T("announcements"), C("Text Channels"), T("general"), T("events"), T("ideas-and-feedback"), C("Voice Channels"), V("Lounge"), V("Community Hangout"), V("Stream Room")],
    },
];

const CREATE_MY_OWN_CODE: &str = "2TffvPucqHkN";
const SCHOOL_CLUB: usize = 3;
// Longer channel lists start clamped under a fade.
const CHIPS_SHOWN: usize = 5;
const NAME_MAX: usize = 100;
const NEAR_LIMIT: usize = 10;
const INVITE_EXAMPLES: [&str; 3] = ["hTKzmak", "https://discord.gg/hTKzmak", "https://discord.gg/wumpus-friends"];
// Not verified: Discord's client-side default role permissions; this is Discord's documented @everyone set
// (create invite, reactions, stream, view, send, embed, attach, history, mention everyone, external emoji,
// connect, speak, voice activity, change nickname).
const DEFAULT_PERMISSIONS: u64 = (1 << 0) | (1 << 6) | (1 << 9) | (1 << 10) | (1 << 11) | (1 << 14) | (1 << 15) | (1 << 16) | (1 << 17) | (1 << 18) | (1 << 20) | (1 << 21) | (1 << 25) | (1 << 26);
const SEND_TTS_MESSAGES: u64 = 1 << 12;
const OFFICER_PERMISSIONS: u64 = DEFAULT_PERMISSIONS | perm::MANAGE_ROLES | perm::MANAGE_CHANNELS | perm::KICK_MEMBERS | perm::BAN_MEMBERS | perm::MANAGE_NICKNAMES | MANAGE_GUILD_EXPRESSIONS | perm::MANAGE_MESSAGES | SEND_TTS_MESSAGES;

impl Waffle {
    pub(crate) fn open_create_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        let default_name = self.default_server_name();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(default_name).validate(|text, _| text.chars().count() <= NAME_MAX));
        let invite = cx.new(|cx| InputState::new(window, cx).placeholder("https://discord.gg/hTKzmak"));
        let subscriptions = [
            cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_create_server(cx),
                _ => cx.notify(),
            }),
            cx.subscribe_in(&invite, window, |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_join_server(cx),
                _ => cx.notify(),
            }),
        ];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.create_server = Some(CreateServer { step: Step::Templates, template: None, expanded: HashSet::new(), name, invite, icon: None, busy: false, error: None, focus, _subscriptions: subscriptions });
        cx.notify();
    }

    // Discord's mod+shift+N: opens the dialog, or jumps to Join when it's already open.
    pub(crate) fn add_server_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.me.is_none() {
            return;
        }
        if self.create_server.is_none() {
            return self.open_create_server(window, cx);
        }
        self.create_server_step(Step::Join, window, cx);
    }

    fn close_create_server(&mut self, cx: &mut Context<Self>) {
        self.create_server = None;
        cx.notify();
    }

    fn default_server_name(&self) -> String {
        let account = &self.my_account;
        let name = account.global_name.clone().filter(|n| !n.is_empty()).or_else(|| (!account.username.is_empty()).then(|| account.username.clone()));
        let name = name.or_else(|| self.me.as_ref().map(|m| m.name.clone())).unwrap_or_default();
        format!("{name}'s server")
    }

    fn create_server_step(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.create_server.as_mut() else { return };
        form.step = step;
        form.error = None;
        match step {
            Step::Customize => form.name.update(cx, |state, cx| state.focus(window, cx)),
            Step::Join => form.invite.update(cx, |state, cx| state.focus(window, cx)),
            Step::Templates | Step::Intent => form.focus.focus(window, cx),
        }
        cx.notify();
    }

    fn pick_create_server_template(&mut self, template: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_limit().reached {
            return;
        }
        if let Some(form) = self.create_server.as_mut() {
            form.template = template;
        }
        self.create_server_step(Step::Intent, window, cx);
    }

    fn create_server_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.create_server.as_mut() else { return };
        if form.busy {
            return;
        }
        let step = if form.step == Step::Customize { Step::Intent } else { Step::Templates };
        if step == Step::Templates {
            form.template = None;
        }
        self.create_server_step(step, window, cx);
    }

    fn toggle_template_channels(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(form) = self.create_server.as_mut() else { return };
        if !form.expanded.remove(&i) {
            form.expanded.insert(i);
        }
        cx.notify();
    }

    // Same as Server settings' icon: the picked file goes out as a base64 data URI.
    fn pick_server_icon(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let Ok(bytes) = std::fs::read(&path) else { return };
            let data = image_data(mime_of(&path), &bytes);
            this.update(cx, |this, cx| {
                if let Some(form) = this.create_server.as_mut() {
                    form.icon = Some(PickedIcon { path, data });
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn submit_create_server(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let Some(form) = self.create_server.as_mut() else { return };
        let name = form.name.read(cx).value().trim().to_owned();
        if name.is_empty() || form.busy || form.step != Step::Customize {
            return;
        }
        let Some(client) = client else { return };
        let body = guild_body(&name, form.icon.as_ref().map(|i| i.data.as_str()), form.template.map(|i| &TEMPLATES[i]));
        form.busy = true;
        form.error = None;
        let captcha = captcha_opener(cx);
        cx.spawn(async move |this, cx| {
            let created = client.call_with_captcha(Method::POST, "/guilds".into(), Some(body), captcha).await;
            let created = created.map_err(String::from).and_then(|guild| guild["id"].as_str().map(str::to_owned).ok_or_else(|| "Discord didn't send the new server back".to_owned()));
            this.update(cx, |this, cx| this.finish_create_server(created, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn submit_join_server(&mut self, cx: &mut Context<Self>) {
        let limit = self.server_limit();
        let client = self.client.clone();
        let Some(form) = self.create_server.as_mut() else { return };
        if form.busy || form.step != Step::Join {
            return;
        }
        let input = form.invite.read(cx).value().to_string();
        let error = match invite_code(&input) {
            None => Some("Please enter a valid invite link or invite code."),
            Some(_) if limit.reached => Some("You must leave a server first to join."),
            Some(_) => None,
        };
        form.error = error.map(str::to_owned);
        cx.notify();
        let (None, Some(code), Some(client)) = (error, invite_code(&input).map(str::to_owned), client) else { return };
        form.busy = true;
        let captcha = captcha_opener(cx);
        cx.spawn(async move |this, cx| {
            let joined = client.join_invite(&code, captcha).await.map_err(|e| join_error(&e));
            this.update(cx, |this, cx| this.finish_create_server(joined, cx)).ok();
        })
        .detach();
    }

    // Created or joined: the dialog closes and the server opens once its GUILD_CREATE lands.
    fn finish_create_server(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        match result {
            Ok(guild_id) => {
                self.create_server = None;
                self.open_joined_guild(guild_id, cx);
            }
            Err(error) => {
                if let Some(form) = self.create_server.as_mut() {
                    form.busy = false;
                    form.error = Some(error);
                }
            }
        }
        cx.notify();
    }

    fn server_limit(&self) -> ServerLimit {
        // Not verified: that only Nitro (premium_type 2) lifts the cap to 200, not Classic or Basic.
        let max = if self.my_account.premium_type == 2 { 200 } else { 100 };
        let count = self.guilds.len();
        ServerLimit { count, max, reached: count >= max, near: count < max && count + NEAR_LIMIT >= max }
    }

    pub(crate) fn render_create_server(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.create_server.as_ref()?;
        let limit = self.server_limit();
        let body = match form.step {
            Step::Templates => templates_step(form, limit, window, cx),
            Step::Intent => intent_step(cx),
            Step::Customize => customize_step(form, cx),
            Step::Join => join_step(form, limit, cx),
        };
        let body = body
            .track_focus(&form.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    this.close_create_server(cx);
                }
            }));
        let backdrop = div().id("create-server-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_create_server(cx)));
        Some(ui::modal(backdrop, ui::dialog("create-server", 480., body, window, cx), cx))
    }
}

#[derive(Clone, Copy)]
struct ServerLimit {
    count: usize,
    max: usize,
    reached: bool,
    near: bool,
}

fn templates_step(form: &CreateServer, limit: ServerLimit, window: &mut Window, cx: &mut Context<Waffle>) -> Div {
    let off = limit.reached;
    let own = card_row("create-server-own", ui::icon_tile(Glyph::Plus, palette::accent(), palette::on_accent()), "Create my own", None, off, cx)
        .when(!off, |r| r.on_click(cx.listener(|this, _, window, cx| this.pick_create_server_template(None, window, cx))));
    let rows: Vec<AnyElement> = TEMPLATES.iter().enumerate().map(|(i, t)| template_row(i, t, form.expanded.contains(&i), off, cx).into_any_element()).collect();
    let templates = ui::col().gap_2().child(section_label("Start from a template", cx)).child(ui::col().gap_0p5().children(rows));
    // The head and the invite footer stay put; the rows scroll once the window is too short for them.
    let room = (f32::from(window.viewport_size().height) * 0.92 - 330.).max(200.);
    let list = ui::col().id("create-server-templates").gap_4().max_h(px(room)).overflow_y_scroll().child(own).child(templates);
    let join = ui::button("create-server-join", Tone::Secondary, cx).w_full().disabled(off).child("Join a server").on_click(cx.listener(move |this, _, window, cx| {
        if !off {
            this.create_server_step(Step::Join, window, cx);
        }
    }));
    let foot = ui::col().gap_2p5().items_center().child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Have an invite already?")).child(join);
    ui::dialog_body()
        .child(head("Create your server", "Your server is where you and your friends hang out. Make yours and start talking.", cx))
        .children(limit_notice(limit, cx))
        .child(list)
        .child(foot.pt_2())
}

fn intent_step(cx: &mut Context<Waffle>) -> Div {
    let tile = |glyph, hue| ui::icon_tile(glyph, template_fill(hue), gpui_kit::white());
    let to_customize = |id: &'static str, glyph, hue, label: &'static str, cx: &mut Context<Waffle>| card_row(id, tile(glyph, hue), label, None, false, cx).on_click(cx.listener(|this, _, window, cx| this.create_server_step(Step::Customize, window, cx)));
    let rows = ui::col()
        .gap_2()
        .child(to_customize("create-server-intent-friends", Glyph::Heart, 55., "For me and my friends", cx))
        .child(to_customize("create-server-intent-community", Glyph::Users, 205., "For a club or community", cx));
    let skip = ui::button("create-server-skip", Tone::Link, cx).child("skip this question").on_click(cx.listener(|this, _, window, cx| this.create_server_step(Step::Customize, window, cx)));
    let hint = ui::row().flex_wrap().justify_center().gap_1().text_size(ui::TEXT_DETAIL).text_color(palette::muted(cx)).child("Not sure? You can").child(skip).child("for now.");
    let back = back_button(cx);
    ui::dialog_body()
        .child(head("Tell us more about your server", "In order to help you with your setup, is your new server for just a few friends or a larger community?", cx))
        .child(rows)
        .child(hint)
        .child(ui::row().items_stretch().child(back))
}

fn customize_step(form: &CreateServer, cx: &mut Context<Waffle>) -> Div {
    let name = form.name.read(cx).value();
    let blank = name.trim().is_empty();
    let focus = form.name.read(cx).focus_handle(cx);
    let field = ui::text_field("create-server-name", Input::new(&form.name).appearance(false), &focus).error(form.error.is_some());
    let guidelines = ui::button("create-server-guidelines", Tone::Link, cx).text_size(ui::TEXT_XS).child("Community Guidelines").on_click(|_, _, cx| cx.open_url("https://discord.com/guidelines"));
    let legal = ui::row().flex_wrap().gap(px(3.)).text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child("By creating a server, you agree to Discord's").child(ui::row().child(guidelines).child("."));
    let name_group = ui::col()
        .gap_2()
        .child(required_label("Server name", cx))
        .child(field)
        .children(form.error.clone().map(|e| ui::field_error(e, cx)))
        .child(legal);
    let create = ui::button("create-server-create", Tone::Primary, cx)
        .flex_1()
        .disabled(blank)
        .loading(form.busy.then_some("Creating"))
        .child("Create")
        .on_click(cx.listener(|this, _, _, cx| this.submit_create_server(cx)));
    let actions = ui::row().items_stretch().gap_2().child(back_button(cx).flex_1()).child(create);
    ui::dialog_body()
        .child(head("Customize your server", "Give your new server a personality with a name and an icon. You can always change it later.", cx))
        .child(ui::row().items_stretch().justify_center().child(upload_tile(form.icon.as_ref().map(|i| i.path.clone()), cx)))
        .child(name_group)
        .child(actions)
}

fn join_step(form: &CreateServer, limit: ServerLimit, cx: &mut Context<Waffle>) -> Div {
    let focus = form.invite.read(cx).focus_handle(cx);
    let field = ui::text_field("create-server-invite", Input::new(&form.invite).appearance(false), &focus).error(form.error.is_some());
    let invite_group = ui::col().gap_2().child(required_label("Invite link", cx)).child(field).children(form.error.clone().map(|e| ui::field_error(e, cx)));
    let examples: Vec<AnyElement> = INVITE_EXAMPLES.iter().enumerate().map(|(i, example)| invite_example(i, example, &form.invite, cx).into_any_element()).collect();
    let examples = ui::col().gap_1p5().child(section_label("Invites should look like", cx)).child(ui::col().gap_0p5().children(examples));
    let discovery = card_row("create-server-discovery", ui::icon_tile(Glyph::Search, template_fill(155.), gpui_kit::white()), "Don't have an invite?", Some("Check out Discoverable communities in Server Discovery."), false, cx)
        .on_click(|_, _, cx| cx.open_url("https://discord.com/servers"));
    let join = ui::button("create-server-join-submit", Tone::Primary, cx)
        .flex_1()
        .disabled(limit.reached)
        .loading(form.busy.then_some("Joining"))
        .child("Join server")
        .on_click(cx.listener(|this, _, _, cx| this.submit_join_server(cx)));
    let actions = ui::row().items_stretch().gap_2().child(back_button(cx).flex_1()).child(join);
    ui::dialog_body()
        .child(head("Join a server", "Enter an invite below to join an existing server", cx))
        .children(limit_notice(limit, cx))
        .child(invite_group)
        .child(examples)
        .child(discovery)
        .child(actions)
}

// The dialog's title, its line under it and the close button.
fn head(title: &'static str, subtitle: &'static str, cx: &mut Context<Waffle>) -> impl IntoElement {
    ui::row()
        .items_start()
        .gap_3()
        .child(
            ui::col()
                .flex_1()
                .gap_1()
                .child(div().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title))
                .child(div().text_size(ui::TEXT_SM).line_height(ui::LINE_SM).text_color(palette::muted(cx)).child(subtitle)),
        )
        .child(ui::icon_button("create-server-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_create_server(cx))))
}

fn section_label(text: &'static str, cx: &App) -> impl IntoElement {
    div().text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(text)
}

fn required_label(text: &'static str, cx: &App) -> impl IntoElement {
    ui::row().items_stretch().gap_1().text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(text).child(div().text_color(palette::danger_ink(cx)).child("*"))
}

// Near the cap a faint count; at it, the danger callout that explains why everything below is off.
fn limit_notice(limit: ServerLimit, cx: &App) -> Option<AnyElement> {
    if limit.reached {
        let title = format!("You're in {} servers, the most you can join.", limit.max);
        return Some(ui::callout(Notice::Danger, Glyph::Warning, title, "Leave one to create or join another.", cx).into_any_element());
    }
    if !limit.near {
        return None;
    }
    let line = format!("You're in {} of {} servers.", limit.count, limit.max);
    Some(div().w_full().text_center().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(line).into_any_element())
}

fn back_button(cx: &mut Context<Waffle>) -> ui::Control {
    ui::button("create-server-back", Tone::Secondary, cx).child(Icon::new(Glyph::ChevronLeft).size(ui::ICON_MD)).child("Back").on_click(cx.listener(|this, _, window, cx| this.create_server_back(window, cx)))
}

fn template_fill(hue: f32) -> Hsla {
    palette::oklch(0.58, 0.13, hue)
}

// A bordered row leading to the next step: the tile, the label (and a line under it), a chevron.
fn card_row(id: &'static str, tile: impl IntoElement, label: &'static str, detail: Option<&'static str>, off: bool, cx: &App) -> Stateful<Div> {
    let hover = palette::hover(cx);
    let text = ui::col()
        .flex_1()
        .gap_0p5()
        .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(label))
        .children(detail.map(|d| div().text_size(ui::TEXT_XS).line_height(ui::LINE_XS).text_color(palette::muted(cx)).child(d)));
    ui::row()
        .id(id)
        .gap_3()
        .p_3()
        .squircle(ui::RADIUS_LG)
        .border_1()
        .border_color(palette::line_strong(cx))
        .child(tile)
        .child(text)
        .child(div().flex_none().text_color(palette::faint(cx)).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)))
        .when(off, |r| r.opacity(0.5).cursor_not_allowed())
        .when(!off, |r| r.cursor_pointer().hover(move |s| s.bg(hover)))
}

// A template: its tile at the top beside the name, the channels it comes with under the name, and for a long
// list the first lines under a fade with a button that shows the rest in place.
fn template_row(i: usize, template: &Template, expanded: bool, off: bool, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (hover, strong, faint) = (palette::hover(cx), palette::strong(cx), palette::faint(cx));
    let chans = channel_chips(template);
    let long = chans.len() > CHIPS_SHOWN;
    let group: SharedString = format!("create-server-template-{i}").into();
    let clamped = long && !expanded;
    let chips = ui::row().items_stretch()
        .relative()
        .flex_wrap()
        .gap_x_2p5()
        .gap_y_1()
        .text_size(ui::TEXT_XS)
        .line_height(ui::LINE_XS)
        .text_color(palette::muted(cx))
        .children(chans.into_iter().map(|(glyph, name)| channel_chip(glyph, name, faint)))
        .when(clamped, |d| d.max_h(px(34.)).overflow_hidden().child(fade(group.clone(), palette::surface(palette::Level::Raised, cx), hover)));
    let count = template.channels.iter().filter(|c| !matches!(c, Chan::Category(_))).count();
    let more = long.then(|| {
        let label: SharedString = if expanded { "Show fewer".into() } else { format!("Show all {count} channels").into() };
        ui::button(("create-server-more", i), Tone::Link, cx).text_size(ui::TEXT_XS).font_weight(FontWeight::SEMIBOLD).child(label).on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_template_channels(i, cx);
        }))
    });
    ui::row()
        .id(("create-server-template", i))
        .group(group)
        .items_start()
        .gap_3()
        .px_3()
        .py_2p5()
        .squircle(ui::RADIUS_LG)
        .child(ui::icon_tile(template.glyph, template_fill(template.hue), gpui_kit::white()))
        .child(ui::col().flex_1().pt_0p5().items_start().gap_1().child(div().font_weight(FontWeight::MEDIUM).text_color(strong).child(template.label)).child(chips).children(more))
        .child(div().flex_none().mt_2p5().text_color(faint).child(Icon::new(Glyph::ChevronRight).size(ui::ICON_MD)))
        .when(off, |r| r.opacity(0.5).cursor_not_allowed())
        .when(!off, |r| r.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| this.pick_create_server_template(Some(i), window, cx))))
}

fn channel_chips(template: &Template) -> Vec<(Glyph, &'static str)> {
    template
        .channels
        .iter()
        .filter_map(|c| match c {
            Chan::Category(_) => None,
            Chan::Text(name) => Some((Glyph::Hashtag, *name)),
            Chan::Voice(name) => Some((Glyph::VolumeHigh, *name)),
        })
        .collect()
}

fn channel_chip(glyph: Glyph, name: &'static str, faint: Hsla) -> impl IntoElement {
    ui::row().gap(px(3.)).whitespace_nowrap().child(div().text_color(faint).child(Icon::new(glyph).size(ui::ICON_XS))).child(name)
}

// The clamped list's last line dissolving into its row: the dialog's surface, or the surface under the row's
// hover wash while the row is hovered, so the fade never shows as a box.
fn fade(group: SharedString, surface: Hsla, hover: Hsla) -> impl IntoElement {
    let into = |fill: Hsla| linear_gradient(180., linear_color_stop(fill.opacity(0.), 0.), linear_color_stop(fill, 1.));
    let hovered = ui::mix(surface, hover.opacity(1.), hover.a);
    div().absolute().left_0().right_0().bottom_0().h_5().bg(into(surface)).group_hover(group, move |s| s.bg(into(hovered)))
}

// The dashed squircle that opens the file picker, or the picked image, with the + badge either way.
fn upload_tile(picked: Option<PathBuf>, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (edge, accent, muted) = (palette::line_strong(cx), palette::accent(), palette::muted(cx));
    let radius = 24.;
    let face = match picked {
        Some(path) => img(path).size_full().squircle(radius).into_any_element(),
        None => ui::col()
            .size_full()
            .items_center()
            .justify_center()
            .gap_0p5()
            .squircle(radius)
            .border_2()
            .border_dashed()
            .border_color(edge)
            .text_color(muted)
            .child(Icon::new(Glyph::Upload).size(px(22.)))
            .child(div().text_size(ui::TEXT_XXS).font_weight(FontWeight::BOLD).child("UPLOAD"))
            .into_any_element(),
    };
    let badge = ui::row()
        .absolute()
        .top_neg_1()
        .right_neg_1()
        .size_6()
        .rounded_full()
        .bg(accent)
        .text_color(palette::on_accent())
        .justify_center()
        .child(Icon::new(Glyph::Plus).size(ui::ICON_SM));
    div()
        .id("create-server-upload")
        .relative()
        .size_20()
        .cursor_pointer()
        .child(face)
        .child(badge)
        .tooltip(|window, cx| ui::tip("Upload a server icon", window, cx))
        .on_click(cx.listener(|this, _, _, cx| this.pick_server_icon(cx)))
}

// One example invite; clicking it fills the field.
fn invite_example(i: usize, example: &'static str, invite: &Entity<InputState>, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (strong, accent) = (palette::strong(cx), palette::accent_ink(cx));
    let invite = invite.clone();
    div()
        .id(("create-server-example", i))
        .text_size(ui::TEXT_DETAIL)
        .font_weight(FontWeight::MEDIUM)
        .text_color(strong)
        .cursor_pointer()
        .hover(move |s| s.text_color(accent).underline())
        .child(example)
        .on_click(cx.listener(move |this, _, window, cx| {
            invite.update(cx, |state, cx| {
                state.set_value(example, window, cx);
                state.focus(window, cx);
            });
            if let Some(form) = this.create_server.as_mut() {
                form.error = None;
            }
            cx.notify();
        }))
}

// A captcha Discord asks for opens in the browser, as `join_invite` does.
fn captcha_opener(cx: &mut Context<Waffle>) -> mpsc::UnboundedSender<String> {
    let (captcha_url, mut captcha_urls) = mpsc::unbounded::<String>();
    cx.spawn(async move |_, cx| {
        while let Some(url) = futures::StreamExt::next(&mut captcha_urls).await {
            cx.update(|cx: &mut App| cx.open_url(&url));
        }
    })
    .detach();
    captcha_url
}

// Discord's rule: the trimmed input's segment after the last `/` is the code.
fn invite_code(input: &str) -> Option<&str> {
    let code = input.trim().rsplit('/').next()?;
    (!code.is_empty()).then_some(code)
}

// Discord's wording for the joins it knows; anything else reads as the API words it.
// Not verified: matched on Discord's API messages, since `join_invite` passes only the message text on.
fn join_error(error: &str) -> String {
    let lower = error.to_lowercase();
    let known = if lower.contains("unknown invite") {
        "The invite is invalid or has expired."
    } else if lower.contains("maximum number of guilds") {
        "You must leave a server first to join."
    } else if lower.contains("paused") || lower.contains("invites disabled") {
        "Invites are currently paused for this server. Please try again later."
    } else {
        return error.to_owned();
    };
    known.to_owned()
}

// `POST /guilds`'s body for a template (None is Create My Own): placeholder ids numbered in list order, each
// channel's `parent_id` the category above it, #general the system channel, and School Club's example role.
// Not verified: the exact placeholder ids Discord's client sends; any ids unique within the body are what the API asks for.
fn guild_body(name: &str, icon: Option<&str>, template: Option<&Template>) -> Value {
    let Some(template) = template else {
        return json!({ "name": name, "icon": icon, "channels": [], "system_channel_id": null, "guild_template_code": CREATE_MY_OWN_CODE });
    };
    let mut parent: Option<u64> = None;
    let mut system: Option<u64> = None;
    let channels: Vec<Value> = template
        .channels
        .iter()
        .zip(1u64..)
        .map(|(chan, id)| match chan {
            Chan::Category(name) => {
                parent = Some(id);
                json!({ "id": id, "type": 4, "name": name })
            }
            Chan::Text(name) => {
                if *name == "general" {
                    system = Some(id);
                }
                json!({ "id": id, "type": 0, "name": name, "parent_id": parent })
            }
            Chan::Voice(name) => json!({ "id": id, "type": 2, "name": name, "parent_id": parent }),
        })
        .collect();
    let mut body = json!({ "name": name, "icon": icon, "channels": channels, "system_channel_id": system, "guild_template_code": template.code });
    if template.officers {
        let officers = template.channels.len() as u64 + 1;
        body["roles"] = json!([
            { "id": 0, "name": "@everyone", "permissions": DEFAULT_PERMISSIONS.to_string() },
            { "id": officers, "name": "officers (example)", "permissions": OFFICER_PERMISSIONS.to_string(), "color": 3066993, "hoist": true, "mentionable": true },
        ]);
    }
    body
}

pub(crate) fn demo(waffle: &mut Waffle, scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    if scene == "create-server-limit" {
        let fixture = waffle.guilds.clone();
        for n in 0..100usize.saturating_sub(fixture.len()) {
            let Some(guild) = fixture.get(n % fixture.len().max(1)) else { break };
            waffle.guilds.push(crate::discord::Guild { id: format!("{}-limit-{n}", guild.id), ..guild.clone() });
        }
    }
    waffle.open_create_server(window, cx);
    match scene {
        "create-server-template" => {
            if let Some(form) = waffle.create_server.as_mut() {
                form.expanded.insert(SCHOOL_CLUB);
            }
        }
        "create-server-customize" => {
            waffle.pick_create_server_template(Some(0), window, cx);
            waffle.create_server_step(Step::Customize, window, cx);
        }
        "create-server-join" => {
            waffle.create_server_step(Step::Join, window, cx);
            if let Some(form) = waffle.create_server.as_mut() {
                form.invite.update(cx, |state, cx| state.set_value("https://discord.gg/expired", window, cx));
                form.error = Some(join_error("Unknown Invite"));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_code_is_the_segment_after_the_last_slash() {
        assert_eq!(invite_code("hTKzmak"), Some("hTKzmak"));
        assert_eq!(invite_code("  https://discord.gg/hTKzmak "), Some("hTKzmak"));
        assert_eq!(invite_code("https://discord.gg/wumpus-friends"), Some("wumpus-friends"));
        assert_eq!(invite_code("discord.gg/"), None);
        assert_eq!(invite_code("   "), None);
    }

    #[test]
    fn template_body_links_channels_to_their_category_and_general() {
        let body = guild_body("Club", None, Some(&TEMPLATES[SCHOOL_CLUB]));
        let channels = body["channels"].as_array().unwrap();
        assert_eq!(channels.len(), TEMPLATES[SCHOOL_CLUB].channels.len());
        assert_eq!(channels[0], json!({ "id": 1, "type": 4, "name": "Information" }));
        assert_eq!(channels[1]["parent_id"], 1);
        let general = channels.iter().find(|c| c["name"] == "general").unwrap();
        assert_eq!(body["system_channel_id"], general["id"]);
        assert_eq!(general["parent_id"], 5);
        assert_eq!(channels.last().unwrap()["parent_id"], 9);
        assert_eq!(body["guild_template_code"], "Ctg7PUHcQmZu");
        let roles = body["roles"].as_array().unwrap();
        assert_eq!(roles[1]["name"], "officers (example)");
        let officer = roles[1]["permissions"].as_str().unwrap().parse::<u64>().unwrap();
        assert_ne!(officer & perm::MANAGE_ROLES, 0);
        assert!(guild_body("Gaming", None, Some(&TEMPLATES[0])).get("roles").is_none());
    }

    #[test]
    fn create_my_own_sends_its_code_and_no_channels() {
        let body = guild_body("Mine", Some("data:image/png;base64,aGk="), None);
        assert_eq!(body["channels"], json!([]));
        assert_eq!(body["system_channel_id"], Value::Null);
        assert_eq!(body["guild_template_code"], CREATE_MY_OWN_CODE);
        assert_eq!(body["icon"], "data:image/png;base64,aGk=");
    }
}
