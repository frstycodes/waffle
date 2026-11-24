// The full profile, Discord's large dialog: the profile card on the left (banner, avatar, name, actions, bio,
// dates, roles, connections and your note), and on the right Activity, Mutual Friends and Mutual Servers tabs.
use gpui_kit::{
    AnyElement, Context, Div, Focusable, SharedString, Window,
    component::input::{InputEvent, Textarea, TextareaState},
    div, img, px, uniform_list,
};

use super::*;
use crate::{
    mascot::Mood,
    ui::{RADIUS_LG, RowHeight},
};

// Discord's `user-profile-modal-v2` sizes: at most 962 by 800, 48px clear of the window on every side, and
// never shorter than 550 while the window allows it.
const MODAL_WIDTH: f32 = 962.;
const MODAL_HEIGHT: f32 = 800.;
const MODAL_MIN_HEIGHT: f32 = 550.;
const MODAL_CLEARANCE: f32 = 48.;
// The card column: 45.5% of the window less 43.68px (48 × 0.91), between 300 and 400.
const CARD_SHARE: f32 = 0.455;
const CARD_OFFSET: f32 = 43.68;
const CARD_MIN: f32 = 300.;
const CARD_MAX: f32 = 400.;
// The dialog's padding and the gutter between card and tabs; the card's own padding.
const PADDING: f32 = 32.;
const AVATAR_FULL: f32 = 120.;
// The status bubble's offsets inside the card: past the avatar, and up into the banner's foot.
const BUBBLE_FROM_AVATAR: f32 = 20.;
const BUBBLE_RISE: f32 = 8.;
// The banner picture behind the whole dialog, faded out by this depth.
const BACKDROP_DEPTH: f32 = 280.;
// A row in the mutual lists, and the face or tile it leads with.
const MUTUAL_ROW: f32 = 60.;
const MUTUAL_FACE_FULL: f32 = 40.;
// Discord's note limit.
const NOTE_MAX: usize = 256;

