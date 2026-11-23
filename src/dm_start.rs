// The top of a DM's history (design: messages.html#dm-start): who this is, what you share, and the one
// next step. It is the feed's first item once the beginning is loaded, so it stays as messages arrive and
// scrolls away with the history, as Discord's does.
//
// GAP: a group gets no actions: Invite to group and Edit group aren't built. Report spam isn't either.
use gpui_kit::{AnyElement, App, Context, FontWeight, SharedString, WeakEntity, div, prelude::*, relative};

use crate::{
    Waffle,
    chat::emphasised,
    discord::{Channel, ChannelKind, DmFace, Mark, Relation, User},
    icons::Glyph,
    layout::decorated_avatar,
    palette::{self, Level},
    profile::profile_key,
    ui::{self, Tone},
};

const FACE: f32 = 80.;

#[derive(Clone)]
pub(crate) struct DmStart {
    face: DmFace,
    status: Option<Mark>,
    name: SharedString,
    // A person's username, or a group's member count.
    subtitle: Option<SharedString>,
    line: Vec<(String, bool)>,
    mutual: Option<SharedString>,
    person: Option<User>,
    next: Next,
}

// What the header offers a person, by where you stand with them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Next {
    // A group: nothing yet (see the GAP above).
    Nothing,
    Befriend,
    Accept,
    Pending,
    Friends,
    Blocked,
    // A bot can't be friended, only blocked.
    BlockOnly,
}

// The header leads a DM's feed once nothing older is left to load.
pub(crate) fn shows(kind: ChannelKind, has_more: bool) -> bool {
    kind == ChannelKind::Dm && !has_more
}

pub(crate) fn next_step(relation: Option<Relation>, bot: bool) -> Next {
    match relation {
        Some(Relation::Friend) => Next::Friends,
        Some(Relation::Blocked) => Next::Blocked,
        Some(Relation::Incoming) => Next::Accept,
        Some(Relation::Outgoing) => Next::Pending,
        None if bot => Next::BlockOnly,
        None => Next::Befriend,
    }
}

impl Waffle {
    // A 1:1 DM's username and mutual servers come from the person's profile: read from disk or fetched
    // once as the DM opens.
    pub(crate) fn load_dm_profile(&mut self, channel: &Channel, cx: &mut Context<Self>) {
        let Some(person) = channel.recipient.clone() else { return };
        if self.profiles.cached(&profile_key(&person.id, None)).is_none() {
            self.fetch_profile(person, None, cx);
        }
    }

    pub(crate) fn dm_start(&self, channel: &Channel, has_more: bool) -> Option<DmStart> {
        if !shows(channel.kind, has_more) {
            return None;
        }
        let Some(person) = channel.recipient.clone() else {
            return Some(DmStart {
                face: channel.face(),
                status: None,
                name: channel.name.clone().into(),
                subtitle: Some(format!("{} members", channel.recipients.len() + 1).into()),
                line: vec![("Welcome to the start of ".into(), false), (channel.name.clone(), true), (".".into(), false)],
                mutual: None,
                person: None,
                next: Next::Nothing,
            });
        };
        let profile = self.profiles.peek(&profile_key(&person.id, None));
        Some(DmStart {
            face: channel.face(),
            status: self.presences.get(&person.id).map(|p| p.mark()),
            name: person.name.clone().into(),
            subtitle: profile.as_ref().map(|p| p.username.clone()).filter(|u| !u.is_empty()).map(Into::into),
            line: vec![("This is the start of your messages with ".into(), false), (person.name.clone(), true), (".".into(), false)],
            mutual: profile.map(|p| mutual_servers(p.mutual_server_count).into()),
            next: next_step(self.relation(&person.id), person.bot),
            person: Some(person),
        })
    }
}

fn mutual_servers(count: usize) -> String {
    match count {
        0 => "No mutual servers".into(),
        1 => "1 mutual server".into(),
        n => format!("{n} mutual servers"),
    }
}

