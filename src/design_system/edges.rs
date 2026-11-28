// Edge-case frames: each component in boxes too small for it, with the worst data it will meet. A box's border
// is the parent's edge, so anything crossing it is a bug to fix in the component.
use super::*;

const LONG: &str = "An unreasonably long display name that never fits in a sidebar row";
const LONG_DEVICE: &str = "Studio Display Microphone (USB Audio Class 2.0, 48 kHz, 24-bit stereo)";

// A fixed-width box with a caption: the parent a component has to survive.
fn boxed(caption: &'static str, width: f32, content: impl IntoElement, cx: &App) -> Div {
    ui::col().gap_1().child(note(caption, cx)).child(ui::col().w(px(width)).gap_2().p_2().border_1().border_color(palette::line_strong(cx)).squircle(ui::RADIUS_SM).child(content))
}

fn edges(title: &'static str, lede_text: &'static str, cx: &App) -> Div {
    section(title, lede_text, cx)
}

impl DesignSystem {
    pub(super) fn edge_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Frame> {
        let frame = match self.page {
            Page::Button => buttons_edges(cx),
            Page::Input => self.fields_edges(cx),
            Page::Switch | Page::Checkbox | Page::Slider | Page::Segmented | Page::Toggle => choices_edges(self.page, window, cx),
            Page::Tabs => tabs_edges(window, cx),
            Page::CountBadge | Page::RoleChip | Page::Kbd | Page::Badge => badges_edges(cx),
            Page::Avatar | Page::AvatarStack => faces_edges(cx),
            Page::Progress | Page::Alert | Page::Sonner | Page::Empty => feedback_edges(self.page, cx),
            Page::DropdownMenu => surfaces_edges(cx),
            Page::Dialog => dialog_edges(window, cx),
            _ => return Vec::new(),
        };
        vec![(Kind::Edges, frame.into_any_element())]
    }

    fn fields_edges(&self, cx: &mut Context<Self>) -> Div {
        let f = &self.fields;
        let input = |id: &'static str, state: &Entity<InputState>, cx: &App| ui::Input::new(id, Input::new(state).appearance(false).px_0().text_size(ui::TEXT_MD), &state.read(cx).focus_handle(cx));
        edges("Fields at the limit", "Long values scroll inside the field, errors wrap under it, and nothing widens its column.", cx).child(
            board(cx).child(
                row()
                    .gap_6()
                    .items_start()
                    .child(boxed("Long value, 180px", 180., ui::Field::new("Display name", cx).child(input("e-long", &f.long, cx)).child(ui::FieldDescription::new("Shown to everyone", Some("32/32".into()), cx)), cx))
                    .child(boxed("Mixed scripts and emoji", 180., ui::Field::new("Display name", cx).child(input("e-mixed", &f.mixed, cx)), cx))
                    .child(boxed("Error that wraps", 180., ui::Field::new("Invite link", cx).child(input("e-err", &f.invite, cx).error(true)).child(ui::FieldError::new("This invite has expired or was revoked by the person who made it. Ask them for a new one.", cx)), cx))
                    .child(boxed("Select, long value", 220., ui::Field::new("Input device", cx).child(ui::row().flex_1().child(ui::Select::new("e-select", Some(Glyph::Mic), LONG_DEVICE, false, cx))), cx))
                    .child(boxed("Disabled and invalid", 180., ui::Field::new("Username", cx).child(input("e-off", &f.username, cx).disabled(true).error(true)), cx)),
            ),
        )
    }
}

fn buttons_edges(cx: &mut Context<DesignSystem>) -> Div {
    edges("Buttons in a tight parent", "Labels are one line. In a box narrower than the label the button has to clip or shrink inside it, never push past the edge.", cx).child(
        board(cx).child(
            row()
                .gap_6()
                .items_start()
                .child(boxed("Long label, 160px", 160., ui::Button::new("e-b-long").child("Save changes to every channel in this server"), cx))
                .child(boxed("Icon and long label", 160., ui::Button::new("e-b-icon").variant(ui::ButtonVariant::Secondary).child(glyph(Glyph::Stream)).child("Start streaming to the whole lounge"), cx))
                .child(boxed("Loading with a long label", 160., ui::Button::new("e-b-busy").loading(Some("Saving your changes to the server…")).child("Save"), cx))
                .child(boxed("Narrower than its icon and label", 72., ui::Button::new("e-b-tiny").child("Join voice"), cx))
                .child(boxed("Toggled and disabled", 160., ui::Button::new("e-b-both").variant(ui::ButtonVariant::Secondary).toggled(true).disabled(true).child("Muted"), cx))
                .child(boxed("Danger, small, 99+ label", 160., ui::Button::new("e-b-danger").variant(ui::ButtonVariant::Destructive).size(ui::ButtonSize::Sm).child("Delete 1,204 messages"), cx)),
        ),
    )
}