impl Waffle {
    pub(super) fn full_profile_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let card = self.profile_card.as_ref()?;
        let tab = card.full?;
        let (hovered, on_hover) = ui::hover_state(SharedString::from(format!("full-profile-hover-{}", card.user.id)), window, cx);
        let profile = card.profile.clone().unwrap_or_default();
        let ink = Ink::new(profile.theme, cx);
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let width = (vw - 2. * MODAL_CLEARANCE).min(MODAL_WIDTH);
        let height = (vh - 2. * MODAL_CLEARANCE).min(MODAL_HEIGHT).max(MODAL_MIN_HEIGHT.min(vh - 2. * ui::SURFACE_INSET));
        let card_width = (CARD_SHARE * vw - CARD_OFFSET).clamp(CARD_MIN, CARD_MAX);
        let left = self.full_card(card, &profile, hovered, ink, cx).w(px(card_width)).flex_none();
        let right = self.full_tabs(card, &profile, tab, ink, window, cx);
        let fill = match ink.border {
            Some(_) => ink.wash(),
            None => palette::surface(palette::Level::Page, cx).into(),
        };
        let backdrop_art = profile.banner.clone().map(|url| {
            let fade = palette::surface(palette::Level::Page, cx);
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(BACKDROP_DEPTH))
                .child(img(url).size_full().object_fit(gpui_kit::ObjectFit::Cover).opacity(0.3))
                .child(div().absolute().inset_0().bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(fade.opacity(0.4), 0.), gpui_kit::linear_color_stop(fade, 1.))))
        });
        let close = ui::icon_button("full-profile-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| {
            this.dismiss_overlay(cx);
        }));
        let inner = div()
            .id("full-profile")
            .relative()
            .size_full()
            .bg(fill)
            .on_hover(on_hover)
            .children((ink.border.is_none()).then_some(backdrop_art).flatten())
            .child(ui::row().relative().items_stretch().size_full().pt(px(PADDING)).pl(px(PADDING)).child(left).child(right))
            .child(ui::row().absolute().top_2().right_2().gap_2().child(close));
        let body = div().h(px(height)).child(inner);
        let avatar = card.avatar_full.then(|| avatar_viewer(&card.user, window, cx));
        let backdrop = crate::overlays::backdrop("full-profile-backdrop", cx);
        let dialog = ui::dialog("full-profile-dialog", width, body, window, cx);
        Some(div().absolute().inset_0().child(ui::modal(backdrop, dialog, cx)).children(avatar).into_any_element())
    }

    // The left card: header, then a scrolling body in Discord's order.
    fn full_card(&self, card: &ProfileCard, profile: &Profile, hovered: bool, ink: Ink, cx: &mut Context<Self>) -> Div {
        let radius = inner_radius(ink);
        let (banner, _) = banner(profile, ink, radius, hovered, BANNER_FULL, cx);
        let bubble = self.status_bubble_for(&card.user, ink, cx).map(|bubble| {
            bubble.absolute().top(px(BANNER_FULL - BUBBLE_RISE)).left(px(PADDING + AVATAR_FULL + BUBBLE_FROM_AVATAR)).right_4()
        });
        let header = ui::col()
            .relative()
            .flex_none()
            .child(banner)
            // lint-allow(raw-margin): the avatar pulls up over the banner's foot by half its size, which padding can't do.
            .child(ui::row().px(px(PADDING - AVATAR_RING)).mt(px(-(AVATAR_FULL / 2. + AVATAR_RING))).child(self.profile_face(card, AVATAR_FULL, hovered, ink, cx)))
            .children(bubble);
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let sections = ui::col()
            .px(px(PADDING))
            .pt_1()
            .pb(px(PADDING))
            .gap_6()
            .child(self.clickable_name(card, profile, ink, cx))
            .when(card.profile.is_none(), |b| b.child(profile_skeleton(cx)))
            .child(self.full_buttons(card, is_me, cx))
            .children(profile.bio.as_ref().map(|bio| bio_view(&card.user.id, bio, ink)))
            .children(self.member_since(card, profile, ink, cx))
            .children(self.friends_since(card, ink))
            .children(self.roles_row(card, profile, false, ink, cx).map(|roles| section("Roles", roles, ink)))
            .children(connections(&profile.connections, ink, cx))
            .children(game_collection(&profile.games, ink))
            .children((!is_me).then(|| self.note_section(card, profile, ink, cx)));
        // A block around the column: inside the full-height scroller, a flex column would squeeze its rows to fit.
        let body = ui::scroll_area(div().id("full-profile-body").child(sections), &card.card_scroll);
        let card_fill = match ink.border {
            Some(_) => ink.wash(),
            None => palette::surface(palette::Level::Raised, cx).into(),
        };
        ui::edge(ui::col().min_h_0(), ui::top(RADIUS_LG)).bg(card_fill).shadow(ui::shadow_pop(cx)).child(header).child(ui::col().flex_1().min_h_0().child(body))
    }

    // Discord's row under the name: Message, the friend state, then More. Yours is Edit Profile.
    fn full_buttons(&self, card: &ProfileCard, is_me: bool, cx: &mut Context<Self>) -> Div {
        if is_me {
            let edit = ui::button("full-profile-edit", ui::Tone::Primary, cx).child(Icon::new(Glyph::Pen).size(ui::ICON_MD)).child("Edit Profile").on_click(cx.listener(|this, _, window, cx| this.edit_own_profile(ProfileTab::Main, window, cx)));
            return ui::row().gap_2().child(edit);
        }
        let user = card.user.clone();
        let dm = user.id.clone();
        let message = ui::button("full-profile-message", ui::Tone::Primary, cx).child(Icon::new(Glyph::ChatRound).size(ui::ICON_MD)).child("Message").on_click(cx.listener(move |this, _, window, cx| this.message_user(dm.clone(), window, cx)));
        let friend = self.friend_action(&user).map(|(glyph, label, next)| {
            let target = user.clone();
            let button = match next {
                // Add Friend and Accept say so; a settled state is its icon alone, named in its tooltip.
                Some(_) => ui::button("full-profile-friend", ui::Tone::Secondary, cx).child(Icon::new(glyph).size(ui::ICON_MD)).child(label),
                None => ui::icon_button("full-profile-friend", glyph, label, cx).tone(ui::Tone::Secondary, cx),
            };
            button.on_click(cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| this.friend_click(target.clone(), event.position(), cx)))
        });
        let more = self.more_button(card, false, ui::icon_button("full-profile-more", Glyph::More, "More", cx).tone(ui::Tone::Secondary, cx), cx);
        ui::row().gap_2().child(message).children(friend).child(more)
    }

    // "Note (only visible to you)": the note as text until clicked, then a field that saves when it loses focus.
    fn note_section(&self, card: &ProfileCard, profile: &Profile, ink: Ink, cx: &mut Context<Self>) -> Div {
        let field = match &card.note {
            Some(input) => {
                let focus = input.read(cx).focus_handle(cx);
                ui::text_area("full-profile-note", Textarea::new(input).appearance(false).px_0().text_size(ui::TEXT_SM), &focus).into_any_element()
            }
            None => {
                let text = profile.note.clone();
                let shown = text.clone().unwrap_or_else(|| "Click to add a note".into());
                let ink_for = if text.is_some() { ink.text } else { ink.muted };
                // lint-allow(custom-button): ui::text_area is the editor itself; this is the note at rest, a click from becoming one.
                ui::row()
                    .id("full-profile-note-rest")
                    .min_h_9()
                    .px_2p5()
                    .py_2()
                    .items_start()
                    .squircle(RADIUS_XS)
                    .bg(ink.tile)
                    .cursor_text()
                    .text_size(ui::TEXT_SM)
                    .text_color(ink_for)
                    .child(div().min_w_0().child(shown))
                    .on_click(cx.listener(move |this, _, window, cx| this.edit_profile_note(text.clone().unwrap_or_default(), window, cx)))
                    .into_any_element()
            }
        };
        section("Note (only visible to you)", field, ink)
    }

    // Swaps the note's text for a field holding it; leaving the field saves it.
    pub(super) fn edit_profile_note(&mut self, current: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Click to add a note").auto_grow(1, 8).default_value(current));
        cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Blur) {
                let note: String = input.read(cx).value().trim().chars().take(NOTE_MAX).collect();
                this.save_profile_note(note, cx);
            }
        })
        // lint-allow(detached-task): the subscription lives as long as the field, dropped with the card.
        .detach();
        input.update(cx, |state, cx| state.focus(window, cx));
        if let Some(card) = self.profile_card.as_mut() {
            card.note = Some(input);
        }
        self.redraw_overlays(cx);
    }

    // PUT /users/@me/notes/{id}, then shows the note at rest; the cached profile keeps it for next time.
    fn save_profile_note(&mut self, note: String, cx: &mut Context<Self>) {
        let Some(card) = self.profile_card.as_mut() else { return };
        card.note = None;
        let saved = (!note.is_empty()).then(|| note.clone());
        let user_id = card.user.id.clone();
        let changed = card.profile.as_ref().is_none_or(|p| p.note != saved);
        if let Some(profile) = card.profile.as_mut() {
            profile.note = saved.clone();
        }
        for key in [profile_key(&user_id, card.guild_id.as_deref()), profile_key(&user_id, None)] {
            if let Some(mut profile) = self.profiles.peek(&key) {
                profile.note = saved.clone();
                self.profiles.seed(&key, profile);
            }
        }
        if changed {
            self.rest(reqwest::Method::PUT, format!("/users/@me/notes/{user_id}"), Some(serde_json::json!({ "note": note })), |_, _| {}, cx);
        }
        self.redraw_overlays(cx);
    }

    // The right column: Discord's tab bar, then the chosen tab's list.
    fn full_tabs(&self, card: &ProfileCard, profile: &Profile, tab: FullTab, ink: Ink, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        let mut tabs = vec![(FullTab::Activity, ui::Tab::new("Activity"))];
        if !is_me {
            tabs.push((FullTab::MutualFriends, ui::Tab::new(mutual_tab(profile.mutual_friend_count, "Mutual Friend"))));
            tabs.push((FullTab::MutualServers, ui::Tab::new(mutual_tab(profile.mutual_server_count, "Mutual Server"))));
        }
        let selected = tabs.iter().position(|(t, _)| *t == tab).unwrap_or(0);
        let order: Vec<FullTab> = tabs.iter().map(|(t, _)| *t).collect();
        let items: Vec<ui::Tab> = tabs.into_iter().map(|(_, t)| t).collect();
        let this = cx.weak_entity();
        let strip = ui::tabs(
            "full-profile-tabs",
            &items,
            selected,
            move |i, _, cx| {
                let Some(&tab) = order.get(i) else { return };
                this.update(cx, |this, cx| {
                    if let Some(card) = this.profile_card.as_mut() {
                        card.full = Some(tab);
                    }
                    this.redraw_overlays(cx);
                })
                .ok();
            },
            window,
            cx,
        );
        let panel = match tab {
            FullTab::Activity => self.activity_tab(card, ink, cx),
            FullTab::MutualFriends => self.mutual_friends_tab(card, profile, cx),
            FullTab::MutualServers => self.mutual_servers_tab(profile, cx),
        };
        ui::col().flex_1().min_w_0().min_h_0().pl(px(PADDING)).pr(px(PADDING)).gap_5().child(div().pr_8().child(strip)).child(panel.flex_1().min_h_0().pb_4())
    }

    // "Current activity" with every card, or Discord's empty line and a way to say hi.
    fn activity_tab(&self, card: &ProfileCard, ink: Ink, cx: &mut Context<Self>) -> Div {
        let cards = self.profile_activity_cards(card, ink, cx);
        if cards.is_empty() {
            return self.no_activity(card, cx);
        }
        let list = section("Current activity", ui::col().gap_2().children(cards), ink);
        ui::col().child(ui::scroll_area(div().id("full-profile-activity").child(list), &card.tab_scroll))
    }

    fn no_activity(&self, card: &ProfileCard, cx: &mut Context<Self>) -> Div {
        let is_me = self.me.as_ref().is_some_and(|me| me.id == card.user.id);
        if is_me {
            return ui::col().justify_center().child(ui::empty_state("full-profile-no-activity", Mood::Idle, "You don't have any activity here", "Connect your accounts to share your in-game status and activity. You're in control of what you share.", cx));
        }
        let dm = card.user.id.clone();
        let say_hi = ui::button("full-profile-say-hi", ui::Tone::Secondary, cx).child(Icon::new(Glyph::ChatRound).size(ui::ICON_MD)).child("Message").on_click(cx.listener(move |this, _, window, cx| this.message_user(dm.clone(), window, cx)));
        let heading = format!("{} doesn't have any activity to share here", card.user.name);
        ui::col().items_center().justify_center().child(ui::empty_state("full-profile-no-activity", Mood::Idle, heading, pick(NO_ACTIVITY, &card.user.id), cx).child(say_hi))
    }

    fn mutual_friends_tab(&self, card: &ProfileCard, profile: &Profile, cx: &mut Context<Self>) -> Div {
        if profile.mutual_friends.is_empty() {
            return ui::col().justify_center().child(ui::empty_state("full-profile-no-friends", Mood::Idle, "You don't have any friends in common", pick(NO_FRIENDS, &card.user.id), cx));
        }
        let people: Vec<(User, Option<crate::discord::Mark>)> = profile.mutual_friends.iter().map(|u| (u.clone(), self.presences.get(&u.id).map(|p| p.mark()))).collect();
        let surface = palette::surface(palette::Level::Page, cx);
        let this = cx.weak_entity();
        let list = uniform_list("full-profile-mutual-friends", people.len(), move |range, _, cx| {
            people[range]
                .iter()
                .map(|(user, mark)| {
                    let (this, target) = (this.clone(), user.clone());
                    ui::list_row(SharedString::from(format!("mutual-friend-{}", user.id)), user.name.clone(), cx)
                        .lead(decorated_avatar(&user.avatar, user.decoration.as_deref(), MUTUAL_FACE_FULL, *mark, surface))
                        .height(RowHeight::Exact(MUTUAL_ROW))
                        .on_click(move |_, _, cx| {
                            this.update(cx, |this, cx| this.open_full_profile(target.clone(), FullTab::Activity, cx)).ok();
                        })
                })
                .collect()
        });
        ui::col().child(list.size_full())
    }

    fn mutual_servers_tab(&self, profile: &Profile, cx: &mut Context<Self>) -> Div {
        let servers: Vec<(String, String, Option<String>, Option<String>)> = profile
            .mutual_guilds
            .iter()
            .filter_map(|m| self.guilds.iter().find(|g| g.id == m.id).map(|g| (g.id.clone(), g.name.clone(), g.icon.clone(), m.nick.clone())))
            .collect();
        if servers.is_empty() {
            let id = self.profile_card.as_ref().map(|c| c.user.id.clone()).unwrap_or_default();
            return ui::col().justify_center().child(ui::empty_state("full-profile-no-servers", Mood::Idle, "You don't have any servers in common", pick(NO_SERVERS, &id), cx));
        }
        let this = cx.weak_entity();
        let list = uniform_list("full-profile-mutual-servers", servers.len(), move |range, _, cx| {
            servers[range]
                .iter()
                .map(|(id, name, icon, nick)| {
                    let (this, target) = (this.clone(), id.clone());
                    ui::list_row(SharedString::from(format!("mutual-server-{id}")), name.clone(), cx)
                        .lead(ui::server_tile(icon.as_deref(), name, MUTUAL_FACE_FULL, cx))
                        .when_some(nick.clone(), |r, nick| r.subtitle(ui::line(nick)))
                        .height(RowHeight::Exact(MUTUAL_ROW))
                        .on_click(move |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.profile_card = None;
                                this.open_guild(Some(target.clone()), window, cx);
                            })
                            .ok();
                        })
                })
                .collect()
        });
        ui::col().child(list.size_full())
    }
}

