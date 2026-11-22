// The Profiles page's preview: your popout built from the same pieces as the real one, fed the draft.
use std::path::PathBuf;

use gpui_kit::{App, Div, IntoElement, ParentElement, Styled, div, img, prelude::FluentBuilder, px};

use crate::{
    discord::{Mark, Profile, User},
    layout::decorated_avatar,
    profile::{AVATAR, AVATAR_RING, Ink, WIDTH, badges, banner, bio_view, inner_radius, name_block, themed_card},
};

// A picked file shows before it's uploaded; everything else is an address.
pub(super) enum PreviewAvatar {
    Url(String),
    File(PathBuf),
}

pub(super) fn profile_preview(user: &User, profile: &Profile, avatar: PreviewAvatar, status: Option<Mark>, cx: &App) -> Div {
    let ink = Ink::new(profile.theme, cx);
    let radius = inner_radius(ink);
    let (banner, _) = banner(profile, ink, radius, false, cx);
    let face = match avatar {
        PreviewAvatar::Url(url) => decorated_avatar(&url, user.decoration.as_deref(), AVATAR, status, ink.top),
        PreviewAvatar::File(path) => img(path).size(px(AVATAR)).flex_none().rounded_full().into_any_element(),
    };
    let header = div()
        .flex_none()
        .flex()
        .flex_col()
        .child(banner)
        .child(div().px(px(16. - AVATAR_RING)).mt(px(-(AVATAR / 2. + AVATAR_RING))).mb_2().flex().child(div().p(px(AVATAR_RING)).rounded_full().bg(ink.top).child(face)));
    let body = div()
        .px_4()
        .pb_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(name_block(user, profile, ink))
        .children(badges(&profile.badges, ink))
        .when_some(profile.bio.as_ref(), |b, bio| b.child(bio_view("preview", bio, ink)));
    let (outer, inner) = themed_card(ink, f32::MAX, cx);
    outer.w(px(WIDTH)).child(inner.child(header).child(body))
}
