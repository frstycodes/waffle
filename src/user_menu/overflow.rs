// The small ⋯ menus Discord keeps apart from the user menu: an activity card's and a Friends row's.
// Discord draws these items without icons.
use gpui_kit::{Context, Pixels, Point};

use crate::{
    Waffle,
    discord::{ActivityKind, RichPresence, User},
    menu::{Entry, item},
};

impl Waffle {
    // Discord's activity menu: Manage Privacy on your own game, Report Issue on anyone's. Its other items
    // need what Waffle doesn't have (game profiles, recent activity, the full profile from a popout).
    pub(crate) fn activity_menu(&self, user: &User, rich: &RichPresence) -> Vec<Entry> {
        if user.bot || rich.kind != ActivityKind::Playing {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.my_id() == user.id {
            out.push(item("Manage Privacy", |this, window, cx| this.open_settings(crate::settings::Page::ActivityPrivacy, window, cx)).entry());
        }
        out.push(item("Report Issue", |this, window, cx| this.report_game_issue(window, cx)).entry());
        out
    }

    pub(crate) fn open_activity_menu(&mut self, user: User, rich: RichPresence, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.open_live_menu(at, move |this| this.activity_menu(&user, &rich), cx);
    }

    // GAP: Discord sends this only as an analytics event (GAME_PROFILE_FEEDBACK); Waffle sends no
    // analytics, so the answer goes nowhere until it has somewhere to send it.
    fn report_game_issue(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        self.prompt("What went wrong?", "Please describe any issues you're seeing, or how we can improve game information or game activity.", "", "Submit", |_, _, _, _| {}, window, cx);
    }

    // Discord's Friends row ⋯: call them, with or without video, or remove them.
    pub(crate) fn open_friend_row_menu(&mut self, user: User, at: Point<Pixels>, cx: &mut Context<Self>) {
        let (video, voice, removed) = (user.id.clone(), user.id.clone(), user.clone());
        let entries = vec![
            item("Start Video Call", move |this, window, cx| this.start_call(video.clone(), true, window, cx)).entry(),
            item("Start Voice Call", move |this, window, cx| this.start_call(voice.clone(), false, window, cx)).entry(),
            item("Remove Friend", move |this, _, cx| this.ask_remove_friend(&removed, cx)).danger().entry(),
        ];
        self.open_menu(at, entries, cx);
    }
}
