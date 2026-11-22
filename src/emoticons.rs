// Discord's "Automatically convert emoticons": `:)` and friends become emoji when a message is sent.

// Discord's emoticon table (its web build's EMOJI_SHORTCUT_RE source, read 2026-11-22), by the emoji's
// shortcode. Discord's `slight_smile` and `slight_frown` are spelled as the emojis crate spells them.
const EMOTICONS: &[(&str, &[&str])] = &[
    ("angry", &[">:(", ">:-(", ">=(", ">=-("]),
    ("blush", &[":\")", ":-\")", "=\")", "=-\")"]),
    ("broken_heart", &["</3", "<\\3"]),
    ("confused", &[":-\\", ":-/", "=-\\", "=-/"]),
    ("cry", &[":'(", ":'-(", ":,(", ":,-(", "='(", "='-(", "=,(", "=,-("]),
    ("slightly_frowning_face", &[":(", ":-(", "=(", "=-("]),
    ("heart", &["<3", "♡"]),
    ("imp", &["]:(", "]:-(", "]=(", "]=-("]),
    ("innocent", &["o:)", "O:)", "o:-)", "O:-)", "0:)", "0:-)", "o=)", "O=)", "o=-)", "O=-)", "0=)", "0=-)"]),
    ("joy", &[":'D", ":'-D", ":,D", ":,-D", "='D", "='-D", "=,D", "=,-D"]),
    ("kissing", &[":*", ":-*", "=*", "=-*"]),
    ("laughing", &["x-)", "X-)"]),
    ("neutral_face", &[":|", ":-|", "=|", "=-|"]),
    ("open_mouth", &[":o", ":-o", ":O", ":-O", "=o", "=-o", "=O", "=-O"]),
    ("rage", &[":@", ":-@", "=@", "=-@"]),
    ("smile", &[":D", ":-D", "=D", "=-D"]),
    ("smiling_face_with_tear", &[":')", ":'-)", ":,)", ":,-)", "=')", "='-)", "=,)", "=,-)"]),
    ("slightly_smiling_face", &[":)", ":-)", "=)", "=-)"]),
    ("smiling_imp", &["]:)", "]:-)", "]=)", "]=-)"]),
    ("sob", &[":,'(", ":,'-(", ";(", ";-(", "=,'(", "=,'-("]),
    ("stuck_out_tongue", &[":P", ":-P", "=P", "=-P"]),
    ("sunglasses", &["8-)", "B-)"]),
    ("sweat", &[",:(", ",:-(", ",=(", ",=-("]),
    ("sweat_smile", &[",:)", ",:-)", ",=)", ",=-)"]),
    ("unamused", &[":s", ":-S", ":z", ":-Z", ":$", ":-$", "=s", "=-S", "=z", "=-Z", "=$", "=-$"]),
    ("wink", &[";)", ";-)"]),
];

// Discord's rule: an emoticon converts when it starts the text or follows whitespace, and ends the text or
// is followed by a space or a line break. Code spans and blocks keep theirs.
pub(crate) fn convert(text: &str) -> String {
    crate::chat::outside_code(text, |plain| {
        let mut out = String::with_capacity(plain.len());
        for piece in plain.split_inclusive(char::is_whitespace) {
            let (word, gap) = match piece.char_indices().next_back() {
                Some((at, c)) if c.is_whitespace() => piece.split_at(at),
                _ => (piece, ""),
            };
            match emoji(word).filter(|_| matches!(gap, "" | " " | "\n")) {
                Some(emoji) => out.push_str(emoji),
                None => out.push_str(word),
            }
            out.push_str(gap);
        }
        out
    })
}

fn emoji(emoticon: &str) -> Option<&'static str> {
    let (code, _) = EMOTICONS.iter().find(|(_, faces)| faces.contains(&emoticon))?;
    emojis::get_by_shortcode(code).map(emojis::Emoji::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emoticon_has_an_emoji() {
        for (code, _) in EMOTICONS {
            assert!(emojis::get_by_shortcode(code).is_some(), "{code}");
        }
        assert_eq!(EMOTICONS.iter().map(|(_, faces)| faces.len()).sum::<usize>(), 128, "Discord's table has 128");
    }

    #[test]
    fn converts_whole_words_outside_code() {
        assert_eq!(convert(":) hi :D\n<3"), "🙂 hi 😄\n❤️");
        assert_eq!(convert("ok :("), "ok 🙁");
        assert_eq!(convert("a:) :)b http://x.io/:) :p"), "a:) :)b http://x.io/:) :p");
        assert_eq!(convert(":)\tx"), ":)\tx", "a tab after doesn't end it");
        assert_eq!(convert("`:)` ```\n;)\n``` ;)"), "`:)` ```\n;)\n``` 😉");
    }
}
