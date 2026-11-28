// The design system's page tree, laid out like a Figma file: one page per component, grouped by what it is.
// Every page carries a summary and its properties (the named variants: `Tone`, `Size`, `State`…); its frames are
// titled `Component / Property`, so a name here is the name in code and in conversation.
use super::*;

macro_rules! pages {
    ($($page:ident, $group:ident, $title:literal, $summary:literal, [$(($prop:literal, $values:literal)),* $(,)?];)*) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(super) enum Page { $($page),* }

        impl Page {
            pub(super) const ALL: &[Page] = &[$(Page::$page),*];

            pub(super) fn title(self) -> &'static str {
                match self { $(Page::$page => $title),* }
            }

            pub(super) fn group(self) -> Group {
                match self { $(Page::$page => Group::$group),* }
            }

            pub(super) fn summary(self) -> &'static str {
                match self { $(Page::$page => $summary),* }
            }

            // The page's named properties and the values each takes: what a Figma component's variants panel lists.
            pub(super) fn props(self) -> &'static [(&'static str, &'static str)] {
                match self { $(Page::$page => &[$(($prop, $values)),*]),* }
            }
        }
    };
}

pages! {
    Colour, Foundations, "Color", "Roles as surface and foreground pairs with measured contrast, then the palette. Graphite neutrals and one honey, light and dark.", [("Surface", "Rail · Sidebar · Page · Raised · Overlay"), ("Ink", "Strong · Default · Muted · Faint · Accent · Danger"), ("Status", "Online · Idle · Dnd · Offline")];
    Type, Foundations, "Typography", "One interface family and a mono for code, on a fixed scale.", [("Size", "XXS · XS · SM · MD · LG · XL"), ("Weight", "Normal · Medium · Semibold")];
    Space, Foundations, "Spacing, radius and elevation", "The spacing scale, the radius ladder and the elevation tiers, each naming what uses it.", [("Radius", "XS · SM · MD · LG · XL"), ("Icon", "XS · SM · MD · LG · XL"), ("Avatar", "XS · SM · MD · LG · XL")];
    Motion, Foundations, "Motion", "Six timings: press, hover, pop, spring, panel, move.", [("Timing", "Press · Hover · Pop · Spring · Panel · Move")];
    Icons, Foundations, "Icons", "Reicon's filled weight; line-only glyphs stay lines.", [("Weight", "Filled · Line")];
    Illustrations, Foundations, "Illustrations", "Spot art that heads dialogs, empty states and first-run, in Waffle's own style.", [("Art", "Every illustration in assets/illustrations")];

    Button, Actions, "Button", "A text button for a verb and its object.", [("Variant", "Default · Secondary · Destructive · Outline · Ghost · Link"), ("Scope", "Page · Media · Art · Stage · Success"), ("Size", "Sm 30 · Default 36 · Lg 44 · IconXs 24 · IconSm 32 · Icon 36 · IconLg 40 · Square n"), ("State", "Rest · Hover · Pressed · Focus · Disabled · Loading · Toggled")];
    ButtonGroup, Actions, "Button group", "A main action and its options chevron, joined.", [("Tone", "Primary · Secondary")];
    Toggle, Actions, "Toggle", "A two-state button for a filter or option; pressed takes the accent tint.", [("State", "Off · Pressed · Removable"), ("Lead", "None · Icon · Custom")];
    Kbd, Actions, "Kbd", "A key you could press, alone or in a shortcut.", [("Kind", "Keycap · Glyph · Translucent · Key hint")];
    Badge, Actions, "Badge", "A small label for what something is, a count that asks for you, or an unread dot.", [("Kind", "Badge · Tag · Tag with glyph · Live")];

    Input, Inputs, "Input", "A single line of text with a label above and help or an error below.", [("State", "Rest · Focus · Filled · Error · Disabled"), ("Slots", "Leading · Trailing"), ("Kind", "Plain · Search · Password · Prefixed")];
    Textarea, Inputs, "Textarea", "Several lines of text with a character count.", [("State", "Rest · Focus · Filled · Error · Disabled")];
    InputGroup, Inputs, "Input group", "A number with minus and plus.", [("State", "Rest · At minimum")];
    Select, Inputs, "Select", "A field that opens a menu of choices.", [("State", "Closed · Open · Disabled"), ("Lead", "None · Icon")];
    Checkbox, Inputs, "Checkbox", "Pick many; a parent shows mixed.", [("State", "Off · On · Mixed"), ("Layout", "Bare · Pick row")];
    RadioGroup, Inputs, "Radio group", "Pick one of a set.", [("State", "Off · On"), ("Layout", "Bare · Pick row")];
    Switch, Inputs, "Switch", "A setting that applies at once, honey when on.", [("State", "Off · On · Pressed"), ("Layout", "Bare · Toggle row with detail")];
    Slider, Inputs, "Slider", "A 0 to 1 value, dragged or stepped by arrow keys.", [("State", "Rest · Hover · Held · Focus")];
    InputOTP, Inputs, "Input OTP", "One digit of a verification code.", [("State", "Empty · Filled · Focused")];

    Tabs, Navigation, "Tabs", "The segmented track and the underline strip switch a view or a value in place.", [("Style", "Underline · Pills"), ("Count", "None · Number")];

    Card, DataDisplay, "Card", "A clickable card: a template, a launcher entry, a choice.", [("State", "Rest · Hover · Chosen"), ("Edge", "Flat · Bordered")];
    Item, DataDisplay, "Item", "A person or thing in a list: media, title, description, actions. ItemMedia is the tile that leads it.", [("Height", "Dense · Regular · Tall · Fit"), ("Lead", "Avatar · Tile · Icon"), ("Slots", "Subtitle · Stamp · Body · Trailing")];
    Avatar, DataDisplay, "Avatar", "A person's photo, or their initial on a hued gradient; AvatarGroup overlaps them and caps with +n.", [("Size", "16 · 20 · 24 · 32 · 40 · 56 · 80 · 120"), ("Source", "Photo · Monogram")];
    Separator, DataDisplay, "Separator", "Separates groups; a label names what follows.", [("Kind", "Plain · Labelled · Seam · Inset rule")];

    Alert, Feedback, "Alert", "An inline notice inside the view it is about; the strip is its compact form.", [("Notice", "Info · Warning · Danger · Success · Accent"), ("Actions", "None · Under the text")];
    Sonner, Feedback, "Sonner", "What just happened, rising in and leaving by itself.", [("Kind", "Text · Detail · With action")];
    Progress, Feedback, "Progress", "Done, a gap, then what's left; or a calm indeterminate sweep.", [("Kind", "Split · Indeterminate · Ring"), ("Weight", "Regular · Thin")];
    Skeleton, Feedback, "Skeleton", "A calm shimmer in the shape of what's loading.", [("Shape", "Line · Block · Circle")];
    Empty, Feedback, "Empty", "Spot art, what's missing, and one action.", [("Art", "Mascot mood · Illustration")];

    Dialog, Overlays, "Dialog", "A card on the raised surface with a header, body and footer.", [("Width", "360 · 440 · 560"), ("Header", "Title · Subtitle · Lead · Close")];
    Popover, Overlays, "Popover", "A floating surface for anything: ringed, shadowed, never behind the pointer.", [("Kind", "Surface · Panel · Floating bar")];
    HoverCard, Overlays, "Hover card", "A list that opens on hover and stays while the pointer is on it.", [("Kind", "Overflow list · Roll")];
    Tooltip, Overlays, "Tooltip", "A one-line label that follows the pointer to a control.", [("Content", "Text · Text with keys")];
    DropdownMenu, Overlays, "Dropdown menu", "A floating surface of rows; the danger row stands apart.", [("Row", "Plain · Icon · Check · Radio · Danger"), ("Kind", "Menu · Context menu")];

    RoleChip, WaffleOwn, "Role chip", "A role: its colour as a dot, the capsule lightly tinted.", [("Kind", "Chip · Removable chip · Pill")];
    Presence, WaffleOwn, "Presence mark", "Status is never colour alone: a dot, a moon, a bar, a ring and more.", [("Status", "Online · Idle · Dnd · Offline · Mobile · Streaming · Typing"), ("Speaking", "Ring")];
    UserAvatar, WaffleOwn, "User avatar", "An avatar with marks on its corners: presence, a count, a voice badge.", [("Shape", "Round · Tile · Control"), ("Spot", "TopLeft · TopRight · BottomLeft · BottomRight")];
    ServerTile, WaffleOwn, "Server tile", "A server's icon as a squircle, or its initials.", [("Icon", "Image · Initials"), ("Marks", "Count · Voice · Stream · Channel kind")];
    AppTile, WaffleOwn, "App tile", "An app's icon, or its initial on a hued gradient.", [("Icon", "Image · Monogram")];
    Meter, WaffleOwn, "Mic meter", "Bars that light up to a level, amber then red near the top.", [("Level", "0 to 1")];
    Swatch, WaffleOwn, "Swatch", "A colour choice, ringed and ticked when chosen.", [("State", "Off · Chosen")];
    DropZone, WaffleOwn, "Drop zone", "Where files land.", [("State", "Rest · Dragging over")];

    DoDont, Guidance, "Do and Don't", "Rendered pairs for the rules that are easiest to break.", [("Rule","One primary per group · Icon buttons carry a tooltip · Destructive only for the destructive step · One accent per surface")];
    Recipes, Guidance, "Recipes", "Waffle's own compositions, built only from tokens and components, then one realistic screen.", [("Recipe","Settings row · Member row · Example screen")];

    CountBadge, Hidden, "Count badge", "A count that asks for you; caps at 99+.", [("Count", "0 · 1 to 99 · 99+"), ("Kind", "Count · Unread dot · Number mark")];
    Strip, Hidden, "Strip", "A compact notice with an action at the row's end.", [("Fill", "Raised · Selected · Accent")];
    AvatarStack, Hidden, "Avatar stack", "Overlapping avatars ringed in the surface, capped with +n.", [("Count", "1 · 3 · many"), ("Cap", "n")];
    IconTile, Hidden, "Icon tile", "A squircle holding an icon, for row leads.", [("Fill", "Selected · Accent · Danger")];
    Segmented, Hidden, "Segmented control", "Switch a view or a value in place.", [("Options", "2 to 4"), ("State", "Selected · Pressed")];
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Group {
    Foundations,
    Actions,
    Inputs,
    Navigation,
    DataDisplay,
    Feedback,
    Overlays,
    WaffleOwn,
    Guidance,
    // Frames shown on another page (Count badge on Badge); not in the navigation.
    Hidden,
}

impl Group {
    pub(super) const ALL: [Group; 9] = [Group::Foundations, Group::Actions, Group::Inputs, Group::Navigation, Group::DataDisplay, Group::Feedback, Group::Overlays, Group::WaffleOwn, Group::Guidance];

    pub(super) fn title(self) -> &'static str {
        match self {
            Group::Foundations => "Foundations",
            Group::Actions => "Actions",
            Group::Inputs => "Inputs",
            Group::Navigation => "Navigation",
            Group::DataDisplay => "Data display",
            Group::Feedback => "Feedback",
            Group::Overlays => "Overlays",
            Group::WaffleOwn => "Waffle's own",
            Group::Guidance => "Guidance",
            Group::Hidden => "",
        }
    }
}