pub(crate) fn dm_start_header(start: &DmStart, this: WeakEntity<Waffle>, cx: &App) -> AnyElement {
    let page = palette::surface(Level::Page, cx);
    let face = match (&start.face, start.person.is_some()) {
        (DmFace::One(url), true) => decorated_avatar(url, None, FACE, start.status, page),
        (face, _) => ui::dm_face(face, FACE, page),
    };
    ui::col()
        .w_full()
        .px_4()
        .pt_2()
        .pb_4()
        .items_start()
        .gap_1()
        .child(div().mb_2().child(face))
        .child(ui::line(start.name.clone()).max_w_full().text_size(ui::TEXT_XL).line_height(relative(1.2)).font_weight(FontWeight::BOLD).text_color(palette::strong(cx)))
        .children(start.subtitle.clone().map(|s| ui::line(s).max_w_full().text_size(ui::TEXT_MD).text_color(palette::muted(cx))))
        .child(div().mt_1p5().text_size(ui::TEXT_MD).text_color(palette::muted(cx)).child(emphasised(&start.line, palette::strong(cx))))
        .children(start.mutual.clone().map(|m| ui::row().mt_0p5().gap_1p5().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child(gpui_kit::component::Icon::new(Glyph::Users).size(ui::ICON_MD)).child(m)))
        .children(actions(start, this, cx))
        .into_any_element()
}

fn actions(start: &DmStart, this: WeakEntity<Waffle>, cx: &App) -> Option<AnyElement> {
    let person = start.person.clone()?;
    let row = ui::row().mt_3().flex_wrap().gap_2();
    let act = move |id: &'static str, tone: Tone, label: &'static str, glyph: Option<Glyph>, run: fn(&mut Waffle, User, &mut Context<Waffle>)| {
        let (this, person) = (this.clone(), person.clone());
        ui::button(id, tone, cx)
            .small()
            .children(glyph.map(|g| gpui_kit::component::Icon::new(g).size(ui::ICON_MD)))
            .child(label)
            .on_click(move |_, _, cx| {
                let person = person.clone();
                this.update(cx, |this, cx| run(this, person, cx)).ok();
            })
    };
    let state = |glyph: Glyph, label: &'static str| {
        ui::row().gap_1p5().text_size(ui::TEXT_SM).font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child(gpui_kit::component::Icon::new(glyph).size(ui::ICON_LG).text_color(palette::positive_ink(cx))).child(label)
    };
    let block = || act("dm-start-block", Tone::Ghost, "Block", None, |this, user, cx| this.ask_block(user, cx));
    let row = match start.next {
        Next::Nothing => return None,
        Next::Friends => row.child(state(Glyph::Users, "Friends")),
        Next::Befriend => row.child(act("dm-start-add", Tone::Primary, "Add friend", Some(Glyph::GroupPlus), |this, user, cx| this.befriend(user, cx))).child(block()),
        Next::Accept => row.child(act("dm-start-accept", Tone::Primary, "Accept friend request", Some(Glyph::GroupPlus), |this, user, cx| this.befriend(user, cx))).child(block()),
        Next::Pending => row
            .child(state(Glyph::CheckCircle, "Friend request sent"))
            .child(act("dm-start-cancel", Tone::Ghost, "Cancel request", None, |this, user, cx| this.drop_relation(&user.id, cx)))
            .child(block()),
        Next::Blocked => row.child(act("dm-start-unblock", Tone::Ghost, "Unblock", None, |this, user, cx| this.drop_relation(&user.id, cx))),
        Next::BlockOnly => row.child(block()),
    };
    Some(row.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leads_only_a_dm_whose_start_is_loaded() {
        assert!(shows(ChannelKind::Dm, false));
        assert!(!shows(ChannelKind::Dm, true));
        assert!(!shows(ChannelKind::Text, false));
    }

    #[test]
    fn offers_the_next_step_by_relation() {
        assert_eq!(next_step(None, false), Next::Befriend);
        assert_eq!(next_step(None, true), Next::BlockOnly);
        assert_eq!(next_step(Some(Relation::Incoming), false), Next::Accept);
        assert_eq!(next_step(Some(Relation::Outgoing), false), Next::Pending);
        assert_eq!(next_step(Some(Relation::Friend), false), Next::Friends);
        assert_eq!(next_step(Some(Relation::Blocked), false), Next::Blocked);
    }
}
