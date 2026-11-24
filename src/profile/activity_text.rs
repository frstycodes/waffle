// What a profile activity card says, by Discord's rules: its header, its three lines and its badges' text.
use crate::discord::{ActivityKind, RichPresence};

pub(crate) fn is_spotify(rich: &RichPresence) -> bool {
    rich.kind == ActivityKind::Listening && rich.name == "Spotify"
}

// The header: "Playing", "Listening to Spotify", "Streaming Twitch", "Watching Crunchyroll" …
pub(crate) fn header(rich: &RichPresence) -> String {
    let named = rich.details.is_some();
    match rich.kind {
        ActivityKind::Streaming if platform(rich).is_some() => format!("Streaming {}", rich.name),
        ActivityKind::Streaming => "Streaming".into(),
        ActivityKind::Listening if named => format!("Listening to {}", rich.name),
        ActivityKind::Listening => "Listening to".into(),
        ActivityKind::Watching if named => format!("Watching {}", rich.name),
        ActivityKind::Watching => "Watching".into(),
        ActivityKind::Competing if named => format!("Competing in {}", rich.name),
        ActivityKind::Competing => "Competing in".into(),
        ActivityKind::Playing | ActivityKind::Custom => "Playing".into(),
    }
}

// The verb before an activity's name in a one-line summary ("Playing Hades").
pub(crate) fn activity_heading(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Streaming => "Streaming",
        ActivityKind::Listening => "Listening to",
        ActivityKind::Watching => "Watching",
        ActivityKind::Competing => "Competing in",
        ActivityKind::Playing | ActivityKind::Custom => "Playing",
    }
}

// The brand whose mark follows the header, as a connection kind (`icons::brand_mark`).
pub(crate) fn platform(rich: &RichPresence) -> Option<&'static str> {
    match rich.name.as_str() {
        _ if is_spotify(rich) => Some("spotify"),
        "Twitch" => Some("twitch"),
        "YouTube" => Some("youtube"),
        "Crunchyroll" => Some("crunchyroll"),
        "Roblox" => Some("roblox"),
        _ => None,
    }
}

// The bold line, then two plain ones. A game leads with its name; anything else with its details. A game's
// state moves into the party badge when it has a party; a show has no third line; the rest end on the large
// image's text (an album).
pub(crate) fn lines(rich: &RichPresence) -> [Option<String>; 3] {
    let playing = rich.kind == ActivityKind::Playing;
    let first = match playing {
        true => Some(rich.name.clone()),
        false => rich.details.clone().or_else(|| Some(rich.name.clone())),
    };
    if is_spotify(rich) {
        return [first, rich.state.as_ref().map(|s| s.replace("; ", ", ")), None];
    }
    let second = if playing { rich.details.clone() } else { rich.state.clone() };
    let third = match rich.kind {
        ActivityKind::Watching => None,
        ActivityKind::Playing => rich.state.clone().filter(|_| rich.party.is_none()),
        _ => rich.large_text.clone(),
    };
    [first, second, third]
}

// A game's party: "{state} ({n} of {max})", or just the count without a state.
pub(crate) fn party(rich: &RichPresence) -> Option<String> {
    let (current, max) = rich.party.filter(|_| rich.kind == ActivityKind::Playing)?;
    let count = if max > 0 { format!("{current} of {max}") } else { current.to_string() };
    Some(match &rich.state {
        Some(state) => format!("{state} ({count})"),
        None => count,
    })
}

// Music and shows with both ends get the progress bar; everything else that started gets the green clock.
pub(crate) fn has_progress(rich: &RichPresence) -> bool {
    matches!(rich.kind, ActivityKind::Listening | ActivityKind::Watching) && rich.started.is_some() && rich.ends.is_some()
}

pub(crate) fn has_timer(rich: &RichPresence) -> bool {
    rich.started.is_some() && !has_progress(rich)
}

// A show's "S2E5", read from a large text like "Season 2, Episode 5".
pub(crate) fn episode(rich: &RichPresence) -> Option<String> {
    let text = rich.large_text.as_deref().filter(|_| rich.kind == ActivityKind::Watching)?;
    let number = |word: &str| text.split(word).nth(1)?.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse::<u32>().ok();
    Some(format!("S{}E{}", number("Season")?, number("Episode")?))
}

