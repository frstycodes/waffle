// The sign-in window, ported from design/auth.html (.au-login): the mascot or the person, a title, one
// line of why, the phase's card springing into place, and the promise at the bottom.
use std::{rc::Rc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, Context, Corners, Div, ElementId, FontWeight, HighlightStyle, SharedString, StyledText, Window,
    component::Icon,
    canvas, div, fill, point, prelude::*, px, size, svg,
};
use qrcode::{EcLevel, QrCode};

use crate::{
    Waffle,
    discord::User,
    icons::{self, Glyph},
    mascot::{Mood, mascot_at},
    motion, palette,
    ui::{self, Squircle, Tone},
    updater::{Command, Status},
};

// The code card: 204 square with 14 of white around the modules.
const QR_SIDE: f32 = 204.;
const QR_PAD: f32 = 14.;
// Finder patterns are 7 modules square.
const FINDER: usize = 7;

#[derive(Clone)]
pub enum Login {
    Preparing,
    Qr(Rc<Modules>),
    Scanned(Box<User>),
    /// Waiting for the captcha the browser is showing; holds that page's URL.
    Captcha(SharedString),
    Failed(SharedString),
}

pub struct Modules {
    width: usize,
    dark: Vec<bool>,
}

impl Login {
    pub fn qr(url: &str) -> Self {
        // High error correction, so the mark in the cleared centre doesn't cost a scan.
        match QrCode::with_error_correction_level(url.as_bytes(), EcLevel::H) {
            Ok(code) => Login::Qr(Rc::new(Modules { width: code.width(), dark: code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect() })),
            Err(error) => Login::Failed(error.to_string().into()),
        }
    }
}

// `back` is the signed-in account to return to while adding another.
// `update`: the updater's status, offered here too, so a build that can't sign in can still be replaced.
pub fn render(login: &Login, back: Option<crate::accounts::Account>, update: &Status, _: &mut Window, cx: &mut Context<Waffle>) -> AnyElement {
    let (phase, title, detail): (&str, SharedString, SharedString) = match login {
        Login::Preparing => ("preparing", "Sign in with your phone".into(), "Getting a fresh code from Discord\u{2026}".into()),
        Login::Qr(_) => ("qr", "Sign in with your phone".into(), "Scan with the Discord app on your phone.".into()),
        Login::Scanned(user) => ("scanned", format!("Hi, {}!", first_name(&user.name)).into(), "Approve the sign-in on your phone to finish.".into()),
        Login::Captcha(_) => ("captcha", "One more step".into(), "Discord wants a quick check. Solve the captcha in your browser and Waffle will finish signing in.".into()),
        Login::Failed(_) => ("failed", "Sign-in didn't finish".into(), "Sign-in stopped before it was approved. Get a new code and scan it again.".into()),
    };
    let lead = match login {
        Login::Preparing | Login::Captcha(_) => Some(mascot_at("login", Mood::Waiting, 0.6, cx)),
        Login::Failed(_) => Some(mascot_at("login", Mood::Idle, 0.6, cx)),
        Login::Scanned(user) => Some(arrive("card-scanned", avatar_pop(user, cx))),
        Login::Qr(_) => None,
    };
    let card = match login {
        Login::Preparing => Some(arrive("card-preparing", blank_card(cx))),
        Login::Qr(modules) => Some(arrive("card-qr", qr_card(modules.clone(), cx))),
        Login::Captcha(url) => {
            let url = url.clone();
            let open = ui::button("open-captcha", Tone::Primary, cx).child(Icon::new(Glyph::Open).size(ui::ICON_MD)).child("Open captcha again").on_click(move |_, _, cx| cx.open_url(&url));
            Some(arrive("card-captcha", ui::col().items_center().gap_3().child(waiting("captcha-wait", "Waiting for the captcha\u{2026}", cx)).child(open)))
        }
        Login::Failed(_) => Some(arrive(
            "card-failed",
            ui::button("retry", Tone::Primary, cx).child(Icon::new(Glyph::TryAgain).size(ui::ICON_MD)).child("Try again").on_click(cx.listener(|this, _, _, cx| this.sign_in(cx))),
        )),
        Login::Scanned(_) => None,
    };
    let muted = palette::muted(cx);
    let after = match login {
        Login::Qr(_) => Some(steps(cx).into_any_element()),
        Login::Scanned(_) => Some(
            ui::col()
                .items_center()
                .gap_5()
                .child(waiting("scanned-wait", "Check your phone", cx))
                .child(ui::button("new-code", ui::Tone::Link, cx).child("Not you? Get a new code").on_click(cx.listener(|this, _, _, cx| this.sign_in(cx))))
                .into_any_element(),
        ),
        _ => None,
    };
    // What happened in plain words above, then the system's own reason verbatim.
    let reason = match login {
        Login::Failed(error) => Some(
            div()
                .max_w(px(TEXT_WIDTH))
                .text_size(ui::TEXT_XS)
                .text_color(muted)
                .child(StyledText::new(format!("{REASON}{error}")).with_highlights([(REASON.len()..REASON.len() + error.len(), HighlightStyle { color: Some(palette::text(cx)), font_weight: Some(FontWeight::MEDIUM), ..Default::default() })])),
        ),
        _ => None,
    };
    ui::col()
        .id(SharedString::from(format!("login-{phase}")))
        .size_full()
        .items_center()
        .gap_3()
        .pt_6()
        .px_6()
        .pb_4()
        .text_center()
        .text_size(ui::TEXT_SM)
        .window_control_area(gpui_kit::WindowControlArea::Drag)
        .child(div().flex_1())
        .children(lead)
        .child(ui::line(title).max_w_full().text_size(ui::TEXT_LG).line_height(ui::LINE_LG).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)))
        .child(div().max_w(px(TEXT_WIDTH)).text_color(muted).child(detail))
        .children(reason)
        .children(card.map(|card| div().pt_1().child(card)))
        .children(after)
        .children(back.map(|account| {
            let label = format!("Back to {}", account.name);
            ui::button("back-to-account", ui::Tone::Link, cx).child(label).on_click(cx.listener(move |this, _, window, cx| this.switch_account(account.id.clone(), window, cx)))
        }))
        .child(div().flex_1())
        .children(update_notice(update, matches!(login, Login::Failed(_)), cx))
        .child(
            ui::row()
                .gap_1p5()
                .text_size(ui::TEXT_XS)
                .text_color(muted)
                .child(Icon::new(Glyph::HashtagLock).size(ui::ICON_SM))
                .child("Waffle never sees your password."),
        )
        .into_any_element()
}

