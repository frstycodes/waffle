# Illustrations

Where Waffle leads a screen with spot art, as Discord does. Each piece is `assets/illustrations/<name>.png`, drawn by
Codex from `<name>.prompt.txt` beside it with the voice status art as the style reference, scaled to 480px wide.
`ui::empty_state` takes either the mascot (`Mood`) or an `Illustration`; the enum and its asset table live in
`src/illustration.rs`. Discord's Wumpus becomes Waffle in the copy. Every row has a `WAFFLE_DEMO` scene.

| Place | Where | Discord's copy | Art | Demo |
| --- | --- | --- | --- | --- |
| Friends, Online, nobody on | `src/home.rs` friends body | "No one's around to play with Wumpus." | `friends-online` | `friends-empty` |
| Friends, All, no friends | `src/home.rs` | "Wumpus is waiting on friends. You don't have to though!" plus Add Friend | `friends-all` | `friends-all-empty` |
| Friends, Pending, none | `src/home.rs` | "There are no pending friend requests. Here's Wumpus for now." | `friends-pending` | `friends-pending-empty` |
| Friends, Blocked, none | `src/home.rs` | "You can't unblock the Wumpus." | `friends-blocked` | `friends-blocked-empty` |
| Pins popover, none | `src/overlays.rs` `pins_overlay` | "This channel doesn't have any pinned messages... yet." | `pins-empty` | `pins-empty` |
| Search results, none | `src/search_results.rs` `search_body` | "We searched far and wide. Unfortunately, no results were found." | `search-empty` | `search-empty` |
| Inbox, Mentions and Unreads, none | `src/overlays.rs` `mention_list`, `src/inbox.rs` `unread_list` | "You're all caught up!" | `inbox-empty` | `inbox-empty`, `inbox-unreads-empty` |
| Forum, no posts | `src/forum.rs` `render_forum` | Discord shows art over its empty forum; copy not verified, Waffle's kept | `forum-empty` | `forum-empty` |
| Messages failed to load | `src/chat.rs` `render_chat` | Extra: Discord shows an error line, Waffle's own copy kept | `load-failed` | `load-failed` |

Not verified: Discord's exact copy above is from memory of its client, not checked against a current build; the
Inbox and Forum rows are the least certain.

Left with the mascot on purpose: no channel open, no messages yet (Discord shows the channel welcome, not art), the
quick switcher and forward dialog with no matches (Discord shows text only), and loading states.

Not here: the voice channel status dialog (`voice-status`), owned by the voice chat work in `src/call.rs`.

## Style

Polished stylised 3D-ish objects, soft light, three-quarter view, breakfast and café things from Waffle's world.
Honey `#EFA831` and amber `#BF7D06` lead, cream and graphite `#1d1f24` support; no purple or blue, no text, no logos,
no background tile. Readable on dark `#25272b` and light `#fcfcfd`. Art centred above a playful title and one muted
subtitle, at 200x150 in `ui::empty_state`.
