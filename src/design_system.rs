// The design system inside the app: every token and shared control from `palette` and `ui`, live, in
// both themes. Open it with `WAVE_DESIGN=1 cargo run`; `WAVE_DESIGN=buttons,choices` shows only those
// sections. Sections, their order and their contents mirror design/foundations.html and
// design/primitives.html (section ids as there), so a drift between the two shows up side by side.
use std::time::{Duration, Instant};

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    App, Context, Entity, Focusable, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, WindowBounds, WindowOptions,
    base::{Easing, Transition, transition},
    component::{
        ActiveTheme, Icon, Theme, ThemeMode,
        input::{Input, InputState, Textarea, TextareaState},
    },
    div, prelude::*, px, relative, size, svg,
};

use crate::{
    discord::Status,
    icons::{Glyph, MARK},
    mascot::Mood,
    motion,
    palette::{self, Level},
    ui::{self, Check, Notice, Squircle, Tone},
};

pub fn open(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::centered(None, size(px(1180.), px(860.)), cx))),
        titlebar: Some(gpui_kit::TitlebarOptions { title: Some("Waffle Design System".into()), ..Default::default() }),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        palette::sync(window, cx);
        let only = std::env::var("WAVE_DESIGN").ok().filter(|v| v != "1").map(|v| v.split(',').map(str::to_owned).collect());
        let fields = Fields::new(window, cx);
        cx.new(|_| DesignSystem {
            only,
            fields,
            muted: true,
            pinned: false,
            members: true,
            busy: [false; 2],
            device_open: false,
            slowmode: 10,
            role_colour: 0,
            drop_over: false,
            noise: true,
            echo: false,
            checks: [true, false],
            voice_mode: 0,
            quality: 1,
            volume: 0.5,
            chips: [true, false, false],
            tab: 0,
            pill: 0,
            started: Instant::now(),
            meter: (0, 0.),
            played: [false; 6],
            password_shown: false,
        })
    })
    .expect("open design system window");
}

// The editors behind the Fields section, made once with the window.
struct Fields {
    name: Entity<InputState>,
    search: Entity<InputState>,
    channel: Entity<InputState>,
    password: Entity<InputState>,
    invite: Entity<InputState>,
    username: Entity<InputState>,
    about: Entity<TextareaState>,
}

impl Fields {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        let mut input = |build: fn(InputState) -> InputState| cx.new(|cx| build(InputState::new(window, cx)));
        Self {
            name: input(|s| s.default_value("you")),
            search: input(|s| s.placeholder("Search Night Shift")),
            channel: input(|s| s.placeholder("new-channel")),
            password: input(|s| s.masked(true).default_value("hunter22")),
            invite: input(|s| s.default_value("wave.gg/nght")),
            username: input(|s| s.default_value("you")),
            about: cx.new(|cx| TextareaState::new(window, cx).placeholder("Tell people about yourself")),
        }
    }
}

pub struct DesignSystem {
    only: Option<Vec<String>>,
    fields: Fields,
    // Buttons
    muted: bool,
    pinned: bool,
    members: bool,
    busy: [bool; 2],
    // Fields
    password_shown: bool,
    device_open: bool,
    slowmode: u32,
    role_colour: usize,
    drop_over: bool,
    // Choices
    noise: bool,
    echo: bool,
    checks: [bool; 2],
    voice_mode: usize,
    quality: usize,
    volume: f32,
    chips: [bool; 3],
    // Tabs
    tab: usize,
    pill: usize,
    // Feedback: the mic meter steps every 90ms like the page's script.
    started: Instant,
    meter: (u128, f32),
    // Foundations
    played: [bool; 6],
}

impl DesignSystem {
    fn shows(&self, section: &str) -> bool {
        self.only.as_ref().is_none_or(|only| only.iter().any(|s| s == section))
    }

    fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        palette::apply(cx);
        cx.notify();
    }

}

impl Render for DesignSystem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().is_dark();
        let header = self.header(dark, window, cx).into_any_element();
        let mut sections: Vec<gpui_kit::AnyElement> = Vec::new();
        let mut add = |id: &str, el: gpui_kit::AnyElement| sections.push(div().id(SharedString::from(id.to_owned())).child(el).into_any_element());
        if self.shows("colour") {
            add("colour", colour(cx).into_any_element());
        }
        if self.shows("type") {
            add("type", type_scale(cx).into_any_element());
        }
        if self.shows("space") {
            add("space", space(cx).into_any_element());
        }
        if self.shows("motion") {
            add("motion", self.motion(window, cx).into_any_element());
        }
        if self.shows("icons") {
            add("icons", icons(cx).into_any_element());
        }
        if self.shows("buttons") {
            add("buttons", self.buttons(cx).into_any_element());
        }
        if self.shows("fields") {
            add("fields", self.fields_section(cx).into_any_element());
        }
        if self.shows("choices") {
            add("choices", self.choices(window, cx).into_any_element());
        }
        if self.shows("tabs") {
            add("tabs", self.tabs(window, cx).into_any_element());
        }
        if self.shows("badges") {
            add("badges", badges(cx).into_any_element());
        }
        if self.shows("avatars") {
            add("avatars", avatars(cx).into_any_element());
        }
        if self.shows("feedback") {
            add("feedback", self.feedback(window, cx).into_any_element());
        }
        if self.shows("layout") {
            add("layout", layout(cx).into_any_element());
        }
        div()
            .id("design-system")
            .size_full()
            .overflow_y_scrollbar()
            .bg(palette::surface(Level::Page, cx))
            .text_color(palette::text(cx))
            .font_family(cx.theme().font_family.clone())
            .text_size(px(14.))
            .line_height(relative(1.45))
            .child(header)
            .child(div().max_w(px(1032.)).mx_auto().px(px(24.)).pb(px(120.)).flex().flex_col().children(sections))
    }
}