// A newer Waffle, whatever its stage: looking, found (download it), downloading with its progress, ready
// (restart into it), or a download that failed (try again). After a failed sign-in (`failed`) the
// sign-in screen has already started the download, and the copy says the update may fix it.
fn update_notice(status: &Status, failed: bool, cx: &mut Context<Waffle>) -> Option<AnyElement> {
    // After a failure, Try again is the screen's one primary action; the update's button stays quiet.
    let tone = if failed { Tone::Secondary } else { Tone::Primary };
    let download = move |id: &'static str, label: &'static str, cx: &mut Context<Waffle>| {
        ui::button(id, tone, cx).small().child(label).on_click(cx.listener(|this, _, _, _| {
            if let Some(updater) = &this.updates.updater {
                updater.send(Command::Download);
            }
        }))
    };
    let might_fix = |version: &str| format!("Waffle {version} might fix this");
    let mut progress = None;
    let (title, note, action): (String, Option<String>, Option<AnyElement>) = match status {
        Status::Checking if failed => ("Looking for a newer Waffle\u{2026}".into(), Some("An update might fix this.".into()), None),
        Status::Available(version) => {
            let title = if failed { might_fix(version) } else { format!("Waffle {version} is available") };
            (title, None, Some(download("login-update-download", "Download", cx).into_any_element()))
        }
        Status::Downloading { version, done, total } => {
            let fraction = total.filter(|t| *t > 0).map(|t| *done as f32 / t as f32);
            progress = fraction;
            let percent = fraction.map(|f| format!(" {}%", (f * 100.).round() as u32)).unwrap_or_default();
            let title = if failed { might_fix(version) } else { format!("Downloading Waffle {version}") };
            (title, Some(format!("Downloading it now\u{2026}{percent}")), None)
        }
        Status::Ready(staged) => {
            let (title, note) = if failed { (might_fix(&staged.version), Some("It's downloaded. Restart to try it.".to_owned())) } else { (format!("Waffle {} is ready", staged.version), None) };
            let staged = staged.clone();
            let restart = ui::button("login-update-restart", tone, cx)
                .small()
                .child("Restart to update")
                .on_click(cx.listener(move |this, _, _, cx| this.request_restart(Some(staged.clone()), cx)));
            (title, note, Some(restart.into_any_element()))
        }
        Status::DownloadFailed(version) => (format!("Couldn't download Waffle {version}"), Some("Check your connection and try again.".into()), Some(download("login-update-retry", "Try again", cx).into_any_element())),
        Status::Checking | Status::Idle | Status::UpToDate | Status::CheckFailed => return None,
    };
    // Straight on the window's surface like the rest of the sign-in column: no box, set apart by space.
    let heading = ui::row()
        .max_w_full()
        .gap_1p5()
        .child(Icon::new(Glyph::Download).size(ui::ICON_MD).flex_none().text_color(palette::accent_ink(cx)))
        .child(ui::line(title).font_weight(FontWeight::MEDIUM).text_color(palette::strong(cx)));
    let notice = ui::col()
        // Clear of the footer line under it.
        .pb_4()
        // One width through every stage, so the progress bar doesn't jump as the copy changes mid-download.
        .w(px(UPDATE_WIDTH))
        .max_w_full()
        .items_center()
        .gap_1p5()
        .child(heading)
        .children(note.map(|note| div().text_size(ui::TEXT_XS).text_color(palette::muted(cx)).child(note)))
        .children(progress.map(|f| div().w_full().pt_1().child(ui::progress_bar(f, cx).thin())))
        .children(action.map(|action| div().pt_1p5().child(action)));
    Some(arrive("login-update", notice))
}

