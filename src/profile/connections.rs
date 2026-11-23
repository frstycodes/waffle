// The profile card's linked accounts: their chips, brand colours and where each opens.
use super::*;

pub(super) fn connections(list: &[Connection], ink: Ink, cx: &App) -> Option<Div> {
    if list.is_empty() {
        return None;
    }
    let chips = list.iter().enumerate().map(|(i, c)| connection_chip(i, c, ink, cx));
    Some(section("Connections", ui::row().items_stretch().flex_wrap().gap_1p5().children(chips), ink))
}

// The service's mark on its colour, the account name, Discord's verified seal, and a click to the account.
// lint-allow(custom-component): ui::chip is a palette-toned filter that toggles; this chip wears the card's themed ink and opens the account.
pub(super) fn connection_chip(i: usize, c: &Connection, ink: Ink, cx: &App) -> Stateful<Div> {
    let (platform, color) = platform(&c.kind);
    let url = connection_url(c);
    let tip: SharedString = match url {
        Some(_) => format!("Open on {platform}").into(),
        None => platform.into(),
    };
    let name = if c.name.is_empty() { platform.to_owned() } else { c.name.clone() };
    // The seal takes the info ink on the plain card; on a themed one, the card's own ink reads better.
    let seal = if ink.border.is_some() { ink.strong } else { palette::info_ink(cx) };
    let hover = ink.strong.opacity(0.16);
    // lint-allow(custom-button): ui::chip is a palette-toned filter that toggles; this one wears the card's themed ink and opens the account.
    ui::row()
        .id(("profile-connection", i))
        .h_7()
        .max_w_full()
        .pl_1()
        .pr_2()
        .gap_1p5()
        .squircle(RADIUS_SM)
        .bg(ink.tile)
        .text_size(ui::TEXT_SM)
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink.strong)
        .child(brand_tile(&c.kind, platform, color))
        .child(ui::line(name))
        .when(c.verified, |d| d.child(Icon::new(Glyph::CheckCircle).size(ui::ICON_SM).flex_none().text_color(seal)))
        .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
        .when_some(url, |d, url| d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |_, _, cx| cx.open_url(&url)))
}

// The mark tinted dark or light by its colour; a service without a mark gets its initial.
pub(super) fn brand_tile(kind: &str, platform: &str, color: Option<u32>) -> Div {
    let fill: Hsla = color.map_or_else(|| palette::oklch(0.5, 0.1, ui::name_hue(kind)), palette::data_color);
    let ink: Hsla = if luminance(fill) > 0.5 { palette::ink_on_light() } else { gpui_kit::white() };
    let tile = ui::row().size_5().flex_none().squircle(RADIUS_XS).bg(fill).justify_center().text_color(ink);
    match crate::icons::brand_mark(kind) {
        Some(path) => tile.child(svg().path(path).size(px(BRAND_GLYPH)).text_color(ink)),
        None => tile.text_size(ui::TEXT_XS).font_weight(FontWeight::BOLD).child(platform.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()),
    }
}

// Discord's connection types: name and brand colour (Simple Icons' hex where it has the service).
// Not verified: the names of types outside the RE doc's URL table (Battle.net through Amazon Music).
pub(super) fn platform(kind: &str) -> (&'static str, Option<u32>) {
    match kind {
        "twitch" => ("Twitch", Some(0x9146FF)),
        "youtube" => ("YouTube", Some(0xFF0000)),
        "bluesky" => ("Bluesky", Some(0x0285FF)),
        "steam" => ("Steam", Some(0x000000)),
        "reddit" => ("Reddit", Some(0xFF4500)),
        "twitter" => ("X", Some(0x000000)),
        "twitter_legacy" => ("Twitter", Some(0x000000)),
        "spotify" => ("Spotify", Some(0x1ED760)),
        "github" => ("GitHub", Some(0x181717)),
        "roblox" => ("Roblox", Some(0x000000)),
        "tiktok" => ("TikTok", Some(0x000000)),
        "instagram" => ("Instagram", Some(0xFF0069)),
        "battlenet" => ("Battle.net", Some(0x4381C3)),
        "playstation" | "playstation-stg" => ("PlayStation Network", Some(0x0070D1)),
        "epicgames" => ("Epic Games", Some(0x313131)),
        "riotgames" => ("Riot Games", Some(0xEB0029)),
        "crunchyroll" => ("Crunchyroll", Some(0xF47521)),
        "xbox" => ("Xbox", Some(0x107C10)),
        "ebay" => ("eBay", None),
        "letterboxd" => ("Letterboxd", None),
        "mastodon" => ("Mastodon", None),
        "domain" => ("Domain", None),
        "skype" => ("Skype", None),
        "bungie" => ("Bungie.net", None),
        "leagueoflegends" => ("League of Legends", None),
        "facebook" => ("Facebook", None),
        "samsung" => ("Samsung Galaxy", None),
        "paypal" => ("PayPal", None),
        "amazon-music" => ("Amazon Music", None),
        _ => ("Account", None),
    }
}

// Discord's `getPlatformUserUrl`; None for the services it shows as plain text.
// GAP: Discord warns before opening a `domain` link (it's untrusted); this opens it directly.
pub(super) fn connection_url(c: &Connection) -> Option<String> {
    let (id, name) = (encode_component(&c.id), encode_component(&c.name));
    Some(match c.kind.as_str() {
        "twitch" => format!("https://www.twitch.tv/{name}"),
        "youtube" => format!("https://www.youtube.com/channel/{id}"),
        "bluesky" => format!("https://bsky.app/profile/{}", id.replace("%3A", ":")),
        "steam" => format!("https://steamcommunity.com/profiles/{id}"),
        "reddit" => format!("https://www.reddit.com/u/{name}"),
        "twitter" => format!("https://x.com/{name}"),
        "twitter_legacy" => format!("https://twitter.com/{name}"),
        "spotify" => format!("https://open.spotify.com/user/{id}"),
        "github" => format!("https://github.com/{name}"),
        "roblox" => format!("https://roblox.com/users/{id}/profile"),
        "ebay" => format!("https://www.ebay.com/usr/{name}"),
        "tiktok" => format!("https://www.tiktok.com/@{name}"),
        "instagram" => format!("https://www.instagram.com/{name}"),
        "letterboxd" => format!("https://letterboxd.com/{name}/"),
        "mastodon" if c.id.starts_with("https://") => c.id.clone(),
        "domain" => format!("https://{id}/"),
        _ => return None,
    })
}

// JavaScript's encodeURIComponent: everything but letters, digits and -_.!~*'() is percent-encoded.
pub(super) fn encode_component(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