impl DesignSystem {
    fn header(&self, dark: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(24.))
            .py(px(10.))
            .border_b_1()
            .border_color(palette::line(cx))
            .child(svg().path(MARK).size(px(22.)).text_color(palette::logo_ink(cx)))
            .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Waffle"))
            .child(div().text_size(px(16.)).font_weight(FontWeight::NORMAL).text_color(palette::faint(cx)).child("Design system"))
            .child(div().flex_1())
            .child(ui::segmented_live("theme", &["Light", "Dark"], dark as usize, move |i, window, cx| {
                this.update(cx, |this, cx| this.set_theme(i == 1, window, cx)).ok();
            }, window, cx))
    }

    fn buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = |i: usize, this: &Self| this.busy[i];
        let start_busy = |i: usize| {
            cx.listener(move |this: &mut Self, _, _, cx| {
                if this.busy[i] {
                    return;
                }
                this.busy[i] = true;
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(1400)).await;
                    this.update(cx, |this, cx| {
                        this.busy[i] = false;
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            })
        };
        div()
            .child(div().pt(px(32.)).text_size(px(28.)).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Primitives"))
            .child(lede("The small parts every screen is built from. Everything here is live: hover, press, Tab to focus, toggle.", cx).mt(px(4.)).mb(px(16.)))
            .child(
                section("Buttons", "Verb plus object, sentence case. One primary per surface; secondary is the quiet fill, ghost for low-stakes actions in dense rows, danger only for the destructive step itself.", cx)
                    .pt(px(0.))
                    .child(
                        board(cx)
                            .gap(px(24.))
                            .child(
                                group("Tones", cx).child(
                                    row()
                                        .child(ui::button("b-primary", Tone::Primary, cx).child("Join voice"))
                                        .child(ui::button("b-secondary", Tone::Secondary, cx).child("Invite friends"))
                                        .child(ui::button("b-ghost", Tone::Ghost, cx).child("Mark as read"))
                                        .child(ui::button("b-danger", Tone::Danger, cx).child("Leave server"))
                                        .child(ui::button("b-link", Tone::Link, cx).child("View all members")),
                                ),
                            )
                            .child(
                                group("Sizes", cx).child(
                                    row()
                                        .child(ui::button("b-lg", Tone::Primary, cx).large().child(glyph(Glyph::Stream)).child("Go live"))
                                        .child(ui::button("b-md", Tone::Primary, cx).child(glyph(Glyph::Stream)).child("Go live"))
                                        .child(ui::button("b-sm", Tone::Primary, cx).small().child(glyph(Glyph::Stream)).child("Go live"))
                                        .child(note("44 / 36 / 30 tall · radius 10, small 8", cx)),
                                ),
                            )
                            .child(
                                group("States", cx).child(
                                    row()
                                        .child(ui::button("b-off-1", Tone::Primary, cx).disabled(true).child("Join voice"))
                                        .child(ui::button("b-off-2", Tone::Secondary, cx).disabled(true).child("Invite friends"))
                                        .child(ui::button("b-busy-1", Tone::Primary, cx).loading(busy(0, self).then_some("Saving…")).child("Save changes").on_click(start_busy(0)))
                                        .child(ui::button("b-busy-2", Tone::Secondary, cx).loading(busy(1, self).then_some("Sending…")).child(glyph(Glyph::PaperPlaneBold)).child("Send invite").on_click(start_busy(1)))
                                        .child(note("Loading keeps the width and shows the wave", cx)),
                                ),
                            )
                            .child(
                                group("Toggle and split", cx).child(
                                    row()
                                        .child(ui::button("b-muted", Tone::Secondary, cx).toggled(self.muted, cx).child(glyph(if self.muted { Glyph::BellSlash } else { Glyph::Bell })).child(if self.muted { "Muted" } else { "Mute" }).on_click(cx.listener(|this, _, _, cx| {
                                            this.muted = !this.muted;
                                            cx.notify();
                                        })))
                                        .child(ui::button("b-pin", Tone::Secondary, cx).toggled(self.pinned, cx).child(glyph(Glyph::Pin)).child("Pin").on_click(cx.listener(|this, _, _, cx| {
                                            this.pinned = !this.pinned;
                                            cx.notify();
                                        })))
                                        .child(ui::split(
                                            ui::button("b-mic", Tone::Secondary, cx).child(glyph(Glyph::Mic)).child("Microphone"),
                                            ui::button("b-mic-more", Tone::Secondary, cx).child(Icon::new(Glyph::ChevronDown).size(px(14.))),
                                        ))
                                        .child(ui::split(
                                            ui::button("b-live", Tone::Primary, cx).child(glyph(Glyph::Stream)).child("Go live"),
                                            ui::button("b-live-more", Tone::Primary, cx).child(Icon::new(Glyph::ChevronDown).size(px(14.))),
                                        )),
                                ),
                            )
                            .child(
                                group("Icon buttons", cx).child(
                                    row()
                                        .child(ui::icon_button("i-close", Glyph::X, "Close", cx).small())
                                        .child(ui::icon_button("i-pin", Glyph::Pin, "Pin", cx))
                                        .child(ui::icon_button("i-settings", Glyph::Settings, "Settings", cx).large())
                                        .child(ui::icon_button("i-members", Glyph::Users, "Member list", cx).toggled(self.members, cx).on_click(cx.listener(|this, _, _, cx| {
                                            this.members = !this.members;
                                            cx.notify();
                                        })))
                                        .child(ui::icon_button("i-unmute", Glyph::MicOff, "Unmute", cx).off(true, cx))
                                        .child(note("24 / 32 / 40 · pressed is the selected fill · off-states are coral", cx)),
                                ),
                            ),
                    ),
            )
    }

    fn fields_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let f = &self.fields;
        let focus = |state: &Entity<InputState>, cx: &App| state.read(cx).focus_handle(cx);
        let input = |id: &'static str, state: &Entity<InputState>, cx: &App| ui::text_field(id, Input::new(state).appearance(false).px_0().text_size(px(16.)), &focus(state, cx));
        let swatches = [(0.6, 0.2, 5.), (0.62, 0.17, 35.), (0.7, 0.15, 80.), (0.6, 0.14, 155.), (0.58, 0.12, 220.), (0.55, 0.17, 290.)];
        let sidebar = palette::surface(Level::Sidebar, cx);
        section("Fields", "Labels above, help and errors below. Focus is the accent border plus a soft ring. Errors say what happened and how to fix it, and keep what the user typed.", cx).child(
            board(cx).child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap(px(24.))
                    .child(ui::field("Display name", cx).child(input("f-name", &f.name, cx)).child(ui::field_hint("Shown to everyone", Some("3/32".into()), cx)))
                    .child(ui::field("Search", cx).child(input("f-search", &f.search, cx).leading(Icon::new(Glyph::Search).size(px(18.))).trailing(ui::keys(["⌘", "F"], cx))))
                    .child(ui::field("Channel name", cx).child(input("f-channel", &f.channel, cx).leading(div().text_size(px(16.)).child("#"))))
                    .child(ui::field("Password", cx).child(input("f-password", &f.password, cx).trailing(self.show_password(cx))))
                    .child(ui::field("Invite link", cx).child(input("f-invite", &f.invite, cx).error(true)).child(ui::field_error("This invite has expired. Ask for a new one.", cx)))
                    .child(ui::field("Username", cx).child(input("f-username", &f.username, cx).disabled(true)).child(ui::field_hint("You can change this once every 30 days", None, cx)))
                    .child(
                        ui::field("About me", cx)
                            .col_span(2)
                            .child(ui::text_area("f-about", Textarea::new(&f.about).appearance(false).px_0().text_size(px(16.)), &f.about.read(cx).focus_handle(cx)))
                            .child(ui::field_hint("Markdown and emoji work here", Some("0/190".into()), cx)),
                    )
                    .child(ui::field("Input device", cx).child(div().flex_1().flex().items_center().child(ui::select("f-device", Some(Glyph::Mic), "MacBook Pro Microphone (Built-in)", self.device_open, cx).on_click(cx.listener(|this, _, _, cx| {
                        this.device_open = !this.device_open;
                        cx.notify();
                    })))))
                    .child(ui::field("Slowmode", cx).child(ui::stepper(
                        "f-slowmode",
                        format!("{}s", self.slowmode),
                        cx.listener(|this, _, _, cx| {
                            this.slowmode = this.slowmode.saturating_sub(5);
                            cx.notify();
                        }),
                        cx.listener(|this, _, _, cx| {
                            this.slowmode += 5;
                            cx.notify();
                        }),
                        cx,
                    )))
                    .child(ui::field("Two-factor code", cx).child(div().flex().gap(px(8.)).children([Some('4'), Some('8'), Some('1'), None, None, None].into_iter().map(|digit| ui::code_cell(digit, false, cx)))))
                    .child(ui::field("Role colour", cx).child(div().flex().flex_wrap().gap(px(8.)).children(swatches.into_iter().enumerate().map(|(i, (l, c, h))| {
                        ui::swatch(("swatch", i), palette::oklch(l, c, h), i == self.role_colour, sidebar).on_click(cx.listener(move |this, _, _, cx| {
                            this.role_colour = i;
                            cx.notify();
                        }))
                    }))))
                    .child(
                        ui::field("Attachments", cx).col_span(2).child(
                            div()
                                .id("f-drop")
                                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                                    this.drop_over = *over;
                                    cx.notify();
                                }))
                                .child(ui::drop_zone(self.drop_over, Glyph::Upload, "Drop files to upload", cx).child(
                                    div().flex().items_center().gap(px(4.)).child("Up to 25 MB each, or").child(ui::button("f-browse", Tone::Link, cx).child("browse files")),
                                )),
                        ),
                    ),
            ),
        )
    }

    // The password field's eye: 28px, tucked 6px into the field's padding. The slot is narrowed rather than
    // given a -6px margin, which taffy mis-measures (see `ui::stacked`).
    fn show_password(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.password_shown { "Hide password" } else { "Show password" };
        let toggle = ui::icon_button("f-password-eye", Glyph::Eye, label, cx).w(px(28.)).h(px(28.)).on_click(cx.listener(|this, _, window, cx| {
            this.password_shown = !this.password_shown;
            let masked = !this.password_shown;
            this.fields.password.update(cx, |state, cx| state.set_masked(masked, window, cx));
            cx.notify();
        }));
        div().w(px(22.)).h(px(28.)).child(toggle)
    }

    fn choices(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let checks = [ui::check_live("k-0", if self.checks[0] { Check::On } else { Check::Off }, window, cx), ui::check_live("k-1", if self.checks[1] { Check::On } else { Check::Off }, window, cx), ui::check_live("k-2", Check::Mixed, window, cx)];
        let radios = [ui::radio_live("r-0", self.voice_mode == 0, window, cx), ui::radio_live("r-1", self.voice_mode == 1, window, cx)];
        let this = cx.entity().downgrade();
        let quality = ui::segmented_live("quality", &["720p", "1080p", "Source"], self.quality, {
            let this = this.clone();
            move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.quality = i;
                    cx.notify();
                })
                .ok();
            }
        }, window, cx);
        let volume = ui::slider_live("volume", self.volume, move |v, _, cx| {
            this.update(cx, |this, cx| {
                this.volume = v;
                cx.notify();
            })
            .ok();
        }, window, cx);
        let [check_0, check_1, check_2] = checks;
        let [radio_0, radio_1] = radios;
        section("Choices", "Switches apply at once; checkboxes pick many; radios pick one; segmented controls switch a view or a value in place; chips filter.", cx).child(
            board(cx).child(
                div()
                    .grid()
                    .grid_cols(4)
                    .gap(px(16.))
                    .child(
                        stack()
                            .child(ui::toggle_row("c-noise", "Noise suppression", Some("Removes background sound from your mic".into()), self.noise, window, cx).on_click(cx.listener(|this, _, _, cx| {
                                this.noise = !this.noise;
                                cx.notify();
                            })))
                            .child(ui::toggle_row("c-echo", "Echo cancellation", Some("Stops others hearing themselves".into()), self.echo, window, cx).on_click(cx.listener(|this, _, _, cx| {
                                this.echo = !this.echo;
                                cx.notify();
                            }))),
                    )
                    .child(
                        stack()
                            .child(ui::pick_row("c-k0", check_0, "Show game activity", Some("Lets friends see what you play".into()), cx).on_click(cx.listener(|this, _, _, cx| {
                                this.checks[0] = !this.checks[0];
                                cx.notify();
                            })))
                            .child(ui::pick_row("c-k1", check_1, "Allow DMs from server members", None, cx).on_click(cx.listener(|this, _, _, cx| {
                                this.checks[1] = !this.checks[1];
                                cx.notify();
                            })))
                            .child(ui::pick_row("c-k2", check_2, "Notify for every channel", Some("Some channels on, some off".into()), cx)),
                    )
                    .child(
                        stack()
                            .child(ui::pick_row("c-r0", radio_0, "Voice activity", Some("Transmits when you speak".into()), cx).on_click(cx.listener(|this, _, _, cx| {
                                this.voice_mode = 0;
                                cx.notify();
                            })))
                            .child(ui::pick_row("c-r1", radio_1, "Push to talk", Some("Hold a key to transmit".into()), cx).on_click(cx.listener(|this, _, _, cx| {
                                this.voice_mode = 1;
                                cx.notify();
                            }))),
                    )
                    .child(
                        stack()
                            .gap(px(16.))
                            .child(quality)
                            .child(
                                ui::field("Mira's volume", cx).child(
                                    div().flex().items_center().gap(px(8.)).child(div().flex_1().min_w_0().child(volume)).child(
                                        div().min_w(px(44.)).text_right().text_color(palette::strong(cx)).child(format!("{}%", (self.volume * 200.).round())),
                                    ),
                                ),
                            )
                            .child(row().children([("Images", Glyph::Image), ("Links", Glyph::Link), ("Files", Glyph::File)].into_iter().enumerate().map(|(i, (name, icon))| {
                                ui::chip(("chip", i), Some(icon), name, self.chips[i], cx).on_click(cx.listener(move |this, _, _, cx| {
                                    this.chips[i] = !this.chips[i];
                                    cx.notify();
                                }))
                            }))),
                    ),
            ),
        )
    }

    fn tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let items = |names: &[(&'static str, Option<u32>)]| names.iter().map(|(n, c)| ui::Tab::new(*n).count(*c)).collect::<Vec<_>>();
        let tabs = ui::tabs("tabs", &items(&[("Overview", None), ("Roles", None), ("Emoji", None), ("Members", Some(4)), ("Audit log", None)]), self.tab, {
            let this = this.clone();
            move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.tab = i;
                    cx.notify();
                })
                .ok();
            }
        }, window, cx);
        let pills = ui::pill_tabs("pills", &items(&[("Online", None), ("All", None), ("Pending", Some(2)), ("Blocked", None)]), self.pill, move |i, _, cx| {
            this.update(cx, |this, cx| {
                this.pill = i;
                cx.notify();
            })
            .ok();
        }, window, cx);
        section("Tabs", "Underline tabs split a page into views; pill tabs filter a list inside one view.", cx).child(board(cx).gap(px(24.)).child(tabs).child(pills))
    }

    fn feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let level = self.meter_level();
        window.request_animation_frame();
        let skeleton_row = |id: &'static str, widths: (f32, f32), block: bool, cx: &App| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(ui::skeleton_circle((id, 0usize), 40., cx))
                .child(
                    stack()
                        .flex_1()
                        .child(ui::skeleton((id, 1usize), ui::RADIUS_XS, cx).h(px(12.)).w(px(widths.0)))
                        .child(ui::skeleton((id, 2usize), ui::RADIUS_XS, cx).h(px(12.)).w(relative(widths.1)))
                        .when(block, |d| d.child(ui::skeleton((id, 3usize), ui::RADIUS_LG, cx).w(px(240.)).h(px(140.)))),
                )
        };
        let strong = |text: &'static str, cx: &App| div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text);
        section("Feedback", "Progress shows a count when the work is countable. Waits look alive: the wave, or a calm shimmer that sweeps then rests. Callouts sit inside the view they're about.", cx).child(
            board(cx)
                .gap(px(24.))
                .child(
                    div()
                        .grid()
                        .grid_cols(4)
                        .gap(px(16.))
                        .child(stack().child(div().flex().justify_between().child(strong("clip.mp4", cx)).child(div().text_color(palette::faint(cx)).child("12.4 / 31 MB"))).child(ui::progress_live("p-clip", 0.4, window, cx)))
                        .child(stack().child(strong("Connecting to Lounge…", cx)).child(ui::progress_indeterminate("p-connect", cx)))
                        .child(stack().child(strong("Mic level", cx)).child(ui::meter(level, 24, cx)))
                        .child(row().child(ui::ring(0.72, cx)).child(div().text_color(palette::muted(cx)).child("18 of 25 uploaded"))),
                )
                .child(stack().gap(px(12.)).child(skeleton_row("sk-a", (120., 0.72), false, cx)).child(skeleton_row("sk-b", (90., 0.48), true, cx)))
                .child(
                    stack()
                        .child(ui::callout(Notice::Info, Glyph::InfoFill, "Slowmode is on", "You can send one message every 10 seconds.", cx))
                        .child(ui::callout(Notice::Warning, Glyph::Warning, "Your connection is unstable", "Others may hear you cut out. Try a wired connection.", cx).child(ui::button("cl-test", Tone::Secondary, cx).small().self_center().child("Run test")))
                        .child(ui::callout(Notice::Danger, Glyph::XCircle, "Couldn't send 2 messages", "Waffle lost the connection. They're kept below.", cx).child(ui::button("cl-retry", Tone::Secondary, cx).small().self_center().child("Retry all")))
                        .child(ui::callout(Notice::Success, Glyph::CheckCircle, "Two-factor is on", "Keep your backup codes somewhere safe.", cx))
                        .child(ui::callout(Notice::Accent, Glyph::AtFill, "3 mentions while you were away", "The oldest is in #looking-for-group.", cx).child(ui::button("cl-jump", Tone::Primary, cx).small().self_center().child("Jump to first"))),
                )
                .child(
                    div()
                        .grid()
                        .grid_cols(3)
                        .gap(px(16.))
                        .child(div().squircle(ui::RADIUS_LG).bg(palette::surface(Level::Page, cx)).child(ui::empty_state("empty-idle", Mood::Idle, "No one's around", "None of your friends are online. Invite someone to play.", cx).child(ui::button("e-add", Tone::Primary, cx).mt(px(8.)).child("Add friend"))))
                        .child(div().squircle(ui::RADIUS_LG).bg(palette::surface(Level::Page, cx)).child(ui::empty_state("empty-wait", Mood::Waiting, "Loading messages…", "Fetching the last 50 messages in #general.", cx))),
                )
                .child(div().flex().flex_wrap().gap(px(16.)).children(crate::illustration::Illustration::ALL.map(|art| div().squircle(ui::RADIUS_LG).bg(palette::surface(Level::Page, cx)).child(gpui_kit::img(art.path()).w(px(200.)).h(px(150.))))))
                .child(div().flex().flex_col().items_center().gap(px(8.)).child(ui::toast("toast", Glyph::Checkmark, palette::positive_ink(cx), "Invite link copied", None, cx))),
        )
    }

    // The page's meter script: every 90ms the level decays and takes a random kick.
    fn meter_level(&mut self) -> f32 {
        let step = self.started.elapsed().as_millis() / 90;
        if step != self.meter.0 {
            let noise = ((step.wrapping_mul(2654435761) % 1000) as f32) / 1000.;
            self.meter = (step, (self.meter.1 * 0.82 + noise * 0.35).clamp(0., 1.));
        }
        self.meter.1
    }

    fn motion(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: [(&str, &str, u64, Easing); 6] = [
            ("Press", "120ms · ease-out · scale 0.97", 120, motion::ease_out()),
            ("Hover", "150ms · ease-out · colour only", 150, motion::ease_out()),
            ("Pop", "200ms · ease-out · from 0.95", 200, motion::ease_out()),
            ("Spring", "250ms · touch-driven, ~0.3 bounce", 250, motion::ease_spring()),
            ("Panel", "260ms · drawer curve", 260, Easing::cubic_bezier(0.32, 0.72, 0., 1.).expect("drawer curve")),
            ("Move", "on-screen movement", 400, motion::ease_move()),
        ];
        let cards: Vec<_> = cards.into_iter().enumerate().map(|(i, (name, detail, ms, curve))| {
            let at = transition(gpui_kit::ElementId::from(("motion-dot", i)), if self.played[i] { 1. } else { 0. }, Transition::new(Duration::from_millis(ms)).easing(curve), window, cx);
            board(cx)
                .id(("motion", i))
                .p(px(16.))
                .gap(px(12.))
                .cursor_pointer()
                .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(name))
                .child(note(detail, cx))
                .child(
                    div().relative().h(px(40.)).squircle(ui::RADIUS_MD).bg(palette::surface(Level::Page, cx)).child(
                        div().absolute().top(px(8.)).left(px(8.)).right(px(32.)).h(px(24.)).child(div().absolute().top_0().left(relative(at)).size(px(24.)).squircle(8.).bg(palette::accent())),
                    ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.played[i] = !this.played[i];
                    cx.notify();
                }))
        }).collect();
        section("Motion", "Quick and a little springy. Things you touch overshoot slightly and settle inside 250ms; nothing that happens a hundred times a day animates more than a fade. Click a card to play it.", cx)
            .child(div().grid().grid_cols(4).gap(px(16.)).children(cards))
    }
}

