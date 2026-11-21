// Create Channel, Discord's dialog: the type (text, voice, forum), the name with the type's glyph in
// front, and Private channel, which hides it from @everyone. Opened from a category's + (in that
// category) or the server menu (uncategorised).
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{Icon, input::Input},
    div,
    prelude::FluentBuilder as _,
    px,
};
use reqwest::Method;
use serde_json::json;

use crate::{
    Waffle,
    discord::VIEW_CHANNEL,
    icons::Glyph,
    palette,
    ui::{self, Squircle as _, Tone},
};

// Discord's API channel types: 0 text, 2 voice, 15 forum.
const TYPES: [(u64, Glyph, &str, &str); 3] = [
    (0, Glyph::Hashtag, "Text", "Send messages, images, GIFs, emoji, opinions and puns"),
    (2, Glyph::VolumeHigh, "Voice", "Hang out together with voice, video and screen share"),
    (15, Glyph::Forum, "Forum", "Create a space for organised discussions"),
];

pub(crate) struct CreateChannel {
    guild_id: String,
    parent: Option<String>,
    category: Option<String>,
    kind: usize,
    private: bool,
}

impl Waffle {
    pub(crate) fn open_create_channel(&mut self, guild_id: String, kind: u64, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let category = parent.as_ref().and_then(|p| self.guilds.iter().find(|g| g.id == guild_id)?.channels.iter().find(|c| &c.id == p)).map(|c| c.name.clone());
        let kind = TYPES.iter().position(|t| t.0 == kind).unwrap_or(0);
        self.menu = None;
        self.dialog_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_placeholder("new-channel", window, cx);
            state.focus(window, cx);
        });
        self.create_channel = Some(CreateChannel { guild_id, parent, category, kind, private: false });
        cx.notify();
    }

    fn close_create_channel(&mut self, cx: &mut Context<Self>) {
        self.create_channel = None;
        cx.notify();
    }

    pub(crate) fn submit_create_channel(&mut self, cx: &mut Context<Self>) {
        let name = self.dialog_input.read(cx).value().trim().to_owned();
        let Some(form) = self.create_channel.as_ref().filter(|_| !name.is_empty()) else { return };
        let mut body = json!({ "name": name, "type": TYPES[form.kind].0, "parent_id": form.parent });
        if form.private {
            // Hidden from @everyone (whose role id is the server's), visible to you; Discord's own flow
            // asks for more members and roles next. Not verified that it adds the creator the same way.
            let me = self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default();
            body["permission_overwrites"] = json!([
                { "id": form.guild_id, "type": 0, "allow": "0", "deny": VIEW_CHANNEL.to_string() },
                { "id": me, "type": 1, "allow": VIEW_CHANNEL.to_string(), "deny": "0" },
            ]);
        }
        let path = format!("/guilds/{}/channels", form.guild_id);
        self.create_channel = None;
        self.rest(Method::POST, path, Some(body), |_, _| {}, cx);
    }

    pub(crate) fn render_create_channel(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.create_channel.as_ref()?;
        let (strong, muted) = (palette::strong(cx), palette::muted(cx));
        let label = |text: &'static str| div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child(text);
        let rows: Vec<AnyElement> = TYPES.iter().enumerate().map(|(i, (_, glyph, name, hint))| type_row(i, *glyph, name, hint, i == form.kind, cx).into_any_element()).collect();
        let types = div().flex().flex_col().gap(px(2.)).children(rows);
        let empty = self.dialog_input.read(cx).value().trim().is_empty();
        let focus = gpui_kit::Focusable::focus_handle(self.dialog_input.read(cx), cx);
        let name = ui::text_field("create-channel-name", Input::new(&self.dialog_input).appearance(false), &focus).leading(Icon::new(TYPES[form.kind].1).size(px(16.)));
        let private = div()
            .id("create-channel-private")
            .flex()
            .items_center()
            .gap(px(16.))
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().flex().items_center().gap(px(6.)).text_color(strong).child(Icon::default().path("icons/hashtag-lock.svg").size(px(16.))).child(div().font_weight(FontWeight::MEDIUM).child("Private channel")))
                    .child(div().text_size(px(12.)).line_height(px(17.)).text_color(muted).child("Only you and the members and roles you add can see it.")),
            )
            .child(ui::switch_live("create-channel-private-switch", form.private, window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(form) = this.create_channel.as_mut() {
                    form.private = !form.private;
                }
                cx.notify();
            }));
        let actions = div()
            .flex()
            .gap(px(8.))
            .child(ui::button("create-channel-cancel", Tone::Secondary, cx).flex_1().child("Cancel").on_click(cx.listener(|this, _, _, cx| this.close_create_channel(cx))))
            .child(ui::button("create-channel-submit", Tone::Primary, cx).flex_1().child("Create channel").when(empty, |b| b.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.submit_create_channel(cx))));
        let head = div()
            .flex()
            .items_start()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().text_size(px(20.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(strong).child("Create channel"))
                    .children(form.category.clone().map(|c| div().text_size(px(14.)).text_color(muted).child(format!("in {c}")))),
            )
            .child(ui::icon_button("create-channel-close", Glyph::X, "Close", cx).on_click(cx.listener(|this, _, _, cx| this.close_create_channel(cx))));
        let body = ui::dialog_body()
            .gap(px(20.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| match e.keystroke.key.as_str() {
                "enter" => this.submit_create_channel(cx),
                "escape" => this.close_create_channel(cx),
                _ => {}
            }))
            .child(head)
            .child(div().flex().flex_col().gap(px(8.)).child(label("Channel type")).child(types))
            .child(div().flex().flex_col().gap(px(8.)).child(label("Channel name")).child(name))
            .child(private)
            .child(actions);
        let backdrop = div().id("create-channel-backdrop").absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_create_channel(cx)));
        Some(ui::modal(backdrop, ui::dialog("create-channel", 460., body, window, cx), cx))
    }
}

// One type: radio, then the glyph inline with its name and its line under; the chosen one carries the selected fill.
fn type_row(i: usize, glyph: Glyph, name: &'static str, hint: &'static str, on: bool, cx: &mut Context<Waffle>) -> impl IntoElement {
    let (selected, hover, strong, muted) = (palette::selected(cx), palette::hover(cx), palette::strong(cx), palette::muted(cx));
    div()
        .id(SharedString::from(format!("create-channel-type-{i}")))
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(12.))
        .py(px(10.))
        .squircle(ui::RADIUS_MD)
        .cursor_pointer()
        .when(on, |r| r.bg(selected))
        .when(!on, |r| r.hover(move |s| s.bg(hover)))
        .child(ui::radio(on, cx).flex_none())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().flex().items_center().gap(px(6.)).text_color(strong).child(Icon::new(glyph).size(px(18.))).child(div().font_weight(FontWeight::MEDIUM).child(name)))
                .child(div().text_size(px(12.)).line_height(px(16.)).text_color(muted).child(hint)),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(form) = this.create_channel.as_mut() {
                form.kind = i;
            }
            cx.notify();
        }))
}

pub(crate) fn demo(waffle: &mut Waffle, _scene: &str, window: &mut Window, cx: &mut Context<Waffle>) {
    waffle.open_create_channel("1".into(), 0, Some("c0".into()), window, cx);
}
