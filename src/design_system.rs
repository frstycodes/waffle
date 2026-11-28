// The design system inside the app: every token and shared control from `palette` and `ui`, live, in both
// themes, laid out like a Figma file. A page tree on the left (foundations, then one page per component); each
// page holds frames: variants, states, edge cases (worst-case data in boxes too small for it) and flows (the
// component in use, start to finish). Standalone with `WAVE_DESIGN=1 cargo run` (`WAVE_DESIGN=buttons` opens
// that page), or from the user menu in a debug build. Section ids mirror design/foundations.html and
// design/primitives.html, so a drift between the two shows up side by side.
mod actions;
mod colour;
mod dialogs;
mod display;
mod edges;
mod flows;
mod foundations;
mod guidance;
mod icons;
mod identity;
mod inputs;
mod overlays;
mod pages;
mod selection;
mod timings;
use colour::colour;
use foundations::{space, type_scale};
use icons::{icons, illustrations};
use pages::{Frame, Group, Kind, Page};
use std::time::{Duration, Instant};

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, Focusable, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement, Pixels, Render, StatefulInteractiveElement, Styled, Window, WindowBounds, WindowOptions,
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
    ui::{self, Check, Notice, Squircle},
};

// The design page's window, and the widest its content grows before it centres, and the room under it.
const WINDOW_SIZE: (f32, f32) = (1180., 860.);
const PAGE_WIDTH: f32 = 1032.;
const PAGE_FOOT: f32 = 120.;
// The logo in the header, and the glyphs on the icon board.
const MARK_SIZE: f32 = 22.;
// The password field's toggle slot.
const TOGGLE_SLOT: f32 = 22.;
// The skeleton's block placeholder.
const SKELETON_BLOCK: (f32, f32) = (240., 140.);
// An illustration's size on the empty-state board.
const ART_SIZE: (f32, f32) = (200., 150.);
// A lede's widest line, a surface swatch's height and the demo menus' width.
const LEDE_WIDTH: f32 = 520.;
const SWATCH_HEIGHT: f32 = 92.;
const MENU_WIDTH: f32 = 248.;
// The page tree's width.
const TREE_WIDTH: f32 = 232.;
// The property name column in a page head.
const PROP_NAME: f32 = 90.;

// What the header's back button does.
type Back = Box<dyn Fn(&mut Window, &mut App)>;

pub fn open(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx))),
        titlebar: Some(gpui_kit::TitlebarOptions { title: Some("Waffle Design System".into()), ..Default::default() }),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        palette::sync(window, cx);
        // `WAVE_DESIGN=buttons:edge` opens the Buttons page on its edge cases.
        let var = std::env::var("WAVE_DESIGN").unwrap_or_default();
        let (key, kind) = var.split_once(':').unwrap_or((&var, ""));
        let page = Page::from_key(key).unwrap_or(Page::Colour);
        let design = build(page, None, window, cx);
        design.update(cx, |design, _| design.view = Kind::ALL.into_iter().find(|k| !kind.is_empty() && k.label().to_lowercase().starts_with(kind)));
        design
    })
    .expect("open design system window");
}

// The page as a view for the app to show in place of itself; `on_back` adds the way back to the header.
pub fn view(on_back: impl Fn(&mut Window, &mut App) + 'static, window: &mut Window, cx: &mut App) -> Entity<DesignSystem> {
    build(Page::Colour, Some(Box::new(on_back)), window, cx)
}

fn build(page: Page, on_back: Option<Back>, window: &mut Window, cx: &mut App) -> Entity<DesignSystem> {
    let fields = Fields::new(window, cx);
    cx.new(|_| DesignSystem {
        page,
        view: None,
        flow: Default::default(),
        on_back,
        fields,
        muted: true,
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
}


// The editors behind the Fields section, made once with the window.
struct Fields {
    name: Entity<InputState>,
    search: Entity<InputState>,
    channel: Entity<InputState>,
    password: Entity<InputState>,
    invite: Entity<InputState>,
    username: Entity<InputState>,
    long: Entity<InputState>,
    mixed: Entity<InputState>,
    join: Entity<InputState>,
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
            long: input(|s| s.default_value("An unreasonably long display name that never fits in a sidebar row")),
            mixed: input(|s| s.default_value("🌸 田中 太郎 مرحبا")),
            join: input(|s| s.placeholder("wave.gg/…").default_value("wave.gg/nght-old")),
            about: cx.new(|cx| TextareaState::new(window, cx).placeholder("Tell people about yourself")),
        }
    }
}