fn section(title: &'static str, lede_text: &'static str, cx: &App) -> gpui_kit::Div {
    div()
        .py(px(32.))
        .border_b_1()
        .border_color(palette::line(cx))
        .flex()
        .flex_col()
        .child(div().text_size(px(20.)).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).mb(px(4.)).child(title))
        .child(lede(lede_text, cx).mb(px(16.)))
}

fn lede(text: &'static str, cx: &App) -> gpui_kit::Div {
    div().max_w(px(520.)).text_color(palette::muted(cx)).child(text)
}

// The design page's board: the sidebar fill, a hairline, the card radius, 24px in.
fn board(cx: &App) -> gpui_kit::Div {
    div().p(px(24.)).squircle(ui::RADIUS_LG).bg(palette::surface(Level::Sidebar, cx)).border_1().border_color(palette::line(cx)).flex().flex_col().gap(px(16.))
}

// A titled group inside a board.
fn group(title: &'static str, cx: &App) -> gpui_kit::Div {
    stack().child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title))
}

fn row() -> gpui_kit::Div {
    div().flex().items_center().gap(px(8.)).flex_wrap()
}

fn stack() -> gpui_kit::Div {
    div().flex().flex_col().gap(px(8.)).min_w_0()
}

fn note(text: &'static str, cx: &App) -> gpui_kit::Div {
    div().text_size(px(12.)).text_color(palette::faint(cx)).child(text)
}