fn choices_edges(page: Page, window: &mut Window, cx: &mut Context<DesignSystem>) -> Div {
    let detail: SharedStringAlias = "Removes background sound from your mic, including keyboard clatter, fans and the dog next door".into();
    let cell = match page {
        Page::Switch => boxed("Switch row, wrapping, 280px", 280., ui::toggle_row("e-toggle", "Noise suppression for every voice channel you join on this account", Some(detail), true, window, cx), cx),
        Page::Checkbox => boxed("Checkbox row, wrapping, 280px", 280., ui::pick_row("e-pick", ui::Checkbox::mark(Check::Mixed).size(ui::ICON_LG), "Notify me for every message in every channel", Some(detail), cx), cx),
        Page::Segmented => boxed("Five segments, 280px", 280., ui::Tabs::new("e-seg").labels(&["Everyone", "Friends of friends", "Server members", "Nobody", "Custom"]).value(1).on_value_change(|_, _, _| {}), cx),
        Page::Slider => row()
            .gap_6()
            .child(boxed("Slider at 0%", 160., ui::Slider::new("e-slider-0").value(0.), cx))
            .child(boxed("Slider at 100%", 160., ui::Slider::new("e-slider-1").value(1.), cx)),
        _ => boxed("Chips wrap, 200px", 200., row().children(["Images", "Links", "Files", "Embeds", "Voice messages", "Stickers"].into_iter().enumerate().map(|(i, name)| ui::Toggle::new(("e-chip", i)).pressed(i == 1).child(name))), cx),
    };
    edges("Long copy and extremes", "Titles and details wrap inside the row; options and chips stay inside their parent; a slider holds at both ends.", cx).child(board(cx).child(row().items_start().child(cell)))
}

type SharedStringAlias = gpui_kit::SharedString;

fn tabs_edges(_window: &mut Window, cx: &mut Context<DesignSystem>) -> Div {
    let many: Vec<_> = ["Overview", "Roles", "Emoji", "Stickers", "Soundboard", "Members", "Invites", "Bans", "Integrations", "Audit log"].into_iter().map(ui::Tab::new).collect();
    let counts = [ui::Tab::new("Members").count(Some(1204)), ui::Tab::new("Requests").count(Some(0)), ui::Tab::new("Announcements and events from the whole community").count(Some(3))];
    let one = [ui::Tab::new("Overview")];
    edges("Tabs past the edge", "Ten tabs in a narrow column, counts from 0 to thousands, one tab, one very long label.", cx).child(
        board(cx).child(
            ui::col()
                .gap_6()
                .child(boxed("Ten tabs, 420px", 420., ui::Tabs::new("e-tabs-many").variant(ui::TabsVariant::Line).tabs(&many).value(0).on_value_change(|_, _, _| {}), cx))
                .child(boxed("Counts 1204, 0 and a long label, 420px", 420., ui::Tabs::new("e-tabs-counts").variant(ui::TabsVariant::Line).tabs(&counts).value(0).on_value_change(|_, _, _| {}), cx))
                .child(boxed("A single tab", 420., ui::Tabs::new("e-tabs-one").variant(ui::TabsVariant::Line).tabs(&one).value(0).on_value_change(|_, _, _| {}), cx)),
        ),
    )
}

fn badges_edges(cx: &mut Context<DesignSystem>) -> Div {
    edges("Counts and labels at the extremes", "A count caps at 99+, a role keeps its colour dot however long its name, a hint with many keys wraps instead of spilling.", cx).child(
        board(cx).child(
            ui::col()
                .gap_4()
                .child(row().gap_4().children([0, 1, 9, 10, 99, 100, 1_000, 100_000].map(ui::Badge::count)).child(note("0 · 1 · 9 · 10 · 99 · 100 · 1,000 · 100,000", cx)))
                .child(
                    row()
                        .gap_6()
                        .items_start()
                        .child(boxed("Role chip, 140px", 140., ui::role_chip("Night shift moderators and trial helpers", palette::oklch(0.65, 0.2, 5.), cx), cx))
                        .child(boxed("Empty role name", 140., ui::role_chip("", palette::oklch(0.65, 0.2, 5.), cx), cx))
                        .child(boxed("Tag, 100px", 100., ui::Badge::new("Verified publisher").variant(ui::BadgeVariant::Accent), cx))
                        .child(boxed("Key hint, 160px", 160., ui::key_hint(ui::KbdGroup::new(["⌃", "⌥", "⇧", "⌘", "K"]), "Open the quick switcher", cx), cx)),
                ),
        ),
    )
}