pub struct DesignSystem {
    page: Page,
    view: Option<Kind>,
    flow: flows::Flows,
    on_back: Option<Back>,
    fields: Fields,
    // Buttons
    muted: bool,
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
    fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        palette::apply(cx);
        cx.notify();
    }

    fn open_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }
        self.page = page;
        cx.notify();
    }

    // Every frame of the current page, then of the pages it also shows.
    fn frames(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let page = self.page;
        let mut frames = self.page_frames(window, cx);
        for member in page.members() {
            self.page = *member;
            frames.extend(self.page_frames(window, cx));
        }
        self.page = page;
        frames
    }

    fn page_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let variants = |el: AnyElement| (Kind::Variants, el);
        let mut frames = match self.page {
            Page::Colour => vec![variants(colour(cx).into_any_element())],
            Page::Type => vec![variants(type_scale(cx).into_any_element())],
            Page::Space => vec![variants(space(cx).into_any_element())],
            Page::Motion => vec![variants(self.motion(window, cx).into_any_element())],
            Page::Icons => vec![variants(icons(cx).into_any_element())],
            Page::Illustrations => vec![variants(illustrations(cx).into_any_element())],
            Page::Button => self.button_frames(cx).into_iter().chain(self.icon_button_frames(cx)).collect(),
            Page::ButtonGroup => self.button_group_frames(cx),
            Page::Toggle => self.toggle_frames(cx),
            Page::Card => self.card_frames(cx),
            Page::Input => self.text_field_frames(cx),
            Page::Textarea => self.text_area_frames(cx),
            Page::Select => self.select_frames(cx),
            Page::InputGroup => self.stepper_frames(cx),
            Page::InputOTP => self.code_cell_frames(cx),
            Page::Swatch => self.swatch_frames(cx),
            Page::DropZone => self.drop_zone_frames(cx),
            Page::Switch => self.switch_frames(window, cx),
            Page::Checkbox => self.checkbox_frames(window, cx),
            Page::RadioGroup => self.radio_frames(window, cx),
            Page::Segmented => self.segmented_frames(window, cx),
            Page::Slider => self.slider_frames(window, cx),
            Page::Tabs => self.tabs_frames(window, cx),
            Page::Badge => display::badge_frames(cx),
            Page::CountBadge => display::count_badge_frames(cx),
            Page::RoleChip => display::role_toggle_frames(cx),
            Page::Kbd => display::keycap_frames(cx),
            Page::Separator => display::divider_frames(cx),
            Page::Progress => self.progress_frames(window, cx),
            Page::Meter => self.meter_frames(window, cx),
            Page::Skeleton => self.skeleton_frames(cx),
            Page::IconTile => display::icon_tile_frames(cx),
            Page::Avatar => identity::avatar_frames(cx),
            Page::Presence => identity::presence_frames(window, cx),
            Page::UserAvatar => identity::user_avatar_frames(cx),
            Page::ServerTile => identity::server_tile_frames(cx),
            Page::AppTile => identity::app_tile_frames(cx),
            Page::AvatarStack => identity::avatar_stack_frames(cx),
            Page::Item => identity::list_row_frames(cx),
            Page::Tooltip => overlays::tooltip_frames(cx),
            Page::DropdownMenu => overlays::menu_frames(cx),
            Page::Popover => overlays::popover_frames(cx),
            Page::HoverCard => overlays::hover_card_frames(window, cx),
            Page::Dialog => vec![variants(self.dialogs(window, cx).into_any_element())],
            Page::Sonner => overlays::toast_frames(cx),
            Page::Alert => overlays::callout_frames(cx),
            Page::Strip => overlays::strip_frames(cx),
            Page::Empty => overlays::empty_state_frames(cx),
            Page::DoDont => guidance::do_dont_frames(cx),
            Page::Recipes => self.recipe_frames(cx),
        };
        frames.extend(self.edge_frames(window, cx));
        frames.extend(self.flow_frames(window, cx));
        frames
    }
}