fn glyph(glyph: Glyph) -> impl IntoElement {
    Icon::new(glyph).size(px(18.))
}

fn colour(cx: &App) -> impl IntoElement {
    let page = palette::surface(Level::Page, cx);
    let surface = |name: &'static str, token: &'static str, level: Level| {
        div().flex_1().h(px(92.)).p(px(10.)).bg(palette::surface(level, cx)).flex().flex_col().justify_end().text_size(px(12.)).text_color(palette::muted(cx)).child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(name)).child(token)
    };
    let ink = |name: &'static str, color: Hsla, use_: &'static str| {
        div()
            .flex()
            .justify_between()
            .items_baseline()
            .gap(px(16.))
            .px(px(12.))
            .py(px(8.))
            .squircle(ui::RADIUS_SM)
            .bg(page)
            .child(div().flex().gap(px(4.)).child(div().font_weight(FontWeight::MEDIUM).text_color(color).child(name)).child(div().text_color(palette::faint(cx)).child(format!("· {use_}"))))
            .child(pass(color, page, cx))
    };
    let swatch = |token: &'static str, name: &'static str, color: Hsla, white_text: bool| {
        div()
            .squircle(ui::RADIUS_MD)
            .border_1()
            .border_color(palette::line(cx))
            .bg(palette::surface(Level::Raised, cx))
            .child(edge_top(div().h(px(64.)).p(px(8.)).bg(color).flex().items_end().justify_end().when(white_text, |d| d.child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(gpui_kit::white()).child(ratio(gpui_kit::white(), color))))))
            .child(div().px(px(10.)).py(px(8.)).text_size(px(12.)).child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(token)).child(div().text_color(palette::faint(cx)).child(name)))
    };
    section("Colour", "Restrained: Graphite neutrals plus one honey. Depth in the dark theme comes from lighter surfaces, never shadows. Contrast figures are measured live against the page.", cx)
        .child(h3("Surfaces, deepest to highest", cx))
        .child(
            div()
                .flex()
                .squircle(ui::RADIUS_LG)
                .border_1()
                .border_color(palette::line(cx))
                .child(surface("Rail", "--bg-rail", Level::Rail).rounded_l(px(ui::RADIUS_LG - 1.)).corner_shape(ui::SQUIRCLE))
                .child(surface("Sidebar", "--bg-sidebar", Level::Sidebar))
                .child(surface("Page", "--bg-page", Level::Page))
                .child(surface("Raised", "--bg-raised", Level::Raised))
                .child(surface("Overlay", "--bg-overlay", Level::Overlay).rounded_r(px(ui::RADIUS_LG - 1.)).corner_shape(ui::SQUIRCLE)),
        )
        .child(h3("Text", cx))
        .child(
            board(cx)
                .gap(px(6.))
                .p(px(12.))
                .child(ink("Strong", palette::strong(cx), "Names, titles, selected rows"))
                .child(ink("Default", palette::text(cx), "Message body"))
                .child(ink("Muted", palette::muted(cx), "Channel rows, descriptions"))
                .child(ink("Faint", palette::faint(cx), "Timestamps, placeholders"))
                .child(ink("Accent ink", palette::accent_ink(cx), "Links, mentions"))
                .child(ink("Danger ink", palette::danger_ink(cx), "Errors, destructive rows")),
        )
        .child(h3("Accent and status", cx))
        .child(
            div()
                .grid()
                .grid_cols(4)
                .gap(px(16.))
                .child(swatch("--accent", "Accent · Primary buttons, counts, live", palette::accent(), true))
                .child(swatch("--accent-hover", "Accent hover", palette::accent_hover(), true))
                .child(swatch("--logo", "Logo · The mark only", palette::logo(), true))
                .child(swatch("--danger", "Danger · Leave, delete", palette::danger(), true))
                .child(swatch("--online", "Online · Dot, voice connected", palette::status_in(Status::Online, cx), false))
                .child(swatch("--idle", "Idle · Moon", palette::status_in(Status::Idle, cx), false))
                .child(swatch("--dnd", "Do not disturb · Bar", palette::status_in(Status::Dnd, cx), false))
                .child(swatch("--offline", "Offline · Ring", palette::status_in(Status::Offline, cx), false)),
        )
}