impl Page {
    pub(super) fn glyph(self) -> Glyph {
        match self.group() {
            Group::Foundations => Glyph::Emoji,
            Group::Actions => Glyph::Pin,
            Group::Inputs => Glyph::Search,
            Group::Navigation => Glyph::ChevronRight,
            Group::DataDisplay => Glyph::Users,
            Group::Feedback => Glyph::Bell,
            Group::Overlays => Glyph::Copy,
            Group::WaffleOwn => Glyph::Star,
            Group::Guidance => Glyph::Checkmark,
            Group::Hidden => Glyph::Emoji,
        }
    }

    // The pages whose frames this page also shows.
    pub(super) fn members(self) -> &'static [Page] {
        match self {
            Page::Badge => &[Page::CountBadge],
            Page::Avatar => &[Page::AvatarStack],
            Page::Item => &[Page::IconTile],
            Page::Tabs => &[Page::Segmented],
            Page::Alert => &[Page::Strip],
            _ => &[],
        }
    }

    // What `WAVE_DESIGN=<key>` opens: the first page whose title starts with the key.
    pub(super) fn from_key(key: &str) -> Option<Page> {
        let key = key.to_lowercase();
        Self::ALL.iter().copied().find(|p| p.title().to_lowercase().starts_with(&key))
    }
}

// A frame's kind, in the order a page lists them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Variants,
    States,
    Edges,
    Flows,
}

impl Kind {
    pub(super) const ALL: [Kind; 4] = [Kind::Variants, Kind::States, Kind::Edges, Kind::Flows];

    pub(super) fn label(self) -> &'static str {
        match self {
            Kind::Variants => "Variants",
            Kind::States => "States",
            Kind::Edges => "Edge cases",
            Kind::Flows => "Flows",
        }
    }
}

pub(super) type Frame = (Kind, AnyElement);