impl Render for DesignSystem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("DesignSystem");
        let dark = cx.theme().is_dark();
        let header = self.header(dark, window, cx).into_any_element();
        let mut frames = self.frames(window, cx);
        // lint-allow(render-work): sorts the current page's handful of frames (a dozen at most)
        frames.sort_by_key(|(kind, _)| *kind);
        let counts = Kind::ALL.map(|kind| frames.iter().filter(|(k, _)| *k == kind).count());
        let shown = self.view;
        let mut body: Vec<AnyElement> = Vec::new();
        let mut last = None;
        for (kind, el) in frames.into_iter().filter(|(kind, _)| shown.is_none_or(|v| v == *kind)) {
            if last != Some(kind) {
                body.push(div().pt_8().child(ui::Separator::new().label(kind.label())).into_any_element());
                last = Some(kind);
            }
            body.push(el);
        }
        let empty = body.is_empty();
        let canvas = div()
            .id(("canvas", self.page as usize))
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scrollbar()
            .child(ui::col().w_full().items_center().child(
                ui::col()
                    .w_full()
                    .max_w(px(PAGE_WIDTH))
                    .px_6()
                    .pb(px(PAGE_FOOT))
                    .child(self.page_head(counts, window, cx))
                    .children(body)
                    .when(empty, |d| d.child(div().py_8().child(note("Nothing of this kind on this page yet.", cx)))),
            ));
        ui::col()
            .size_full()
            .bg(palette::surface(Level::Page, cx))
            .text_color(palette::text(cx))
            .font_family(cx.theme().font_family.clone())
            .text_size(ui::TEXT_SM)
            .line_height(relative(1.45))
            .child(header)
            .child(ui::row().items_stretch().flex_1().min_h_0().child(self.page_tree(cx)).child(canvas))
    }
}

impl DesignSystem {
    fn header(&self, dark: bool, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        ui::row()
            .gap_2p5()
            .px_6()
            .py_2p5()
            .border_b_1()
            .border_color(palette::line(cx))
            .when(self.on_back.is_some(), |d| {
                d.child(ui::Button::new("ds-back").variant(ui::ButtonVariant::Secondary).size(ui::ButtonSize::Sm).child(glyph(Glyph::ChevronLeft)).child("Back to Waffle").on_click(cx.listener(|this, _, window, cx| {
                    if let Some(back) = &this.on_back {
                        back(window, cx);
                    }
                })))
            })
            .child(svg().path(MARK).size(px(MARK_SIZE)).text_color(palette::logo_ink(cx)))
            .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child("Waffle"))
            .child(div().text_size(ui::TEXT_MD).font_weight(FontWeight::NORMAL).text_color(palette::faint(cx)).child("Design system"))
            .child(div().flex_1())
            .child(ui::Tabs::new("theme").labels(&["Light", "Dark"]).value(dark as usize).on_value_change(move |i, window, cx| {
                this.update(cx, |this, cx| this.set_theme(i == 1, window, cx)).ok();
            }))
    }

    // The page tree: the groups in order, each with its components.
    fn page_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let item = |page: Page, cx: &mut Context<Self>| {
            ui::settings_nav_item(("ds-page", page as usize), page.glyph(), page.title(), page == self.page, cx).on_click(cx.listener(move |this, _, _, cx| this.open_page(page, cx)))
        };
        let group = |group: Group, cx: &mut Context<Self>| {
            let pages = Page::ALL.iter().copied().filter(|p| p.group() == group).collect::<Vec<_>>();
            ui::col().child(div().px_2().pt_3().pb_1().child(ui::section_label(group.title(), ui::Heading::Faint, cx))).children(pages.into_iter().map(|page| item(page, cx)))
        };
        // lint-allow(unvirtualized-list): the page tree, about fifty rows at most
        div()
            .id("ds-tree")
            .w(px(TREE_WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .bg(palette::surface(Level::Sidebar, cx))
            .border_r_1()
            .border_color(palette::line(cx))
            .p_2()
            .children(Group::ALL.map(|g| group(g, cx)))
    }

    // The page's title, what it is, its named properties, and the filter over its frames.
    fn page_head(&self, counts: [usize; 4], _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let labels = ["All", "Variants", "States", "Edge cases", "Flows"];
        let selected = self.view.map_or(0, |kind| kind as usize + 1);
        let props = self.page.props().iter().map(|(name, values)| {
            ui::row().items_baseline().gap_4().py_1p5().border_t_1().border_color(palette::line(cx)).child(div().w(px(PROP_NAME)).flex_none().font_family(cx.theme().mono_font_family.clone()).text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(*name)).child(div().flex_1().min_w_0().text_color(palette::muted(cx)).child(*values))
        });
        let tally = Kind::ALL.iter().zip(counts).filter(|(_, n)| *n > 0).map(|(kind, n)| format!("{} {n}", kind.label())).collect::<Vec<_>>().join(" · ");
        ui::col()
            .gap_3()
            .pt_8()
            .child(div().text_size(ui::TEXT_XS).font_weight(FontWeight::MEDIUM).text_color(palette::faint(cx)).child(self.page.group().title()))
            .child(div().text_size(ui::TEXT_XL).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(self.page.title()))
            .child(lede(self.page.summary(), cx))
            .child(ui::col().max_w(px(LEDE_WIDTH * 1.5)).children(props))
            .child(div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(tally))
            .child(ui::Tabs::new("ds-view").labels(&labels).value(selected).on_value_change(move |i, _, cx| {
                this.update(cx, |this, cx| {
                    this.view = i.checked_sub(1).map(|k| Kind::ALL[k]);
                    cx.notify();
                })
                .ok();
            }))
    }
}