fn edge_top(el: gpui_kit::Div) -> gpui_kit::Div {
    ui::edge(el, ui::top(ui::RADIUS_MD - 1.))
}

fn h3(text: &'static str, cx: &App) -> gpui_kit::Div {
    div().mt(px(24.)).mb(px(12.)).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(text)
}

// "4.62:1 AA", the way the page labels each ink against what it sits on.
fn pass(fg: Hsla, bg: Hsla, cx: &App) -> impl IntoElement {
    div().font_family(cx.theme().mono_font_family.clone()).text_size(px(12.)).text_color(palette::faint(cx)).child(ratio(fg, bg))
}

fn ratio(fg: Hsla, bg: Hsla) -> String {
    let r = palette::contrast(fg, bg);
    let grade = if r >= 4.5 { "AA" } else if r >= 3. { "AA large" } else { "fails" };
    format!("{r:.2}:1 {grade}")
}

fn type_scale(cx: &App) -> impl IntoElement {
    let spec = |meta: &'static str, first: bool, text: gpui_kit::Div| {
        div()
            .flex()
            .items_baseline()
            .gap(px(16.))
            .py(px(12.))
            .when(!first, |d| d.border_t_1().border_color(palette::line(cx)))
            .child(div().w(px(140.)).flex_none().font_family(cx.theme().mono_font_family.clone()).child(note(meta, cx)))
            .child(text)
    };
    let (strong, text, faint) = (palette::strong(cx), palette::text(cx), palette::faint(cx));
    section("Type", "One family for the interface, a mono for code. Gabarito is the proposal: geometric and friendly like the bars, and nobody else's.", cx).child(
        board(cx)
            .gap(px(0.))
            .child(spec("28 · bold · 1.2", true, div().text_size(px(28.)).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("Welcome to Night Shift")))
            .child(spec("20 · bold · 1.2", false, div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("Voice and video")))
            .child(spec("16 · regular · 1.45", false, div().text_size(px(16.)).line_height(relative(1.45)).text_color(text).child("anyone up for a few rounds after dinner? bringing snacks this time, promise")))
            .child(spec("14 · medium", false, div().font_weight(FontWeight::NORMAL).text_color(text).child("general · looking-for-group · Lounge")))
            .child(spec("12 · semibold", false, div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(faint).child("Today at 9:41 PM · Online · 3")))
            .child(spec("mono 14", false, div().font_family(cx.theme().mono_font_family.clone()).text_color(text).child("cargo run --release"))),
    )
}

