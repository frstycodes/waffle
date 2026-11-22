// Group DMs (docs/discord-group-dms-re.md, design/explore-dm-social.html §1): Discord's one recipient picker
// in its three flavours (New Message, Add to DM, Invite to Group DM), Edit Group, Leave Group, and the
// owner's member actions. Friends only; 10 members including you, drawn as a ten-segment meter.
//
// GAP: no group invite links ("Or, send an invite link"), no icon in the new-group form, no non-friends
// through invites, and no duplicate-group guard ("Confirm New Group").
use std::path::PathBuf;

use gpui_kit::{
    AnyElement, Context, Entity, Focusable, FontWeight, SharedString, Subscription, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState as SearchState},
    },
    div, img,
    prelude::*,
    px,
};
use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Waffle,
    chat::emphasised,
    discord::{Channel, Relation, User},
    icons::Glyph,
    layout::avatar_with_status,
    overlays::backdrop,
    palette,
    ui::{self, Check, Tone},
};

// Members a group DM may hold, you included (Discord's getGroupDMRecipientLimit without Nitro or staff).
pub(crate) const LIMIT: usize = 10;
const NAME_MAX: usize = 100;
const ICON_MAX_BYTES: u64 = 8 * 1024 * 1024;
const WIDTH: f32 = 440.;
const LIST_HEIGHT: f32 = 288.;
const ROW_AVATAR: f32 = 32.;
const EDIT_ICON: f32 = 80.;

pub(crate) enum GroupDialog {
    Picker(Box<Picker>),
    Edit(Box<EditGroup>),
    Leave(LeaveGroup),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flavour {
    // From the Friends page: one pick opens a DM, more make a group.
    New,
    // From a 1:1 DM: a new group with them and the picks; the DM stays as it is.
    AddToDm,
    // From a group: the picks join it.
    Invite,
}

pub(crate) struct Picker {
    flavour: Flavour,
    // The DM or group it was opened from; None for a new message.
    channel: Option<Channel>,
    search: Entity<SearchState>,
    name: Entity<SearchState>,
    picked: Vec<User>,
    error: Option<SharedString>,
    busy: bool,
    _subscriptions: [Subscription; 2],
}

pub(crate) struct EditGroup {
    channel_id: String,
    name: Entity<SearchState>,
    // The custom name as it was; empty when the group shows its members' names.
    was_named: String,
    icon: IconEdit,
    error: Option<SharedString>,
    busy: bool,
    _subscription: Subscription,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum IconEdit {
    Keep,
    Set { path: PathBuf, uri: String },
    Remove,
}

pub(crate) struct LeaveGroup {
    channel_id: String,
    name: String,
    silent: bool,
    // Who takes over when you, the owner, leave.
    next_owner: Option<String>,
}

impl Waffle {
    // The open channel when it is a group DM.
    pub(crate) fn open_group(&self) -> Option<&Channel> {
        self.channel.as_ref().filter(|c| c.raw_type == 3)
    }

    pub(crate) fn group_owner_is_me(&self, channel: &Channel) -> bool {
        channel.props.owner_id.as_deref().is_some_and(|owner| owner == self.my_id())
    }

    // The picker for where it was opened: nothing (New Message), a 1:1 DM (Add to DM) or a group (Invite).
    pub(crate) fn open_group_picker(&mut self, channel: Option<Channel>, window: &mut Window, cx: &mut Context<Self>) {
        let flavour = match &channel {
            None => Flavour::New,
            Some(c) if c.raw_type == 3 => Flavour::Invite,
            Some(_) => Flavour::AddToDm,
        };
        let search = cx.new(|cx| SearchState::new(window, cx).placeholder("Search"));
        let name = cx.new(|cx| SearchState::new(window, cx).validate(|text, _| text.chars().count() <= NAME_MAX));
        let _subscriptions = [
            cx.subscribe_in(&search, window, |_, _, _: &InputEvent, _, cx| cx.notify()),
            cx.subscribe_in(&name, window, |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_group_picker(window, cx),
                _ => cx.notify(),
            }),
        ];
        search.update(cx, |state, cx| state.focus(window, cx));
        self.menu = None;
        self.group_dialog = Some(GroupDialog::Picker(Box::new(Picker { flavour, channel, search, name, picked: Vec::new(), error: None, busy: false, _subscriptions })));
        cx.notify();
    }