// The update notice's width, the QR card's give or take, so it reads as part of the column.
const UPDATE_WIDTH: f32 = 280.;

// GAP: the design reads "Discord said:", but auth.rs's errors also come from the network and the
// captcha page, so the label doesn't name a source.
const REASON: &str = "Reason: ";

// About 34 characters of the body text.
const TEXT_WIDTH: f32 = 270.;

// First name only: the least identity the screen needs.
fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

// The code card while Discord prepares a code: its final size, shimmering.
fn blank_card(cx: &App) -> AnyElement {
    ui::skeleton("qr-shimmer", ui::RADIUS_LG, cx).size(px(QR_SIDE)).flex_none().into_any_element()
}

// Graphite ink squircles on white, finder patterns as rounded rings, the waffle mark in the cleared centre.
// lint-allow(custom-component): ui::card is a clickable raised card; this is the sign-in QR code painted on white in both themes.
fn qr_card(modules: Rc<Modules>, cx: &App) -> AnyElement {
    let inner = QR_SIDE - 2. * QR_PAD;
    let module = inner / modules.width as f32;
    let clear = centre_clear(modules.width);
    let shadow = vec![
        ui::shadow(palette::line_strong(cx), 0., 0., 0., 1., false),
        ui::shadow(palette::shade(cx).opacity(0.18), 0., 8., 24., 0., false),
    ];
    let mark = (clear.end - clear.start) as f32 * module * 0.78;
    let code = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let ink = palette::qr_ink();
            let at = |x: f32, y: f32, side: f32| gpui_kit::Bounds::new(point(bounds.origin.x + px(x * module), bounds.origin.y + px(y * module)), size(px(side * module), px(side * module)));
            let w = modules.width;
            for (i, _) in modules.dark.iter().enumerate().filter(|(_, dark)| **dark) {
                let (x, y) = (i % w, i / w);
                if in_finder(x, y, w) || (clear.contains(&x) && clear.contains(&y)) {
                    continue;
                }
                // A hair under a module, so neighbours read as separate squircles.
                let inset = 0.06;
                window.paint_quad(fill(at(x as f32 + inset, y as f32 + inset, 1. - 2. * inset), ink).corner_radii(Corners::all(px(module * 0.32))));
            }
            for (fx, fy) in [(0, 0), (w - FINDER, 0), (0, w - FINDER)] {
                let ring = gpui_kit::quad(at(fx as f32, fy as f32, FINDER as f32), Corners::all(px(module * 2.3)), gpui_kit::transparent_black(), px(module), ink, gpui_kit::BorderStyle::Solid);
                window.paint_quad(ring);
                window.paint_quad(fill(at(fx as f32 + 2., fy as f32 + 2., 3.), ink).corner_radii(Corners::all(px(module * 1.1))));
            }
        },
    )
    .size(px(inner));
    // lint-allow(custom-surface): ui::surface is the theme's overlay fill; the code sits on white in both themes so phones can read it.
    div()
        .relative()
        .size(px(QR_SIDE))
        .p(px(QR_PAD))
        .squircle(ui::RADIUS_LG)
        .bg(gpui_kit::white())
        .shadow(shadow)
        .child(code)
        .child(ui::row().absolute().inset_0().justify_center().child(svg().path(icons::MARK).size(px(mark)).text_color(palette::logo_on_light())))
        .into_any_element()
}