fn space(cx: &App) -> impl IntoElement {
    let bar = |n: f32| {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .text_size(px(12.))
            .text_color(palette::faint(cx))
            .font_family(cx.theme().mono_font_family.clone())
            .child(div().w(px(70.)).child(format!("{n}px")))
            .child(div().w(px(n * 3.)).h(px(12.)).squircle(3.).bg(palette::accent().opacity(0.8)))
    };
    let tile = |name: &'static str, radius: f32, use_: &'static str| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .child(div().size(px(76.)).squircle(radius).bg(palette::surface(Level::Raised, cx)).border(px(1.5)).border_color(palette::line_strong(cx)))
            .child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).font_family(cx.theme().mono_font_family.clone()).child(name))
            .child(div().text_color(palette::faint(cx)).child(use_))
    };
    section("Space and shape", "A 4px scale. Controls 10, surfaces 14, dialogs 20; nested corners stay concentric.", cx).child(
        div()
            .grid()
            .grid_cols(3)
            .gap(px(16.))
            .child(board(cx).gap(px(6.)).children([4., 8., 12., 16., 24., 32., 48.].map(bar)))
            .child(
                board(cx).child(
                    row()
                        .gap(px(16.))
                        .items_start()
                        .child(tile("xs · 6", 6., "Tags, tooltips"))
                        .child(tile("sm · 8", 8., "Rows, menu items"))
                        .child(tile("md · 10", 10., "Buttons, inputs"))
                        .child(tile("lg · 14", 14., "Cards, tiles, servers"))
                        .child(tile("xl · 20", 20., "Dialogs")),
                ),
            ),
    )
}