// The ends of a track's bar: "00:42", "03:10", hours only when there are some.
pub(crate) fn track_time(secs: u64) -> String {
    match secs / 3600 {
        0 => format!("{:02}:{:02}", secs / 60, secs % 60),
        h => format!("{h:02}:{:02}:{:02}", secs / 60 % 60, secs % 60),
    }
}

// (elapsed, length) in seconds of a track that ends, at `now` (Unix milliseconds).
pub(crate) fn track_position(rich: &RichPresence, now: i64) -> Option<(u64, u64)> {
    let (start, end) = (rich.started?, rich.ends?);
    let length = (end - start).max(0) / 1000;
    Some((((now - start) / 1000).clamp(0, length) as u64, length as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich(kind: ActivityKind, name: &str) -> RichPresence {
        RichPresence { kind, name: name.into(), ..Default::default() }
    }

    #[test]
    fn headers_follow_the_kind() {
        assert_eq!(header(&rich(ActivityKind::Playing, "Hades")), "Playing");
        let spotify = RichPresence { details: Some("Midnight City".into()), ..rich(ActivityKind::Listening, "Spotify") };
        assert_eq!(header(&spotify), "Listening to Spotify");
        assert_eq!(header(&rich(ActivityKind::Listening, "Tidal")), "Listening to");
        assert_eq!(header(&rich(ActivityKind::Streaming, "Twitch")), "Streaming Twitch");
        assert_eq!(header(&rich(ActivityKind::Streaming, "Kick")), "Streaming");
        assert_eq!(header(&RichPresence { details: Some("Frieren".into()), ..rich(ActivityKind::Watching, "Crunchyroll") }), "Watching Crunchyroll");
        assert_eq!(header(&rich(ActivityKind::Competing, "Arena")), "Competing in");
    }

    #[test]
    fn lines_by_kind() {
        let game = RichPresence { details: Some("Normal, Erangel".into()), state: Some("Squad".into()), ..rich(ActivityKind::Playing, "PUBG") };
        assert_eq!(lines(&game), [Some("PUBG".into()), Some("Normal, Erangel".into()), Some("Squad".into())]);
        let party = RichPresence { party: Some((3, 4)), ..game };
        assert_eq!(lines(&party)[2], None, "the state moves into the party badge");
        assert_eq!(super::party(&party).as_deref(), Some("Squad (3 of 4)"));
        let track = RichPresence { details: Some("Midnight City".into()), state: Some("M83; Anthony Gonzalez".into()), large_text: Some("Hurry Up".into()), ..rich(ActivityKind::Listening, "Spotify") };
        assert_eq!(lines(&track), [Some("Midnight City".into()), Some("M83, Anthony Gonzalez".into()), None]);
        let tidal = RichPresence { large_text: Some("Album".into()), ..rich(ActivityKind::Listening, "Tidal") };
        assert_eq!(lines(&tidal), [Some("Tidal".into()), None, Some("Album".into())]);
        let show = RichPresence { details: Some("Frieren".into()), state: Some("Episode 5".into()), large_text: Some("Season 2, Episode 5".into()), ..rich(ActivityKind::Watching, "Crunchyroll") };
        assert_eq!(lines(&show), [Some("Frieren".into()), Some("Episode 5".into()), None]);
        assert_eq!(episode(&show).as_deref(), Some("S2E5"));
    }

    #[test]
    fn party_without_state_or_max() {
        let open = RichPresence { party: Some((2, 0)), ..rich(ActivityKind::Playing, "x") };
        assert_eq!(party(&open).as_deref(), Some("2"));
        assert_eq!(party(&RichPresence { party: Some((1, 5)), ..rich(ActivityKind::Playing, "x") }).as_deref(), Some("1 of 5"));
    }

    #[test]
    fn timer_or_progress() {
        let track = RichPresence { started: Some(0), ends: Some(190_000), ..rich(ActivityKind::Listening, "Spotify") };
        assert!(has_progress(&track) && !has_timer(&track));
        assert_eq!(track_position(&track, 42_500), Some((42, 190)));
        assert_eq!(track_position(&track, 900_000), Some((190, 190)));
        assert!(has_timer(&RichPresence { started: Some(0), ..rich(ActivityKind::Playing, "x") }));
        assert_eq!(track_time(42), "00:42");
        assert_eq!(track_time(3725), "01:02:05");
    }
}