    fn close_group_dialog(&mut self, cx: &mut Context<Self>) {
        self.group_dialog = None;
        cx.notify();
    }

    fn picker(&self) -> Option<&Picker> {
        match &self.group_dialog {
            Some(GroupDialog::Picker(p)) => Some(p),
            _ => None,
        }
    }

    fn picker_mut(&mut self) -> Option<&mut Picker> {
        match &mut self.group_dialog {
            Some(GroupDialog::Picker(p)) => Some(p),
            _ => None,
        }
    }

    // You and everyone already in the DM or group it adds to.
    fn picker_members(picker: &Picker) -> usize {
        1 + picker.channel.as_ref().map_or(0, |c| c.recipients.len())
    }

    // Friends whose names hold the query, by name.
    fn pickable_friends(&self, query: &str) -> Vec<User> {
        let query = query.to_lowercase();
        let mut friends: Vec<User> = self.relationships.iter().filter(|r| r.kind == Relation::Friend && r.user.name.to_lowercase().contains(&query)).map(|r| r.user.clone()).collect();
        friends.sort_by_key(|u| u.name.to_lowercase());
        friends
    }

    pub(crate) fn toggle_group_pick(&mut self, user: User, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker_mut() else { return };
        let left = remaining(Self::picker_members(picker), picker.picked.len());
        match picker.picked.iter().position(|u| u.id == user.id) {
            Some(at) => _ = picker.picked.remove(at),
            None if left > 0 => picker.picked.push(user),
            None => return,
        }
        picker.error = None;
        let (search, name) = (picker.search.clone(), picker.name.clone());
        let placeholder = if picker.picked.is_empty() { "Search" } else { "" };
        let auto = picker.picked.iter().map(|u| u.name.as_str()).collect::<Vec<_>>().join(", ");
        search.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder(placeholder, window, cx);
            state.focus(window, cx);
        });
        name.update(cx, |state, cx| state.set_placeholder(auto, window, cx));
        cx.notify();
    }

    fn submit_group_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.picker() else { return };
        if picker.picked.is_empty() || picker.busy {
            return;
        }
        let picks: Vec<String> = picker.picked.iter().map(|u| u.id.clone()).collect();
        let name = picker.name.read(cx).value().trim().to_owned();
        match (picker.flavour, picker.channel.clone()) {
            (Flavour::New, _) if picks.len() == 1 => {
                self.group_dialog = None;
                self.message_user(picks[0].clone(), window, cx);
            }
            (Flavour::Invite, Some(group)) => self.add_to_group(group.id, picks, cx),
            (flavour, dm) => {
                let mut recipients: Vec<String> = dm.filter(|_| flavour == Flavour::AddToDm).and_then(|d| d.recipient).map(|r| r.id).into_iter().collect();
                recipients.extend(picks);
                let name = (flavour == Flavour::New).then_some(name).filter(|n| !n.is_empty());
                self.create_group(recipients, name, window, cx);
            }
        }
        cx.notify();
    }

    fn picker_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker_mut() {
            picker.busy = false;
            picker.error = Some(error.into());
        }
        cx.notify();
    }

    // A new group, named if a name was given, then opened.
    fn create_group(&mut self, recipients: Vec<String>, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if let Some(picker) = self.picker_mut() {
            picker.busy = true;
        }
        cx.spawn_in(window, async move |this, cx| {
            let created = client.open_private(&recipients).await;
            let mut group = match created {
                Ok(group) => group,
                Err(error) => {
                    this.update(cx, |this, cx| this.picker_failed(error, cx)).ok();
                    return;
                }
            };
            // The group exists by now: a rename that fails leaves it under its members' names.
            if let Some(name) = name
                && client.call(Method::PATCH, format!("/channels/{}", group.id), Some(json!({ "name": name }))).await.is_ok()
            {
                group.name = name;
            }
            this.update_in(cx, |this, window, cx| {
                this.group_dialog = None;
                this.upsert_dm(group.clone(), cx);
                this.guild_id = None;
                this.open_channel(group, window, cx);
            })
            .ok();
        })
        .detach();
    }

    // One PUT per person, in turn, as Discord's client does; the first refusal stops the rest and shows.
    fn add_to_group(&mut self, channel_id: String, picks: Vec<String>, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        if let Some(picker) = self.picker_mut() {
            picker.busy = true;
        }
        cx.spawn(async move |this, cx| {
            for user_id in picks {
                if let Err(error) = client.call(Method::PUT, format!("/channels/{channel_id}/recipients/{user_id}"), None).await {
                    this.update(cx, |this, cx| this.picker_failed(error, cx)).ok();
                    return;
                }
            }
            this.update(cx, |this, cx| this.close_group_dialog(cx)).ok();
        })
        .detach();
    }

    // Edit Group: the name (its members' names as the placeholder) and the icon; Save sends only what changed.
    pub(crate) fn open_edit_group(&mut self, channel_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group) = self.dms.iter().chain(self.channel.iter()).find(|c| c.id == channel_id) else { return };
        let auto = auto_name(group);
        // GAP: a channel keeps only its shown name, so a custom name spelled exactly like the auto name reads as none.
        let was_named = if group.name == auto { String::new() } else { group.name.clone() };
        let name = cx.new(|cx| SearchState::new(window, cx).placeholder(auto).validate(|text, _| text.chars().count() <= NAME_MAX));
        name.update(cx, |state, cx| {
            state.set_value(was_named.clone(), window, cx);
            state.focus(window, cx);
        });
        let _subscription = cx.subscribe_in(&name, window, |this, _, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { .. } => this.save_edit_group(cx),
            _ => cx.notify(),
        });
        self.menu = None;
        self.group_dialog = Some(GroupDialog::Edit(Box::new(EditGroup { channel_id, name, was_named, icon: IconEdit::Keep, error: None, busy: false, _subscription })));
        cx.notify();
    }

    fn edit_mut(&mut self) -> Option<&mut EditGroup> {
        match &mut self.group_dialog {
            Some(GroupDialog::Edit(e)) => Some(e),
            _ => None,
        }
    }

    fn pick_group_icon(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let picked = icon_data(&path);
            this.update(cx, |this, cx| {
                let Some(edit) = this.edit_mut() else { return };
                match picked {
                    Ok(uri) => (edit.icon, edit.error) = (IconEdit::Set { path, uri }, None),
                    Err(error) => edit.error = Some(error.into()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_edit_group(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.edit_mut() else { return };
        let Some(patch) = edit_patch(&edit.was_named, edit.name.read(cx).value().trim(), &edit.icon).filter(|_| !edit.busy) else { return };
        edit.busy = true;
        let id = edit.channel_id.clone();
        let Some(client) = self.client.clone() else { return };
        cx.spawn(async move |this, cx| {
            let result = client.call(Method::PATCH, format!("/channels/{id}"), Some(patch)).await;
            this.update(cx, |this, cx| match result {
                Ok(_) => this.close_group_dialog(cx),
                Err(error) => {
                    if let Some(edit) = this.edit_mut() {
                        (edit.busy, edit.error) = (false, Some(error.into()));
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn ask_leave_group(&mut self, channel_id: String, cx: &mut Context<Self>) {
        let Some(group) = self.dms.iter().chain(self.channel.iter()).find(|c| c.id == channel_id) else { return };
        // Not verified: which member Discord makes owner when the owner leaves; this names the first one listed.
        let next_owner = self.group_owner_is_me(group).then(|| group.recipients.first().map(|u| u.name.clone())).flatten();
        self.menu = None;
        self.group_dialog = Some(GroupDialog::Leave(LeaveGroup { channel_id, name: group.name.clone(), silent: false, next_owner }));
        cx.notify();
    }

    // Gone from the list at once, as Discord dispatches its CHANNEL_DELETE before the request.
    fn leave_group(&mut self, cx: &mut Context<Self>) {
        let Some(GroupDialog::Leave(leave)) = self.group_dialog.take() else { return };
        self.dms.retain(|d| d.id != leave.channel_id);
        if self.channel.as_ref().is_some_and(|c| c.id == leave.channel_id) {
            self.open_home(cx);
        }
        self.rest(Method::DELETE, format!("/channels/{}?silent={}", leave.channel_id, leave.silent), None, |_, _| {}, cx);
    }

    // The member leaves through the gateway's CHANNEL_RECIPIENT_REMOVE; Discord asks nothing first.
    pub(crate) fn remove_from_group(&mut self, channel_id: String, user_id: String, cx: &mut Context<Self>) {
        self.rest(Method::DELETE, format!("/channels/{channel_id}/recipients/{user_id}"), None, |_, _| {}, cx);
    }

    pub(crate) fn ask_make_group_owner(&mut self, channel_id: String, user: User, cx: &mut Context<Self>) {
        self.confirm("Transfer Group Ownership", format!("Transfer ownership of this group to {}?", user.name), "Confirm", move |this, _, cx| {
            let (id, owner) = (channel_id.clone(), user.id.clone());
            this.rest(Method::PATCH, format!("/channels/{id}"), Some(json!({ "owner": owner })), move |this, _| this.set_group_owner(&id, &owner), cx);
        }, cx);
    }

    fn set_group_owner(&mut self, channel_id: &str, owner: &str) {
        for group in self.dms.iter_mut().chain(self.channel.as_mut()).filter(|c| c.id == channel_id) {
            group.props.owner_id = Some(owner.to_owned());
        }
    }

    pub(crate) fn group_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (id, body) = match self.group_dialog.as_ref()? {
            GroupDialog::Picker(picker) => ("group-picker", self.picker_body(picker, window, cx)),
            GroupDialog::Edit(edit) => ("group-edit", self.edit_body(edit, cx)),
            GroupDialog::Leave(leave) => ("group-leave", leave_body(leave, window, cx)),
        };
        Some(ui::modal(backdrop("group-backdrop", cx), ui::dialog(id, WIDTH, body, window, cx), cx))
    }

    fn picker_body(&self, picker: &Picker, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::Div {
        let members = Self::picker_members(picker);
        let picked = picker.picked.len();
        let friends = self.pickable_friends("");
        let peer = picker.channel.as_ref().and_then(|c| c.recipient.clone()).filter(|_| picker.flavour == Flavour::AddToDm);
        let stranger = peer.clone().filter(|p| self.relation(&p.id) != Some(Relation::Friend));
        let title = picker_title(picker.flavour, members, stranger.is_some());
        let subtitle = picker_subtitle(picker.flavour, members, picked, friends.len(), stranger.as_ref().map(|s| s.name.as_str()));
        let body = ui::dialog_body().child(dialog_head(title, Some(subtitle), cx));
        if picker.flavour == Flavour::Invite && members >= LIMIT {
            let close = ui::button("group-picker-done", Tone::Secondary, cx).child("Close").on_click(cx.listener(|this, _, _, cx| this.close_group_dialog(cx)));
            return body.child(member_meter(members, 0, cx)).child(div().flex().justify_end().child(close));
        }
        if let Some(stranger) = stranger {
            return body.child(self.stranger_footer(stranger, cx));
        }
        let query = picker.search.read(cx).value().trim().to_owned();
        let shown = self.pickable_friends(&query);
        let left = remaining(members, picked);
        let in_already: Vec<&str> = picker.channel.iter().flat_map(|c| c.recipients.iter().map(|u| u.id.as_str())).collect();
        let rows: Vec<AnyElement> = shown
            .iter()
            .map(|user| {
                let state = match () {
                    _ if in_already.contains(&user.id.as_str()) => RowState::Member,
                    _ if picker.picked.iter().any(|p| p.id == user.id) => RowState::Picked,
                    _ if left == 0 => RowState::Full,
                    _ => RowState::Open,
                };
                self.friend_pick_row(user, state, window, cx)
            })
            .collect();
        let list = match (rows.is_empty(), friends.is_empty()) {
            (false, _) => div().id("group-picker-list").flex().flex_col().gap(px(2.)).max_h(px(LIST_HEIGHT)).overflow_y_scroll().children(rows).into_any_element(),
            (true, false) => div().py(px(16.)).text_color(palette::muted(cx)).child("No results found.").into_any_element(),
            (true, true) => div().into_any_element(),
        };
        let add = (picker.flavour == Flavour::Invite).then(|| {
            ui::button("group-picker-add", Tone::Primary, cx)
                .h(px(40.))
                .disabled(picked == 0)
                .loading(picker.busy.then_some("Adding"))
                .child(create_label(picker.flavour, picked))
                .on_click(cx.listener(|this, _, window, cx| this.submit_group_picker(window, cx)))
        });
        let to = div().flex().items_start().gap(px(8.)).child(div().flex_1().min_w_0().child(self.to_field(picker, cx))).children(add);
        let name = (picker.flavour == Flavour::New && picked > 1).then(|| {
            let focus = picker.name.read(cx).focus_handle(cx);
            ui::field("Group Name (optional)", cx).child(ui::text_field("group-picker-name", Input::new(&picker.name).appearance(false), &focus))
        });
        let footer = (picker.flavour != Flavour::Invite).then(|| {
            let create = ui::button("group-picker-create", Tone::Primary, cx)
                .disabled(picked == 0)
                .loading(picker.busy.then_some("Creating"))
                .child(create_label(picker.flavour, picked))
                .on_click(cx.listener(|this, _, window, cx| this.submit_group_picker(window, cx)));
            div().flex().justify_end().gap(px(8.)).child(cancel_button("group-picker-cancel", cx)).child(create)
        });
        body.child(to)
            .child(member_meter(members, picked, cx))
            .child(list)
            .children(name)
            .children(picker.error.clone().map(|e| ui::field_error(e, cx)))
            .children(footer)
    }

    // "To:", a removable chip per pick, then the search.
    fn to_field(&self, picker: &Picker, cx: &mut Context<Self>) -> gpui_kit::Div {
        let chips: Vec<AnyElement> = picker
            .picked
            .iter()
            .map(|user| {
                let chip_user = user.clone();
                ui::chip(SharedString::from(format!("group-chip-{}", user.id)), None, user.name.clone(), true, cx)
                    .child(Icon::new(Glyph::X).size(px(14.)))
                    .on_click(cx.listener(move |this, _, window, cx| this.toggle_group_pick(chip_user.clone(), window, cx)))
                    .into_any_element()
            })
            .collect();
        ui::field_frame(cx)
            .h_auto()
            .min_h(px(40.))
            .py(px(4.))
            .flex_wrap()
            .gap(px(6.))
            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::muted(cx)).child("To:"))
            .children(chips)
            .child(div().flex_1().min_w(px(96.)).child(Input::new(&picker.search).appearance(false)))
    }

    fn friend_pick_row(&self, user: &User, state: RowState, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let surface = palette::surface(palette::Level::Raised, cx);
        let check = if matches!(state, RowState::Picked | RowState::Member) { Check::On } else { Check::Off };
        let detail = (state == RowState::Member).then(|| div().text_size(px(12.)).text_color(palette::muted(cx)).child("Already in the group"));
        let text = div().flex_1().min_w_0().flex().flex_col().child(div().truncate().text_color(palette::strong(cx)).child(user.name.clone())).children(detail);
        let row = ui::menu_row(SharedString::from(format!("group-pick-{}", user.id)), cx)
            .disabled(matches!(state, RowState::Member | RowState::Full))
            .child(avatar_with_status(&user.avatar, ROW_AVATAR, self.presences.get(&user.id).map(|p| p.mark()), surface))
            .child(text)
            .child(ui::check_live(SharedString::from(format!("group-pick-check-{}", user.id)), check, window, cx));
        if matches!(state, RowState::Member | RowState::Full) {
            return row.into_any_element();
        }
        let user = user.clone();
        row.on_click(cx.listener(move |this, _, window, cx| this.toggle_group_pick(user.clone(), window, cx))).into_any_element()
    }

    // A 1:1 DM with someone who isn't a friend: no group until they are.
    fn stranger_footer(&self, stranger: User, cx: &mut Context<Self>) -> gpui_kit::Div {
        let sent = self.relation(&stranger.id) == Some(Relation::Outgoing);
        let request = ui::button("group-picker-befriend", Tone::Primary, cx)
            .disabled(sent)
            .child(if sent { "Friend Request Sent" } else { "Send Friend Request" })
            .on_click(cx.listener(move |this, _, _, cx| this.befriend(stranger.clone(), cx)));
        div().flex().justify_end().gap(px(8.)).child(cancel_button("group-picker-cancel", cx)).child(request)
    }

    fn edit_body(&self, edit: &EditGroup, cx: &mut Context<Self>) -> gpui_kit::Div {
        let group = self.dms.iter().chain(self.channel.iter()).find(|c| c.id == edit.channel_id);
        let has_icon = match &edit.icon {
            IconEdit::Keep => group.is_some_and(|g| g.icon.is_some()),
            IconEdit::Set { .. } => true,
            IconEdit::Remove => false,
        };
        let surface = palette::surface(palette::Level::Raised, cx);
        let face: AnyElement = match (&edit.icon, group) {
            (IconEdit::Set { path, .. }, _) => img(path.clone()).size(px(EDIT_ICON)).rounded_full().into_any_element(),
            (IconEdit::Remove, Some(group)) => ui::dm_face(&Channel { icon: None, ..group.clone() }.face(), EDIT_ICON, surface),
            (IconEdit::Keep, Some(group)) => ui::dm_face(&group.face(), EDIT_ICON, surface),
            (_, None) => div().size(px(EDIT_ICON)).into_any_element(),
        };
        let pen = div()
            .absolute()
            .right_0()
            .bottom_0()
            .size(px(28.))
            .rounded_full()
            .border_2()
            .border_color(surface)
            .bg(palette::surface(palette::Level::Overlay, cx))
            .text_color(palette::strong(cx))
            .flex()
            .items_center()
            .justify_center()
            .child(Icon::new(Glyph::Pen).size(px(14.)));
        let icon = div()
            .id("group-edit-icon")
            .relative()
            .flex_none()
            .cursor_pointer()
            .child(face)
            .child(pen)
            .tooltip(|window, cx| ui::tip("Edit Group Icon", window, cx))
            .on_click(cx.listener(|this, _, _, cx| this.pick_group_icon(cx)));
        let remove = has_icon.then(|| {
            ui::button("group-edit-remove-icon", Tone::Ghost, cx).small().danger(cx).child("Remove Icon").on_click(cx.listener(|this, _, _, cx| {
                if let Some(edit) = this.edit_mut() {
                    edit.icon = IconEdit::Remove;
                }
                cx.notify();
            }))
        });
        let typed = edit.name.read(cx).value().trim().to_owned();
        let focus = edit.name.read(cx).focus_handle(cx);
        let name = ui::field("Group Name", cx)
            .child(ui::text_field("group-edit-name", Input::new(&edit.name).appearance(false), &focus))
            .child(ui::field_hint("", Some(format!("{} / {NAME_MAX}", typed.chars().count()).into()), cx));
        let changed = edit_patch(&edit.was_named, &typed, &edit.icon).is_some();
        let save = ui::button("group-edit-save", Tone::Primary, cx)
            .disabled(!changed)
            .loading(edit.busy.then_some("Saving"))
            .child("Save")
            .on_click(cx.listener(|this, _, _, cx| this.save_edit_group(cx)));
        ui::dialog_body()
            .child(dialog_head("Edit Group", None, cx))
            .child(div().flex().items_center().gap(px(16.)).child(icon).children(remove))
            .child(name)
            .children(edit.error.clone().map(|e| ui::field_error(e, cx)))
            .child(div().flex().justify_end().gap(px(8.)).child(cancel_button("group-edit-cancel", cx)).child(save))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowState {
    Open,
    Picked,
    // Already in the group: ticked, not pickable.
    Member,
    // Nothing picked here and no room left.
    Full,
}

fn leave_body(leave: &LeaveGroup, window: &mut Window, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    let (strong, muted) = (palette::strong(cx), palette::muted(cx));
    let name = leave.name.clone();
    let warning = [("Are you sure you want to leave ".to_owned(), false), (name.clone(), true), ("? You won't be able to rejoin this group unless you are re-invited.".to_owned(), false)];
    let owner = leave.next_owner.clone().map(|next| {
        let line = [("You own this group. ".to_owned(), false), (next, true), (" becomes the owner.".to_owned(), false)];
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_color(muted)
            .child(div().flex_none().text_color(palette::warning_ink(cx)).child(Icon::new(Glyph::Crown).size(px(16.))))
            .child(emphasised(&line, strong))
    });
    let silent = ui::pick_row("group-leave-silent", ui::check_live("group-leave-silent-check", if leave.silent { Check::On } else { Check::Off }, window, cx), "Leave without notifying other members", None, cx).on_click(
        cx.listener(|this, _, _, cx| {
            if let Some(GroupDialog::Leave(leave)) = this.group_dialog.as_mut() {
                leave.silent = !leave.silent;
            }
            cx.notify();
        }),
    );
    let confirm = ui::button("group-leave-confirm", Tone::Danger, cx).child("Leave Group").on_click(cx.listener(|this, _, _, cx| this.leave_group(cx)));
    ui::dialog_body()
        .child(dialog_head(format!("Leave '{name}'"), None, cx))
        .child(div().text_color(muted).child(emphasised(&warning, strong)))
        .children(owner)
        .child(silent)
        .child(div().flex().justify_end().gap(px(8.)).child(cancel_button("group-leave-cancel", cx)).child(confirm))
}

fn dialog_head(title: impl Into<SharedString>, subtitle: Option<SharedString>, cx: &mut Context<Waffle>) -> gpui_kit::Div {
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(title.into()))
        .children(subtitle.map(|s| div().text_color(palette::muted(cx)).child(s)));
    div().flex().items_start().gap_3().child(text).child(ui::icon_button("group-dialog-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_group_dialog(cx))))
}

fn cancel_button(id: &'static str, cx: &mut Context<Waffle>) -> ui::Control {
    ui::button(id, Tone::Secondary, cx).child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_group_dialog(cx)))
}

// Ten segments: those already in (you first) in grey, the picks in the accent, then "N of 10".
fn member_meter(members: usize, picked: usize, cx: &Context<Waffle>) -> gpui_kit::Div {
    let total = members + picked;
    let (held, pick, empty) = (palette::muted(cx), palette::accent(), palette::line_strong(cx));
    let segments = (0..LIMIT).map(|i| {
        let fill = match i {
            _ if i < members => held,
            _ if i < total => pick,
            _ => empty,
        };
        div().flex_1().h(px(4.)).rounded(px(2.)).bg(fill)
    });
    let ink = if total >= LIMIT { palette::danger_ink(cx) } else { palette::muted(cx) };
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(div().flex_1().flex().gap(px(3.)).children(segments))
        .child(div().flex_none().text_size(px(12.)).text_color(ink).child(format!("{} of {LIMIT}", total.min(LIMIT))))
}

// A group's name when it has none of its own: its members', as Discord shows it.
fn auto_name(group: &Channel) -> String {
    group.recipients.iter().map(|u| u.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn icon_data(path: &std::path::Path) -> Result<String, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("Couldn't read that file: {e}"))?.len();
    if size > ICON_MAX_BYTES {
        return Err("Icons can be up to 8 MB".into());
    }
    crate::settings::profile::image_data_uri(path).map(|(uri, _)| uri)
}

pub(crate) fn remaining(members: usize, picked: usize) -> usize {
    LIMIT.saturating_sub(members + picked)
}

// Discord's getTitle: `stranger` is a 1:1 DM peer who isn't a friend.
pub(crate) fn picker_title(flavour: Flavour, members: usize, stranger: bool) -> &'static str {
    match flavour {
        Flavour::AddToDm if stranger => "Add Friends to DM",
        Flavour::Invite if members >= LIMIT => "The party is full!",
        Flavour::New => "New Message",
        Flavour::Invite => "Invite to Group DM",
        Flavour::AddToDm => "Add to DM",
    }
}

// Discord's getSubtitle, counting the picks so far.
pub(crate) fn picker_subtitle(flavour: Flavour, members: usize, picked: usize, friends: usize, stranger: Option<&str>) -> SharedString {
    if let Some(name) = stranger {
        return format!("You need to be friends with {name} to start a group DM.").into();
    }
    if friends == 0 {
        return "Add friends to start messaging.".into();
    }
    match (flavour, remaining(members, picked)) {
        (_, 0) => format!("This group has a {LIMIT} member limit.").into(),
        (Flavour::Invite, 1) => "You can add 1 more person.".into(),
        (Flavour::Invite, left) => format!("You can add {left} more people.").into(),
        _ => format!("Group DMs can have up to {LIMIT} members.").into(),
    }
}

pub(crate) fn create_label(flavour: Flavour, picked: usize) -> &'static str {
    match (flavour, picked) {
        (Flavour::Invite, _) => "Add",
        (Flavour::New, 0 | 1) => "Create Message",
        _ => "Create Group Message",
    }
}

// The PATCH body for what changed, None when nothing did.
pub(crate) fn edit_patch(was_named: &str, name: &str, icon: &IconEdit) -> Option<Value> {
    let mut patch = serde_json::Map::new();
    if name != was_named {
        // Not verified: that an empty name clears a custom one (Discord's "removed the custom group name").
        patch.insert("name".into(), json!(name));
    }
    match icon {
        IconEdit::Keep => {}
        IconEdit::Set { uri, .. } => _ = patch.insert("icon".into(), json!(uri)),
        IconEdit::Remove => _ = patch.insert("icon".into(), Value::Null),
    }
    (!patch.is_empty()).then_some(Value::Object(patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_counts_you_the_members_and_the_picks() {
        assert_eq!(remaining(1, 0), 9);
        assert_eq!(remaining(4, 3), 3);
        assert_eq!(remaining(10, 0), 0);
        assert_eq!(remaining(9, 2), 0);
    }

    #[test]
    fn titles_follow_discords_get_title() {
        assert_eq!(picker_title(Flavour::New, 1, false), "New Message");
        assert_eq!(picker_title(Flavour::AddToDm, 2, false), "Add to DM");
        assert_eq!(picker_title(Flavour::AddToDm, 2, true), "Add Friends to DM");
        assert_eq!(picker_title(Flavour::Invite, 5, false), "Invite to Group DM");
        assert_eq!(picker_title(Flavour::Invite, 10, false), "The party is full!");
    }

    #[test]
    fn subtitles_count_what_is_left() {
        assert_eq!(picker_subtitle(Flavour::New, 1, 2, 5, None), "Group DMs can have up to 10 members.");
        assert_eq!(picker_subtitle(Flavour::New, 1, 9, 12, None), "This group has a 10 member limit.");
        assert_eq!(picker_subtitle(Flavour::Invite, 5, 0, 5, None), "You can add 5 more people.");
        assert_eq!(picker_subtitle(Flavour::Invite, 8, 1, 5, None), "You can add 1 more person.");
        assert_eq!(picker_subtitle(Flavour::Invite, 10, 0, 5, None), "This group has a 10 member limit.");
        assert_eq!(picker_subtitle(Flavour::New, 1, 0, 0, None), "Add friends to start messaging.");
        assert_eq!(picker_subtitle(Flavour::AddToDm, 2, 0, 3, Some("ferris")), "You need to be friends with ferris to start a group DM.");
    }

    #[test]
    fn one_pick_makes_a_message_more_make_a_group() {
        assert_eq!(create_label(Flavour::New, 1), "Create Message");
        assert_eq!(create_label(Flavour::New, 2), "Create Group Message");
        assert_eq!(create_label(Flavour::AddToDm, 1), "Create Group Message");
        assert_eq!(create_label(Flavour::Invite, 3), "Add");
    }

    #[test]
    fn edit_sends_only_what_changed() {
        assert_eq!(edit_patch("road trip", "road trip", &IconEdit::Keep), None);
        assert_eq!(edit_patch("", "road trip", &IconEdit::Keep), Some(json!({ "name": "road trip" })));
        assert_eq!(edit_patch("road trip", "road trip", &IconEdit::Remove), Some(json!({ "icon": null })));
        let set = IconEdit::Set { path: "a.png".into(), uri: "data:image/png;base64,AA==".into() };
        assert_eq!(edit_patch("a", "b", &set), Some(json!({ "name": "b", "icon": "data:image/png;base64,AA==" })));
    }
}