fn icons(cx: &App) -> impl IntoElement {
    // The page's set, in its order, where the app has the glyph.
    let set = [
        Glyph::Hashtag,
        Glyph::VolumeHigh,
        Glyph::Mic,
        Glyph::MicOff,
        Glyph::Headphones,
        Glyph::ShareScreen,
        Glyph::Video,
        Glyph::Stream,
        Glyph::CallEnd,
        Glyph::Users,
        Glyph::Thread,
        Glyph::Pin,
        Glyph::Search,
        Glyph::Settings,
        Glyph::Emoji,
        Glyph::Gif,
        Glyph::EmojiAdd,
        Glyph::Plus,
        Glyph::Reply,
        Glyph::Trash,
        Glyph::Bell,
        Glyph::Game,
        Glyph::Crown,
    ];
    section("Icons", "Reicon's filled weight, like Discord; line-only glyphs (#, +, arrows) stay lines. The full set is design/icons.html.", cx)
        .child(board(cx).child(row().gap(px(16.)).text_color(palette::text(cx)).children(set.map(|g| Icon::new(g).size(px(22.))))))
}

fn badges(cx: &App) -> impl IntoElement {
    let muted = |text: &'static str| div().text_color(palette::muted(cx)).child(text);
    section("Badges and keys", "A count asks for you, so it takes the accent. Tags label what something is; roles carry their colour as a dot, never as text colour.", cx).child(
        board(cx)
            .child(
                row()
                    .gap(px(16.))
                    .child(ui::count_badge(3))
                    .child(ui::count_badge(42))
                    .child(ui::count_badge(120))
                    .child(ui::unread_dot())
                    .child(ui::tag("App", cx))
                    .child(ui::tag_with(Glyph::Crown, "Host", cx))
                    .child(ui::tag("New", cx))
                    .child(ui::live_badge()),
            )
            .child(
                row()
                    .child(ui::role_chip("Moderators", palette::oklch(0.65, 0.2, 5.), cx))
                    .child(ui::role_chip("Raid team", palette::oklch(0.7, 0.14, 155.), cx))
                    .child(ui::role_chip("Night owls", palette::oklch(0.75, 0.14, 80.), cx))
                    .child(ui::role_chip("Members", palette::status_in(Status::Offline, cx), cx)),
            )
            .child(
                row()
                    .child(ui::keys(["⌘", "K"], cx))
                    .child(muted("Quick switcher"))
                    .child(ui::keys(["=", "Backspace"], cx).ml(px(16.)))
                    .child(muted("Mute"))
                    .child(ui::keys(["Esc"], cx).ml(px(16.)))
                    .child(muted("Close")),
            ),
    )
}