fn in_finder(x: usize, y: usize, width: usize) -> bool {
    let near = |v: usize| v < FINDER;
    let far = |v: usize| v >= width - FINDER;
    (near(x) && near(y)) || (far(x) && near(y)) || (near(x) && far(y))
}

// The module rows and columns left blank for the mark: an odd span about a fifth of the code.
fn centre_clear(width: usize) -> std::ops::Range<usize> {
    let span = ((width as f32 / 5.).round() as usize) | 1;
    let start = (width - span) / 2;
    start..start + span
}

// Where the scanner lives in Discord's app.
fn steps(cx: &App) -> Div {
    let strong = palette::strong(cx);
    let step = |n: &'static str, before: &'static str, bold: &'static str, after: &'static str| {
        let text = format!("{before}{bold}{after}");
        let range = before.len()..before.len() + bold.len();
        ui::row()
            .gap_2()
            .child(ui::badge(palette::selected(cx), strong).size_5().justify_center().child(n))
            .child(StyledText::new(text).with_highlights([(range, HighlightStyle { color: Some(strong), font_weight: Some(FontWeight::MEDIUM), ..Default::default() })]))
    };
    ui::col()
        .pt_1()
        .gap_1p5()
        .text_color(palette::text(cx))
        .child(step("1", "In Discord, open ", "Settings", ""))
        .child(step("2", "Choose ", "Scan QR Code", " and point it here"))
    // GAP: the design's "Sign in with a token instead" disclosure needs a token sign-in path that
    // auth.rs doesn't have yet.
}

// The mini wave and what it waits on.
fn waiting(id: &'static str, text: &'static str, cx: &App) -> Div {
    ui::row().gap_2().font_weight(FontWeight::NORMAL).text_color(palette::muted(cx)).child(ui::bars(id, palette::accent(), 12.)).child(text)
}

// The account that scanned springs in from half size, the phone it's approving on badged at its foot.
fn avatar_pop(user: &User, cx: &App) -> AnyElement {
    // lint-allow(custom-mark, custom-surface): ui::badged marks a corner with a count or a status; this is the 32px phone chip, ringed by a hairline.
    let badge = ui::row()
        .absolute()
        .right_neg_0p5()
        .bottom_neg_0p5()
        .size_8()
        .rounded_full()
        .bg(palette::surface(palette::Level::Page, cx))
        .text_color(palette::strong(cx))
        .shadow(vec![ui::shadow(palette::line_strong(cx), 0., 0., 0., 1., false)])
        .justify_center()
        .child(Icon::new(Glyph::DeviceMobile).size(ui::ICON_LG));
    let spring = motion::ease_spring();
    div()
        .relative()
        .size_24()
        .child(ui::avatar(user.avatar.clone(), 96.))
        .child(badge)
        .with_animation("avatar-pop", Animation::new(Duration::from_millis(320)).with_easing(move |t| spring.sample(t)), |avatar, t| avatar.scale(0.5 + 0.5 * t).opacity(t.clamp(0., 1.)))
        .into_any_element()
}

// Each phase's card arrives from 12px below and scale 0.97, fading in on the spring.
fn arrive(id: impl Into<ElementId>, card: impl IntoElement) -> AnyElement {
    let spring = motion::ease_spring();
    div()
        .child(card)
        .with_animation(id, Animation::new(Duration::from_millis(260)).with_easing(move |t| spring.sample(t)), |el, t| {
            // lint-allow(raw-px): the rise's distance is computed from the animation's progress.
            el.translate_y(px(12. * (1. - t))).scale(0.97 + 0.03 * t).opacity(t.clamp(0., 1.))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_clear_is_odd_and_centred() {
        for width in [21, 25, 33, 41] {
            let clear = centre_clear(width);
            assert_eq!((clear.end - clear.start) % 2, 1);
            assert_eq!(clear.start, width - clear.end, "centred in {width}");
            assert!(clear.start >= FINDER, "clear of the finders in {width}");
        }
    }
}
