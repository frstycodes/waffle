// Settings › Profiles, Discord's two tabs: your main profile, and how you show on each server. Fields edit a
// local draft that the preview (the real popout) draws as you type; nothing reaches Discord until Save
// changes. While something differs the bar names it, and a switch of tab or server is refused with a shake.
// What Discord lets only Nitro set shares one row that opens Discord.
use std::{cell::Cell, rc::Rc, time::Duration};

use gpui_kit::{
    Animation, AnimationExt, AnyElement, App, AppContext, Context, Div, Entity, Focusable, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window,
    component::{
        Icon,
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
    },
    div,
    prelude::FluentBuilder,
    px, rgb,
};
use serde_json::Value;

use super::{
    Page, Section,
    profile_draft::{self as draft, AvatarChoice, Field, MainProfile, RecentAvatar, Request, ServerProfile, TagGuild},
    profile_preview::{PreviewAvatar, profile_preview},
};
use crate::{
    Waffle,
    discord::{Client, GuildTag, perm},
    icons::Glyph,
    motion,
    palette::{self, Level},
    ui::{self, Notice, Squircle, Tone},
};

const DISCORD: &str = "https://discord.com/app";
const UNDO_FOR: Duration = Duration::from_secs(6);
const SHAKE: Duration = Duration::from_millis(360);
const SLOT: f32 = 56.;
// The page holds the editor and the preview side by side, so it runs wider than other settings pages.
pub(super) const PAGE_WIDTH: f32 = 960.;
pub(super) const MAIN_GROUP: &str = "Main Profile";
pub(super) const SERVER_GROUP: &str = "Per-server Profiles";
// Discord's role colour presets, offered as banner colours to people without Nitro.
const BANNER_COLOURS: [u32; 10] = [0x1ABC9C, 0x2ECC71, 0x3498DB, 0x9B59B6, 0xE91E63, 0xF1C40F, 0xE67E22, 0xE74C3C, 0x95A5A6, 0x607D8B];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ProfileTab {
    Main,
    Server,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Tag,
    Server,
}

pub(crate) struct ProfileEditor {
    tab: ProfileTab,
    // None until Discord answers.
    saved: Option<MainProfile>,
    load_error: Option<String>,
    // Your popout as Discord last sent it; the preview keeps its banner, theme and badges.
    shown: crate::discord::Profile,
    avatar: AvatarChoice,
    accent: Option<u32>,
    tag_guild: Option<String>,
    name: Entity<InputState>,
    pronouns: Entity<InputState>,
    bio: Entity<TextareaState>,
    guild_id: Option<String>,
    server_saved: Option<ServerProfile>,
    nick: Entity<InputState>,
    server_pronouns: Entity<InputState>,
    recent: Vec<RecentAvatar>,
    removing: Option<Removal>,
    tags: Vec<TagGuild>,
    menu: Option<Menu>,
    saving: bool,
    error: Option<String>,
    // Bumped by each refused switch: the bar shakes once per bump.
    shakes: u32,
}

// A recent avatar taken off the list, deleted on Discord once its Undo runs out.
struct Removal {
    avatar: RecentAvatar,
    index: usize,
    undone: Rc<Cell<bool>>,
}

impl ProfileEditor {
    fn new(window: &mut Window, cx: &mut Context<Waffle>) -> Self {
        let bio = cx.new(|cx| TextareaState::new(window, cx).placeholder("Tell everyone a bit about yourself").auto_grow(3, 8));
        // A textarea takes no validator, so a paste past the limit is cut back to it.
        cx.subscribe_in(&bio, window, |_, bio, _: &InputEvent, window, cx| {
            let text = bio.read(cx).value();
            if text.chars().count() > draft::BIO_MAX {
                let kept: String = text.chars().take(draft::BIO_MAX).collect();
                bio.update(cx, |s, cx| s.set_value(kept, window, cx));
            }
            cx.notify();
        })
        .detach();
        Self {
            tab: ProfileTab::Main,
            saved: None,
            load_error: None,
            shown: Default::default(),
            avatar: AvatarChoice::Current,
            accent: None,
            tag_guild: None,
            name: limited_input(draft::DISPLAY_NAME_MAX, "", window, cx),
            pronouns: limited_input(draft::PRONOUNS_MAX, "Add your pronouns", window, cx),
            bio,
            guild_id: None,
            server_saved: None,
            nick: limited_input(draft::NICK_MAX, "", window, cx),
            server_pronouns: limited_input(draft::PRONOUNS_MAX, "Uses your main pronouns", window, cx),
            recent: Vec::new(),
            removing: None,
            tags: Vec::new(),
            menu: None,
            saving: false,
            error: None,
            shakes: 0,
        }
    }

    fn main_draft(&self, cx: &App) -> Option<MainProfile> {
        self.saved.as_ref()?;
        Some(MainProfile {
            display_name: self.name.read(cx).value().to_string(),
            pronouns: self.pronouns.read(cx).value().to_string(),
            bio: self.bio.read(cx).value().to_string(),
            accent: self.accent,
            tag_guild: self.tag_guild.clone(),
            avatar: self.avatar.clone(),
        })
    }