fn avatars(cx: &App) -> impl IntoElement {
    let ring = palette::surface(Level::Sidebar, cx);
    let page = palette::surface(Level::Page, cx);
    let sized = |size: f32| {
        let mark = ui::monogram("Mira", size);
        if size < 32. {
            return div().child(mark);
        }
        ui::with_status(mark, Status::Online, size, page, cx)
    };
    let person = |name: &'static str, label: &'static str, status: Option<Status>| {
        let avatar = match status {
            Some(status) => ui::with_status(ui::monogram(name, 40.), status, 40., ring, cx),
            None => ui::monogram(name, 40.).shadow(ui::speaking_ring(ring)),
        };
        row().child(avatar).child(div().text_color(palette::muted(cx)).child(label))
    };
    let more = div().size(px(24.)).rounded_full().bg(palette::surface(Level::Overlay, cx)).text_color(palette::strong(cx)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).flex().items_center().justify_center().child("+3");
    section("Avatars and status", "Status is never colour alone: online is a dot, idle a moon, do not disturb a bar, offline a ring. Speaking is the accent ring plus the bars.", cx).child(
        board(cx)
            .gap(px(24.))
            .child(row().gap(px(16.)).items_end().children([16., 20., 24., 32., 40., 56., 80.].map(sized)).child(note("16 20 24 32 40 56 80 · below 24 no initials, no status", cx)))
            .child(
                row()
                    .gap(px(24.))
                    .child(person("Mira", "Online", Some(Status::Online)))
                    .child(person("Ana", "Idle", Some(Status::Idle)))
                    .child(person("Theo", "Do not disturb", Some(Status::Dnd)))
                    .child(person("Jun", "Offline", Some(Status::Offline)))
                    .child(person("Rosa", "Speaking", None)),
            )
            .child(
                row()
                    .gap(px(12.))
                    .child(ui::stacked([ui::monogram("Mira", 24.).into_any_element(), ui::monogram("Theo", 24.).into_any_element(), ui::monogram("Ana", 24.).into_any_element(), more.into_any_element()], 24., ring))
                    .child(div().text_color(palette::muted(cx)).child("6 watching")),
            ),
    )
}

fn layout(cx: &App) -> impl IntoElement {
    section("Dividers and tiles", "Dividers separate groups; a label on a divider names what follows. Icon tiles lead rows that need more presence than a bare icon.", cx).child(
        board(cx)
            .child(ui::divider(Some("Today"), cx))
            .child(ui::divider(None, cx))
            .child(
                row()
                    .gap(px(16.))
                    .child(ui::icon_tile(Glyph::Hashtag, palette::selected(cx), palette::strong(cx)))
                    .child(ui::icon_tile(Glyph::VolumeHigh, palette::selected(cx), palette::strong(cx)))
                    .child(ui::icon_tile(Glyph::Stream, palette::accent(), palette::on_accent()))
                    .child(ui::icon_tile(Glyph::Trash, palette::danger_tint().opacity(1.25), palette::danger_ink(cx))),
            )
            .child(
                ui::surface(cx)
                    .w(px(248.))
                    .child(ui::menu_row("m-reply", cx).child(div().text_color(palette::muted(cx)).child(glyph(Glyph::Reply))).child(div().flex_1().child("Reply")))
                    .child(ui::menu_row("m-react", cx).child(div().text_color(palette::muted(cx)).child(glyph(Glyph::EmojiAdd))).child(div().flex_1().child("Add reaction")))
                    .child(ui::menu_row("m-copy", cx).child(div().text_color(palette::muted(cx)).child(glyph(Glyph::Copy))).child(div().flex_1().child("Copy text")))
                    .child(ui::menu_row("m-mark", cx).child(div().flex_1().child("Mark as unread")).child(ui::check(Check::On, cx).size(px(18.))))
                    .child(ui::menu_row("m-mode", cx).child(div().flex_1().child("Push to talk")).child(ui::radio_with(false, 18., palette::line_strong(cx))))
                    .child(div().my(px(4.)).mx(px(8.)).h(px(1.)).bg(palette::line(cx)))
                    .child(ui::menu_row("m-delete", cx).danger(cx).child(glyph(Glyph::Trash)).child("Delete message")),
            ),
    )
}