fn faces_edges(cx: &mut Context<DesignSystem>) -> Div {
    let page = palette::surface(Level::Page, cx);
    let names = ["", "x", "🌸 Sakura", "田中 太郎", "Ωmega", "  leading space", LONG];
    edges("Faces with no photo and worst-case names", "The initial comes from the first character of any script; an empty name still draws a face; counts cap; a stack keeps its size however many people it holds.", cx).child(
        board(cx).child(
            ui::col()
                .gap_4()
                .child(row().gap_4().items_end().children(names.into_iter().map(|name| ui::AvatarFallback::new(name, ui::AVATAR_XL))).child(note("empty · one letter · emoji · CJK · Greek · leading space · long", cx)))
                .child(row().gap_6().children([1u32, 9, 99, 100, 1_000].map(|n| ui::badged(ui::AvatarFallback::new("Mira", ui::AVATAR_XL), ui::AVATAR_XL, ui::Shape::Round, page).count(n))).child(note("count on a face: 1 · 9 · 99 · 100 · 1,000", cx)))
                .child(boxed("A name beside its face, 200px", 200., ui::row().gap_2().child(ui::AvatarFallback::new("Mira", ui::AVATAR_MD).flex_none()).child(ui::line(LONG).flex_1()).child(div().flex_none().child(ui::Badge::count(120))), cx)),
        ),
    )
}

fn feedback_edges(page: Page, cx: &mut Context<DesignSystem>) -> Div {
    let cell = match page {
        Page::Progress => row().gap_6().items_start().child(boxed("0%", 160., ui::Progress::new(0.), cx)).child(boxed("1%", 160., ui::Progress::new(0.01), cx)).child(boxed("100%", 160., ui::Progress::new(1.), cx)).child(boxed("Ring 0 and 1", 100., row().child(ui::ring(0., cx)).child(ui::ring(1., cx)), cx)),
        Page::Alert => boxed(
            "Callout, 300px",
            300.,
            ui::Alert::new(Notice::Danger, Glyph::XCircle, "Couldn't send 2 messages to a channel that no longer exists", "Waffle lost the connection while sending and the channel was deleted in the meantime, so they can't be delivered.", cx),
            cx,
        ),
        Page::Sonner => boxed("Toast, 260px", 260., ui::Sonner::toast("e-toast", Glyph::Checkmark, palette::positive_ink(cx), "Invite link copied to your clipboard and ready to paste anywhere", Some("It expires in 7 days unless you change it.".into()), cx), cx),
        _ => boxed("Empty state, 240px", 240., ui::Empty::new("e-empty", Mood::Idle, "Nobody you know is playing anything right now, anywhere", "Invite a friend to play, or start something yourself and they'll see it here.", cx), cx),
    };
    edges("In a tight space", "Progress at 0%, a sliver and 100%; a callout and a toast with more words than width; an empty state with a long title.", cx).child(board(cx).child(row().items_start().child(cell)))
}

fn surfaces_edges(cx: &mut Context<DesignSystem>) -> Div {
    let rows = ["Mark as unread", LONG, "Copy message link to the clipboard for sharing outside Waffle"];
    edges("Menus with long rows", "A menu keeps its width: long labels truncate, the leading icon and trailing mark keep their place.", cx).child(
        board(cx).child(row().gap_6().items_start().child(boxed("Menu, 200px", 200., ui::PopoverContent::new(cx).children(rows.into_iter().enumerate().map(|(i, text)| ui::DropdownMenuItem::new(("e-menu", i), cx).child(glyph(Glyph::Copy)).child(ui::line(text).flex_1()).child(ui::Checkbox::mark(Check::On).size(ui::ICON_LG)))), cx))),
    )
}

fn dialog_edges(window: &mut Window, cx: &mut Context<DesignSystem>) -> Div {
    let body = ui::DialogContent::new()
        .child(ui::dialog_header("e-dialog-head", LONG).subtitle("# a-channel-with-a-very-long-name-that-goes-on-and-on").lead(ui::server_tile(None, "Night Shift", ui::AVATAR_LG, cx)).on_close(|_, _, _| {}))
        .child(div().text_color(palette::muted(cx)).child("A worst-case body: a header whose title and subtitle are longer than the dialog is wide."))
        .child(row().justify_end().child(ui::Button::new("e-dialog-cancel").variant(ui::ButtonVariant::Secondary).child("Cancel")).child(ui::Button::new("e-dialog-ok").child("Send invite to everyone in the server")));
    edges("Dialog with a header and footer too long for it", "The title and subtitle truncate beside the close button; the footer's buttons stay inside the card.", cx).child(board(cx).child(ui::Dialog::new("e-dialog", 360., body, window, cx)))
}