    fn server_draft(&self, cx: &App) -> Option<ServerProfile> {
        self.server_saved.as_ref()?;
        Some(ServerProfile { nick: self.nick.read(cx).value().to_string(), pronouns: self.server_pronouns.read(cx).value().to_string() })
    }

    // What differs on the open tab.
    fn changes(&self, cx: &App) -> Vec<Field> {
        match self.tab {
            ProfileTab::Main => self.saved.as_ref().zip(self.main_draft(cx)).map(|(saved, now)| draft::main_changes(saved, &now)).unwrap_or_default(),
            ProfileTab::Server => self.server_saved.as_ref().zip(self.server_draft(cx)).map(|(saved, now)| draft::server_changes(saved, &now)).unwrap_or_default(),
        }
    }

    fn requests(&self, cx: &App) -> Vec<Request> {
        match self.tab {
            ProfileTab::Main => self.saved.as_ref().zip(self.main_draft(cx)).map(|(saved, now)| draft::main_requests(saved, &now)).unwrap_or_default(),
            ProfileTab::Server => match (&self.guild_id, &self.server_saved, self.server_draft(cx)) {
                (Some(guild_id), Some(saved), Some(now)) => draft::server_requests(guild_id, saved, &now),
                _ => Vec::new(),
            },
        }
    }

    fn tag_label(&self, guild_id: &str) -> Option<String> {
        self.tags.iter().find(|t| t.guild_id == guild_id).map(|t| format!("{} ({})", t.name, t.tag))
    }
}

fn limited_input(max: usize, placeholder: &'static str, window: &mut Window, cx: &mut Context<Waffle>) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).validate(move |t, _| t.chars().count() <= max));
    cx.subscribe(&input, |_, _, _: &InputEvent, cx| cx.notify()).detach();
    input
}

impl Waffle {
    fn profile_editor(&mut self) -> Option<&mut ProfileEditor> {
        self.settings.as_mut()?.profiles.as_mut()
    }

    fn profile_editor_ref(&self) -> Option<&ProfileEditor> {
        self.settings.as_ref()?.profiles.as_ref()
    }