// The tab's label: "No Mutual Friends" with none, else the count.
fn mutual_tab(n: usize, noun: &str) -> String {
    if n == 0 { format!("No {noun}s") } else { mutual_count(n, noun) }
}

// Discord draws one of these at random each time; this keeps one per person so a redraw doesn't change it.
fn pick(lines: &'static [&'static str], key: &str) -> &'static str {
    let n = key.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    lines[n % lines.len()]
}

const NO_ACTIVITY: &[&str] = &[
    "Looks like this profile is still a work in progress - send them a message to say hi!",
    "Profile under construction 🚧 - ask them about their plans for decorating",
    "No activity shared yet - drop them a friendly hello to start chatting",
    "Gaming console offline - they must be achievement hunting in secret",
    "So mysterious! ask them to tell you a secret",
];
const NO_FRIENDS: &[&str] = &[
    "No mutual friends yet - perfect time to make a new connection",
    "Join the same servers to meet more mutual friends 🤝",
    "Your friend networks are like separate galaxies - time for them to collide!",
    "Friendship multiverse detected - time for a crossover episode! 🎬",
];
const NO_SERVERS: &[&str] = &[
    "Time to impress them with your taste...drop an invite link to your favorite server",
    "No shared server = no shared emoji options...time to create a server for all your favorite memes together",
    "Join the same servers to unlock shared emotes and more VC channels to hang in",
    "Ask them to invite you to their favorite server so you can bond",
    "Shared server inventory: empty but full of potential ✨",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_labels_and_lines() {
        assert_eq!(mutual_tab(0, "Mutual Friend"), "No Mutual Friends");
        assert_eq!(mutual_tab(1, "Mutual Server"), "1 Mutual Server");
        assert_eq!(pick(NO_SERVERS, "8388608"), pick(NO_SERVERS, "8388608"));
        assert!(NO_FRIENDS.contains(&pick(NO_FRIENDS, "")));
    }
}