fn section(title: &'static str, lede_text: &'static str, cx: &App) -> Div {
    ui::col()
        .py_8()
        .border_b_1()
        .border_color(palette::line(cx))
        .child(div().text_size(ui::TEXT_LG).line_height(relative(1.2)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).pb_1().child(title))
        .child(lede(lede_text, cx).pb_4())
}

fn lede(text: &'static str, cx: &App) -> Div {
    div().max_w(px(LEDE_WIDTH)).text_color(palette::muted(cx)).child(text)
}

// The design page's board: the sidebar fill, a hairline, the card radius, 24px in.
fn board(cx: &App) -> Div {
    ui::col().p_6().squircle(ui::RADIUS_LG).bg(palette::surface(Level::Sidebar, cx)).border_1().border_color(palette::line(cx)).gap_4()
}

// A titled group inside a board.
fn group(title: &'static str, cx: &App) -> Div {
    stack().child(div().font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)).child(title))
}

fn row() -> Div {
    ui::row().gap_2().flex_wrap()
}

fn stack() -> Div {
    ui::col().gap_2()
}

fn note(text: &'static str, cx: &App) -> Div {
    div().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(text)
}

fn glyph(glyph: Glyph) -> impl IntoElement {
    Icon::new(glyph).size(ui::ICON_LG)
}

// The way in from the user menu (debug builds only: this module isn't compiled into release builds).
impl crate::Waffle {
    pub(crate) fn design_system_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let item = crate::menu::item("Design system", |_, _, _| {}).icon(Glyph::Settings);
        crate::menu::menu_item_row("design-system".into(), &item, false, true, cx).on_click(cx.listener(|this, _, window, cx| this.open_design_system(window, cx)))
    }

    fn open_design_system(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let waffle = cx.entity().downgrade();
        let back = move |_: &mut Window, cx: &mut App| {
            waffle.update(cx, |this, cx| {
                this.design_system = None;
                cx.notify(); // lint-allow(global-notify): leaving the design system brings every column back
            }).ok();
        };
        self.design_system = Some(view(back, window, cx));
        self.status_menu = false;
        cx.notify(); // lint-allow(global-notify): the design system replaces every column
    }
}