    // Run as the page draws: makes the editor the first time, and follows a search jump to the other tab.
    pub(super) fn ensure_profile_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = &mut self.settings else { return };
        if view.profiles.is_none() {
            view.profiles = Some(ProfileEditor::new(window, cx));
            self.load_main_profile(window, cx);
        }
        match self.settings.as_ref().and_then(|v| v.pending_jump) {
            Some(SERVER_GROUP) => self.switch_profile_tab(ProfileTab::Server, window, cx),
            Some(MAIN_GROUP) => self.switch_profile_tab(ProfileTab::Main, window, cx),
            _ => {}
        }
    }

    fn load_main_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let body = client.my_profile(&me.id, None).await;
            let recent = client.recent_avatars().await.map(|b| draft::recent_from(&b, &me.id)).unwrap_or_default();
            let worn = body.as_ref().ok().and_then(|b| draft::main_from(b).tag_guild);
            // Not verified: Discord marking the servers that have a tag with the GUILD_TAGS feature.
            let Ok(candidates) = this.update(cx, |this, _| this.guilds.iter().filter(|g| g.has_feature("GUILD_TAGS") || Some(&g.id) == worn.as_ref()).map(|g| (g.id.clone(), g.name.clone())).collect::<Vec<_>>()) else { return };
            let mut tags = Vec::new();
            for (id, name) in candidates {
                if let Ok(profile) = client.guild_profile(&id).await {
                    tags.extend(draft::tag_from(&profile, &id, &name));
                }
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let me_id = me.id.clone();
                let Some(editor) = this.profile_editor() else { return };
                (editor.recent, editor.tags) = (recent, tags);
                match body {
                    Ok(body) => {
                        editor.shown = crate::discord::parse_profile(&me_id, &body);
                        editor.saved = Some(draft::main_from(&body));
                        this.reset_main_profile(window, cx);
                    }
                    Err(error) => editor.load_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // The main tab's fields back to what Discord has.
    fn reset_main_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.profile_editor() else { return };
        let Some(saved) = editor.saved.clone() else { return };
        editor.name.update(cx, |s, cx| s.set_value(saved.display_name.clone(), window, cx));
        editor.pronouns.update(cx, |s, cx| s.set_value(saved.pronouns.clone(), window, cx));
        editor.bio.update(cx, |s, cx| s.set_value(saved.bio.clone(), window, cx));
        (editor.avatar, editor.accent, editor.tag_guild, editor.error) = (AvatarChoice::Current, saved.accent, saved.tag_guild, None);
        cx.notify();
    }

    fn reset_server_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username = self.my_account.username.clone();
        let Some(editor) = self.profile_editor() else { return };
        let saved = editor.server_saved.clone().unwrap_or_default();
        let display = editor.saved.as_ref().map(|s| s.display_name.clone()).filter(|n| !n.is_empty()).unwrap_or(username);
        editor.nick.update(cx, |s, cx| {
            s.set_value(saved.nick.clone(), window, cx);
            s.set_placeholder(display, window, cx);
        });
        editor.server_pronouns.update(cx, |s, cx| s.set_value(saved.pronouns.clone(), window, cx));
        editor.error = None;
        cx.notify();
    }

    fn reset_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.profile_editor_ref().map(|e| e.tab) {
            Some(ProfileTab::Main) => self.reset_main_profile(window, cx),
            Some(ProfileTab::Server) => self.reset_server_profile(window, cx),
            None => {}
        }
    }

    // Refused with unsaved changes, as Discord does: the bar shakes instead.
    fn refuse_if_unsaved(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.settings.as_mut().and_then(|v| v.profiles.as_mut()) else { return true };
        if editor.changes(cx).is_empty() {
            return false;
        }
        editor.shakes += 1;
        cx.notify();
        true
    }

    fn switch_profile_tab(&mut self, tab: ProfileTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.profile_editor_ref().is_none_or(|e| e.tab == tab) || self.refuse_if_unsaved(cx) {
            return;
        }
        let first = self.guild_id.clone().or_else(|| self.guilds.first().map(|g| g.id.clone()));
        let Some(editor) = self.profile_editor() else { return };
        (editor.tab, editor.menu) = (tab, None);
        if tab == ProfileTab::Server && editor.guild_id.is_none() && let Some(first) = first {
            self.choose_profile_server(first, window, cx);
        }
        cx.notify();
    }

    fn choose_profile_server(&mut self, guild_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.profile_editor() {
            editor.menu = None;
        }
        if self.profile_editor_ref().is_none_or(|e| e.guild_id.as_ref() == Some(&guild_id)) || self.refuse_if_unsaved(cx) {
            return cx.notify();
        }
        let Some(editor) = self.profile_editor() else { return };
        (editor.guild_id, editor.server_saved) = (Some(guild_id.clone()), None);
        self.reset_server_profile(window, cx);
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let body = client.my_profile(&me.id, Some(&guild_id)).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(editor) = this.profile_editor().filter(|e| e.guild_id.as_ref() == Some(&guild_id)) else { return };
                match body {
                    Ok(body) => {
                        editor.server_saved = Some(draft::server_from(&body));
                        this.reset_server_profile(window, cx);
                    }
                    Err(error) => editor.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn edit_profile(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut ProfileEditor)) {
        if let Some(editor) = self.profile_editor() {
            change(editor);
        }
        cx.notify();
    }

    fn save_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let Some(editor) = self.settings.as_mut().and_then(|v| v.profiles.as_mut()) else { return };
        let requests = editor.requests(cx);
        if editor.saving || requests.is_empty() {
            return;
        }
        let Some(client) = client else { return };
        (editor.saving, editor.error) = (true, None);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let mut user = None;
            let mut result = Ok(());
            for request in requests {
                match send(&client, request).await {
                    Ok(answer) => user = answer.or(user),
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
            let _ = this.update_in(cx, |this, window, cx| this.profile_saved(result, user, window, cx));
        })
        .detach();
    }

    // Discord's answer is the truth: the updated user shows everywhere, and the draft becomes what's saved.
    fn profile_saved(&mut self, result: Result<(), String>, user: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(user) = &user {
            self.my_account = crate::discord::MyAccount::parse(user);
            let updated = crate::discord::parse_user(user);
            if let Some(me) = &mut self.me {
                (me.name, me.avatar) = (updated.name, updated.avatar);
            }
        }
        let me_id = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        let Some(editor) = self.settings.as_mut().and_then(|v| v.profiles.as_mut()) else { return };
        editor.saving = false;
        if let Err(error) = result {
            editor.error = Some(error);
            return cx.notify();
        }
        let avatar_changed = editor.avatar != AvatarChoice::Current;
        match editor.tab {
            ProfileTab::Main => {
                editor.saved = editor.main_draft(cx).map(|d| MainProfile { avatar: AvatarChoice::Current, ..d });
                editor.avatar = AvatarChoice::Current;
                let worn = editor.tag_guild.as_ref().and_then(|g| editor.tags.iter().find(|t| &t.guild_id == g)).map(|t| GuildTag { text: t.tag.clone(), badge: t.badge.clone() });
                if let Some(me) = &mut self.me {
                    me.tag = worn;
                }
            }
            ProfileTab::Server => editor.server_saved = editor.server_draft(cx),
        }
        self.profile_changed(&me_id);
        if avatar_changed {
            self.reload_recent_avatars(window, cx);
        }
        cx.notify();
    }

    fn reload_recent_avatars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(client), Some(me)) = (self.client.clone(), self.me.clone()) else { return };
        cx.spawn_in(window, async move |this, cx| {
            let Ok(body) = client.recent_avatars().await else { return };
            let _ = this.update(cx, |this, cx| {
                if let Some(editor) = this.profile_editor() {
                    editor.recent = draft::recent_from(&body, &me.id);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // Off the list at once; deleted on Discord only once its Undo has run out, even if Settings closes first.
    fn remove_recent_avatar(&mut self, index: usize, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let Some(editor) = self.profile_editor() else { return };
        if index >= editor.recent.len() {
            return;
        }
        let avatar = editor.recent.remove(index);
        if editor.avatar == AvatarChoice::Recent(avatar.id.clone()) {
            editor.avatar = AvatarChoice::Current;
        }
        let undone = Rc::new(Cell::new(false));
        editor.removing = Some(Removal { avatar: avatar.clone(), index, undone: undone.clone() });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_FOR).await;
            if undone.get() {
                return;
            }
            let result = match client {
                Some(client) => client.delete_recent_avatar(&avatar.id).await,
                None => Ok(()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(editor) = this.profile_editor().filter(|e| e.removing.as_ref().is_some_and(|r| r.avatar.id == avatar.id)) {
                    editor.removing = None;
                }
                if let Err(error) = result {
                    this.show_toast(error, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn undo_avatar_removal(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.profile_editor() else { return };
        let Some(removal) = editor.removing.take() else { return };
        removal.undone.set(true);
        let at = removal.index.min(editor.recent.len());
        editor.recent.insert(at, removal.avatar);
        cx.notify();
    }

    pub(super) fn profiles_page(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Section> {
        let Some(editor) = self.profile_editor_ref() else { return Vec::new() };
        let this = cx.entity();
        let tabs = ui::tabs(
            "profile-tabs",
            &[ui::Tab::new("Main Profile"), ui::Tab::new("Per-server Profiles")],
            if editor.tab == ProfileTab::Main { 0 } else { 1 },
            move |i, window, cx| this.update(cx, |this, cx| this.switch_profile_tab(if i == 0 { ProfileTab::Main } else { ProfileTab::Server }, window, cx)),
            window,
            cx,
        );
        let (groups, group): (Vec<AnyElement>, &'static str) = match (&editor.saved, &editor.load_error) {
            (_, Some(error)) => (vec![ui::callout(Notice::Danger, Glyph::WarningCircle, "Couldn't load your profile", error.clone(), cx).into_any_element()], MAIN_GROUP),
            (None, None) => (vec![ui::settings_card(cx).child(ui::settings_row("pf-loading", false, cx).child(ui::settings_text("Loading your profile…", None::<&str>, cx))).into_any_element()], MAIN_GROUP),
            (Some(_), None) if editor.tab == ProfileTab::Main => (self.main_profile_groups(editor, cx), MAIN_GROUP),
            (Some(_), None) => (self.server_profile_groups(editor, cx), SERVER_GROUP),
        };
        let fields = div().flex_1().min_w_0().flex().flex_col().gap_6().child(tabs).children(groups);
        let body = div().flex().items_start().gap_8().child(fields).children(editor.saved.is_some().then(|| self.profile_preview_column(editor, cx)));
        vec![Section::keyed(group, body)]
    }

    fn main_profile_groups(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut groups = Vec::new();
        let name_count = editor.name.read(cx).value().chars().count();
        let pronoun_count = editor.pronouns.read(cx).value().chars().count();
        let identity = ui::settings_card(cx)
            .child(field_row("pf-name", false, "Display name", text_input("pf-name-input", &editor.name, cx), count_hint("Shown instead of your username", name_count, draft::DISPLAY_NAME_MAX, cx), cx))
            .child(field_row("pf-pronouns", true, "Pronouns", text_input("pf-pronouns-input", &editor.pronouns, cx), count_hint("", pronoun_count, draft::PRONOUNS_MAX, cx), cx));
        groups.push(ui::settings_group("Identity", cx).child(identity).into_any_element());
        groups.push(ui::settings_group("Avatar", cx).child(ui::settings_card(cx).child(self.avatar_row(editor, cx)).child(self.recent_avatars_row(editor, cx))).into_any_element());
        if !self.my_account.premium {
            let caption = "Without Nitro, Discord shows this colour where a banner would be.";
            groups.push(ui::settings_group("Banner colour", cx).child(ui::settings_card(cx).child(self.banner_colour_row(editor, cx))).child(ui::settings_caption(caption, cx)).into_any_element());
        }
        let bio_count = editor.bio.read(cx).value().chars().count();
        let bio_focus = editor.bio.read(cx).focus_handle(cx);
        let bio = ui::text_area("pf-bio-input", Textarea::new(&editor.bio).appearance(false).px_0().text_size(ui::TEXT_SM), &bio_focus);
        let about = ui::settings_card(cx).child(field_row("pf-bio", false, "About me", bio, count_hint("You can use markdown and links if you'd like.", bio_count, draft::BIO_MAX, cx), cx));
        groups.push(ui::settings_group("About me", cx).child(about).into_any_element());
        if !editor.tags.is_empty() || editor.tag_guild.is_some() {
            groups.push(ui::settings_group("Server tag", cx).child(ui::settings_card(cx).child(self.server_tag_row(editor, cx))).into_any_element());
        }
        let nitro = nitro_row("pf-nitro", "Decoration, nameplate, effect, frame and theme", "Discord only lets Nitro set these. Yours show here as they are.", cx);
        groups.push(ui::settings_group("Nitro", cx).child(ui::settings_card(cx).child(nitro)).into_any_element());
        groups
    }

    fn avatar_row(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Stateful<Div> {
        let face = match self.preview_avatar(editor) {
            PreviewAvatar::Url(url) => ui::avatar(url, 64.).into_any_element(),
            PreviewAvatar::File(path) => gpui_kit::img(path).size_16().flex_none().rounded_full().into_any_element(),
        };
        let has_avatar = self.me.as_ref().is_some_and(|m| !m.avatar.contains("/embed/avatars/"));
        let removable = editor.avatar != AvatarChoice::Removed && (has_avatar || editor.avatar != AvatarChoice::Current);
        let change = ui::button("pf-avatar-change", Tone::Secondary, cx).small().child(Icon::new(Glyph::Upload).size(ui::ICON_MD)).child("Change avatar").on_click(cx.listener(|this, _, _, cx| {
            this.prompt_profile_image(cx, |this, path, data, cx| this.edit_profile(cx, |e| e.avatar = AvatarChoice::Upload { path, data }));
        }));
        let remove = ui::button("pf-avatar-remove", Tone::Link, cx).small().disabled(!removable).child("Remove avatar").on_click(cx.listener(|this, _, _, cx| this.edit_profile(cx, |e| e.avatar = AvatarChoice::Removed)));
        ui::settings_row("pf-avatar", false, cx).child(face).child(div().flex_1()).child(change).child(remove)
    }

    fn recent_avatars_row(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Stateful<Div> {
        let slots = (0..draft::RECENT_AVATARS).map(|i| match editor.recent.get(i) {
            Some(avatar) => self.recent_slot(i, avatar, editor.avatar == AvatarChoice::Recent(avatar.id.clone()), cx).into_any_element(),
            None => div().size(px(SLOT)).flex_none().rounded_full().border(px(1.5)).border_dashed().border_color(palette::line_strong(cx)).into_any_element(),
        }).collect::<Vec<_>>();
        let field = ui::field("Recent avatars", cx).flex_1().child(div().flex().flex_wrap().gap_3().py_1().children(slots)).child(ui::field_hint("Your last 6 avatars. Pick one to use it again; no new upload.", None, cx));
        ui::settings_row("pf-recent", true, cx).child(field)
    }

    fn recent_slot(&self, i: usize, avatar: &RecentAvatar, chosen: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let group = SharedString::from(format!("pf-recent-{i}"));
        let id = avatar.id.clone();
        let surface = palette::surface(Level::Overlay, cx);
        let remove = div()
            .id(SharedString::from(format!("pf-recent-{i}-remove")))
            .absolute()
            .top_0()
            .right_0()
            .size_5()
            .rounded_full()
            .bg(surface)
            .shadow(ui::shadow_pop(cx))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(palette::strong(cx))
            .invisible()
            .group_hover(group.clone(), |s| s.visible())
            .child(Icon::new(Glyph::X).size(ui::ICON_XS))
            .tooltip(|window, cx| ui::tip("Remove from recent avatars", window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.remove_recent_avatar(i, cx);
            }));
        div()
            .id(group.clone())
            .group(group)
            .relative()
            .size(px(SLOT))
            .flex_none()
            .rounded_full()
            .cursor_pointer()
            .when(chosen, |d| d.shadow(vec![ui::shadow(palette::accent(), 0., 0., 0., 2., false)]))
            .child(ui::avatar(avatar.url.clone(), SLOT))
            .child(remove)
            .on_click(cx.listener(move |this, _, _, cx| this.edit_profile(cx, |e| e.avatar = AvatarChoice::Recent(id.clone()))))
    }

    fn banner_colour_row(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Stateful<Div> {
        let ring = palette::surface(Level::Group, cx);
        let mut colours = BANNER_COLOURS.to_vec();
        if let Some(saved) = editor.saved.as_ref().and_then(|s| s.accent).filter(|c| !colours.contains(c)) {
            colours.insert(0, saved);
        }
        let none = ui::chip("pf-accent-default", None, "Default", editor.accent.is_none(), cx).on_click(cx.listener(|this, _, _, cx| this.edit_profile(cx, |e| e.accent = None)));
        let swatches = colours.into_iter().map(|colour| {
            ui::swatch(SharedString::from(format!("pf-accent-{colour:06x}")), rgb(colour).into(), editor.accent == Some(colour), ring).on_click(cx.listener(move |this, _, _, cx| this.edit_profile(cx, |e| e.accent = Some(colour))))
        });
        ui::settings_row("pf-accent", false, cx).child(div().flex().flex_wrap().items_center().gap_2().child(none).children(swatches))
    }

    fn server_tag_row(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = editor.menu == Some(Menu::Tag);
        let mut options: Vec<(String, String)> = editor.tags.iter().map(|t| (t.guild_id.clone(), format!("{} ({})", t.name, t.tag))).collect();
        options.push((String::new(), "No tag".to_owned()));
        let chosen = editor.tag_guild.clone().unwrap_or_default();
        let value = match &editor.tag_guild {
            None => "No tag".to_owned(),
            Some(id) => editor.tag_label(id).or_else(|| self.guilds.iter().find(|g| &g.id == id).map(|g| g.name.clone())).unwrap_or_else(|| "A server's tag".to_owned()),
        };
        let pick = |id: String, this: &mut Waffle, _: &mut Window, cx: &mut Context<Waffle>| {
            this.edit_profile(cx, |e| {
                e.tag_guild = (!id.is_empty()).then_some(id);
                e.menu = None;
            })
        };
        let select = self.profile_select("pf-tag-select", Menu::Tag, Some(Glyph::Tag), value, &options, &chosen, open, pick, cx);
        let field = ui::field("Server tag", cx).flex_1().child(select).child(ui::field_hint("Choose a server tag to rep next to your name across Discord.", None, cx));
        ui::settings_row("pf-tag", false, cx).child(field)
    }

    // A dropdown on this page: its trigger, and its list while open.
    #[allow(clippy::too_many_arguments)]
    fn profile_select(
        &self,
        id: &'static str,
        menu: Menu,
        glyph: Option<Glyph>,
        value: String,
        options: &[(String, String)],
        chosen: &str,
        open: bool,
        pick: impl Fn(String, &mut Waffle, &mut Window, &mut Context<Waffle>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let trigger = ui::select(id, glyph, value, open, cx).on_click(cx.listener(move |this, _, _, cx| this.edit_profile(cx, |e| e.menu = if e.menu == Some(menu) { None } else { Some(menu) })));
        let list = open.then(|| {
            let (this, close) = (cx.entity(), cx.entity());
            let dismiss = move |_: &mut Window, cx: &mut App| close.update(cx, |this, cx| this.edit_profile(cx, |e| e.menu = None));
            ui::select_options(SharedString::from(format!("{id}-options")), options, chosen, 40., move |value, window, cx| this.update(cx, |this, cx| pick(value, this, window, cx)), dismiss, cx)
        });
        div().relative().w_full().child(trigger).children(list)
    }

    fn server_profile_groups(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let lede = div().px_1().text_color(palette::muted(cx)).child("Show who you are with different profiles for each of your servers.");
        let Some(guild) = editor.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)) else {
            let none = ui::settings_card(cx).child(ui::settings_row("pf-no-server", false, cx).child(ui::settings_text("Join a server to give it its own profile", None::<&str>, cx)));
            return vec![lede.into_any_element(), none.into_any_element()];
        };
        let options: Vec<(String, String)> = self.guilds.iter().map(|g| (g.id.clone(), g.name.clone())).collect();
        let open = editor.menu == Some(Menu::Server);
        let picker = self.profile_select("pf-server-select", Menu::Server, None, guild.name.clone(), &options, &guild.id, open, |id, this, window, cx| this.choose_profile_server(id, window, cx), cx);
        let Some(_) = editor.server_saved else {
            let loading = ui::settings_card(cx).child(ui::settings_row("pf-server-loading", false, cx).child(ui::settings_text(editor.error.clone().unwrap_or_else(|| "Loading your server profile…".to_owned()), None::<&str>, cx)));
            return vec![lede.into_any_element(), picker.into_any_element(), loading.into_any_element()];
        };
        let me = self.me.as_ref().map(|m| m.id.as_str()).unwrap_or_default();
        let can_nick = guild.can(me, perm::CHANGE_NICKNAME | perm::MANAGE_NICKNAMES);
        let display = editor.saved.as_ref().map(|s| s.display_name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| self.my_account.username.clone());
        let nick_hint = if can_nick { format!("Empty uses your display name, {display}") } else { "This server doesn't let you change your nickname".to_owned() };
        let nick_count = editor.nick.read(cx).value().chars().count();
        let pronoun_count = editor.server_pronouns.read(cx).value().chars().count();
        let nick_focus = editor.nick.read(cx).focus_handle(cx);
        let nick = ui::text_field("pf-nick-input", Input::new(&editor.nick).appearance(false).disabled(!can_nick), &nick_focus).disabled(!can_nick);
        let card = ui::settings_card(cx)
            .child(field_row("pf-nick", false, "Server nickname", nick, count_hint(nick_hint, nick_count, draft::NICK_MAX, cx), cx))
            .child(field_row("pf-server-pronouns", true, "Pronouns", text_input("pf-server-pronouns-input", &editor.server_pronouns, cx), count_hint("", pronoun_count, draft::PRONOUNS_MAX, cx), cx));
        let nitro = nitro_row("pf-server-nitro", "Server avatar, banner, bio and nameplate", "Discord only lets Nitro set these per server.", cx);
        vec![
            lede.into_any_element(),
            picker.into_any_element(),
            ui::settings_group(format!("{} profile", guild.name), cx).child(card).into_any_element(),
            ui::settings_group("Nitro", cx).child(ui::settings_card(cx).child(nitro)).into_any_element(),
        ]
    }


    fn preview_avatar(&self, editor: &ProfileEditor) -> PreviewAvatar {
        let me_id = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
        match &editor.avatar {
            AvatarChoice::Current => PreviewAvatar::Url(self.me.as_ref().map(|m| m.avatar.clone()).unwrap_or_default()),
            AvatarChoice::Recent(id) => PreviewAvatar::Url(editor.recent.iter().find(|r| &r.id == id).map(|r| r.url.clone()).unwrap_or_default()),
            AvatarChoice::Upload { path, .. } => PreviewAvatar::File(path.clone()),
            AvatarChoice::Removed => PreviewAvatar::Url(crate::discord::avatar_url(&me_id, None)),
        }
    }

    // "Preview" or "Preview for {server}", over the popout drawn from the draft.
    fn profile_preview_column(&self, editor: &ProfileEditor, cx: &mut Context<Self>) -> Div {
        let mut user = self.me.clone().unwrap_or_default();
        let mut profile = editor.shown.clone();
        let main = editor.main_draft(cx).unwrap_or_default();
        let display = Some(main.display_name.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| self.my_account.username.clone());
        user.name = display.clone();
        user.tag = main.tag_guild.as_ref().and_then(|g| editor.tags.iter().find(|t| &t.guild_id == g)).map(|t| GuildTag { text: t.tag.clone(), badge: t.badge.clone() }).or(if main.tag_guild.is_some() { user.tag } else { None });
        profile.username = self.my_account.username.clone();
        profile.bio = Some(main.bio.trim_end().to_owned()).filter(|b| !b.is_empty());
        profile.pronouns = Some(main.pronouns.trim().to_owned()).filter(|p| !p.is_empty());
        profile.accent = main.accent;
        let server = editor.guild_id.as_ref().and_then(|id| self.guilds.iter().find(|g| &g.id == id)).filter(|_| editor.tab == ProfileTab::Server);
        let title = match (server, editor.server_draft(cx)) {
            (Some(guild), Some(server)) => {
                user.name = Some(server.nick.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or(display);
                if !server.pronouns.trim().is_empty() {
                    profile.pronouns = Some(server.pronouns.trim().to_owned());
                }
                format!("Preview for {}", guild.name)
            }
            (Some(guild), None) => format!("Preview for {}", guild.name),
            (None, _) => "Preview".to_owned(),
        };
        let card = profile_preview(&user, &profile, self.preview_avatar(editor), Some(self.my_status.into()), cx);
        ui::settings_group(title, cx).flex_none().w(px(crate::profile::WIDTH)).child(card)
    }

    // Slides up while something differs: what changed (or Discord's refusal, verbatim), Reset and Save changes.
    pub(super) fn profiles_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.profile_editor_ref()?;
        let changes = editor.changes(cx);
        if changes.is_empty() && !editor.saving {
            return None;
        }
        let (message, ink) = match &editor.error {
            Some(error) => (error.clone(), palette::danger_ink(cx)),
            None => (format!("Careful, you have unsaved changes: {}", draft::changed_names(&changes)), palette::strong(cx)),
        };
        let reset = ui::button("pf-reset", Tone::Link, cx).disabled(editor.saving).child("Reset").on_click(cx.listener(|this, _, window, cx| this.reset_profile(window, cx)));
        let save = ui::button("pf-save", Tone::Primary, cx).loading(editor.saving.then_some("Saving…")).child("Save changes").on_click(cx.listener(|this, _, window, cx| this.save_profile(window, cx)));
        let bar = div()
            .id("pf-savebar")
            .occlude()
            .w_full()
            .max_w(px(ui::SETTINGS_WIDTH))
            .py_2p5()
            .pl_4()
            .pr_2p5()
            .squircle(ui::RADIUS_LG)
            .bg(palette::surface(Level::Overlay, cx))
            .shadow(ui::shadow_pop(cx))
            .flex()
            .items_center()
            .gap_3()
            .child(div().flex_1().min_w_0().text_color(ink).child(message))
            .child(div().flex().flex_none().items_center().gap_2().child(reset).child(save));
        let shaken = div().w_full().flex().justify_center().child(bar).with_animation(SharedString::from(format!("pf-shake-{}", editor.shakes)), Animation::new(SHAKE), |el, t| el.translate_x(px(shake_offset(t))));
        let slid = div().w_full().child(shaken).with_animation("pf-savebar-in", Animation::new(ui::DUR_PANEL).with_easing(|t| motion::ease_out().sample(t)), |el, t| el.opacity(t).translate_y(px(16. * (1. - t))));
        Some(div().absolute().bottom_4().left_10().right_10().child(slid).into_any_element())
    }

    // "Avatar removed" with Undo, over the bar when it shows.
    pub(super) fn profiles_undo(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let editor = self.profile_editor_ref()?;
        let removal = editor.removing.as_ref()?;
        let undo = ui::button("pf-undo", Tone::Secondary, cx).small().child("Undo").on_click(cx.listener(|this, _, _, cx| this.undo_avatar_removal(cx)));
        let toast = ui::toast_with(SharedString::from(format!("pf-undo-{}", removal.avatar.id)), Glyph::Trash, palette::muted(cx), "Avatar removed", None, Some(undo.into_any_element()), cx);
        let bar_shows = !editor.changes(cx).is_empty() || editor.saving;
        let surface = div().id("pf-undo-toast").occlude().w(px(380.)).child(toast);
        Some(div().absolute().bottom(px(if bar_shows { 84. } else { 16. })).left_0().right_0().flex().justify_center().child(surface).into_any_element())
    }
}

// Demo scenes: the editor filled as Discord would answer, without asking it.
#[cfg(debug_assertions)]
impl Waffle {
    pub(crate) fn demo_profile_editor(&mut self, tab: ProfileTab, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings(Page::Profiles, window, cx);
        self.ensure_profile_editor(window, cx);
        let guild = self.guilds.first().map(|g| g.id.clone());
        let Some(editor) = self.profile_editor() else { return };
        editor.saved = Some(MainProfile { display_name: "kiwi".into(), pronouns: "they/them".into(), bio: "Plays support. Will heal you, will **not** carry you.".into(), accent: Some(0x1ABC9C), tag_guild: guild.clone(), avatar: AvatarChoice::Current });
        editor.shown = crate::discord::Profile { username: "kiwi.dev".into(), ..Default::default() };
        editor.recent = ["peek", "riso", "sticker", "blueprint"].iter().enumerate().map(|(i, n)| RecentAvatar { id: format!("a{i}"), url: format!("brand/icons/{n}.png") }).collect();
        editor.tags = guild.iter().map(|g| TagGuild { guild_id: g.clone(), name: "Night Shift".into(), tag: "NITE".into(), badge: Some("brand/icons/sticker.png".into()) }).collect();
        self.reset_main_profile(window, cx);
        match tab {
            ProfileTab::Main => {
                let Some(editor) = self.profile_editor() else { return };
                editor.name.update(cx, |s, cx| s.set_value("kiwi, healer", window, cx));
                editor.bio.update(cx, |s, cx| s.set_value("Plays support. Will heal you, will **not** carry you.\nAsk me about _gpui_.", window, cx));
                editor.accent = Some(0x9B59B6);
            }
            ProfileTab::Server => {
                self.switch_profile_tab(ProfileTab::Server, window, cx);
                if let Some(editor) = self.profile_editor() {
                    editor.server_saved = Some(ServerProfile::default());
                }
                self.reset_server_profile(window, cx);
            }
        }
        cx.notify();
    }
}

// A labelled field filling a settings row: the label, the input, the hint and count under it.
fn field_row(id: &'static str, line: bool, label: &'static str, input: impl IntoElement, hint: Div, cx: &App) -> Stateful<Div> {
    ui::settings_row(id, line, cx).child(ui::field(label, cx).flex_1().child(input).child(hint))
}

fn text_input(id: &'static str, state: &Entity<InputState>, cx: &App) -> impl IntoElement {
    ui::text_field(id, Input::new(state).appearance(false), &state.read(cx).focus_handle(cx))
}

// Help under a field with its count, which turns red only at the limit.
fn count_hint(text: impl Into<SharedString>, count: usize, max: usize, cx: &App) -> Div {
    let ink = if count >= max { palette::danger_ink(cx) } else { palette::faint(cx) };
    div().flex().justify_between().gap_2().text_size(ui::TEXT_XS).text_color(palette::faint(cx)).child(text.into()).child(div().flex_none().text_color(ink).child(format!("{count}/{max}")))
}

fn nitro_row(id: &'static str, title: &'static str, caption: &'static str, cx: &mut Context<Waffle>) -> Stateful<Div> {
    let open = ui::button(SharedString::from(format!("{id}-open")), Tone::Secondary, cx).small().child(Icon::new(Glyph::Open).size(ui::ICON_MD)).child("Set on Discord").on_click(|_, _, cx| cx.open_url(DISCORD));
    ui::settings_row(id, false, cx).child(div().flex_none().text_color(palette::muted(cx)).child(Icon::new(Glyph::Gem).size(ui::ICON_XL))).child(ui::settings_text(title, Some(caption), cx)).child(open)
}

// A short horizontal shake, settling to rest.
fn shake_offset(t: f32) -> f32 {
    8. * (t * 4. * std::f32::consts::PI).sin() * (1. - t)
}

// One save request against its endpoint; the user PATCH answers with the updated user.
async fn send(client: &Client, request: Request) -> Result<Option<Value>, String> {
    match request {
        Request::User(body) => client.modify_me(body).await.map(Some),
        Request::Profile(body) => client.modify_my_profile(body).await.map(|_| None),
        Request::ServerTag(guild_id) => client.set_server_tag(guild_id.as_deref()).await.map(|_| None),
        Request::Member { guild_id, body } => client.modify_my_member(&guild_id, body).await.map(|_| None),
        Request::GuildProfile { guild_id, body } => client.modify_guild_profile(&guild_id, body).await.map(|_| None),
    }
}
