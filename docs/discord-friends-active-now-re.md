# Discord Friends: Active Now + Add Friend, reverse-engineered

Source: public web client bundles from `discord.com/app` (2026-11-22, `web.355a76e660bee7a5.js`), with en-US strings from `c30b23bab1166ada.js` and CSS from `css/599618.8b099a3b548d3c11.css`. The desktop cache was unreadable (macOS privacy block). Labels: **observed** = seen in a bundle (file + search term); **inferred** = not seen directly.

## Friends page layout

- Tabs, top-pill style: "Online" (shown if anyone is online), "All" (shown if friends > 0), "Pending" (shown if incoming or outgoing > 0, with a count badge), "Suggestions" (shown if > 0), and "Add Friend" (always shown, with accent styling). **observed** web.js `function A1(`.
- When there are no friends, no pending requests in either direction and no blocks, the page jumps to the Add Friend tab. **observed** same function.
- The header toolbar has a "New Group DM" icon. **observed** `function A0(`.

## Active Now column

Container: `<aside>` with heading "Active Now" (`heading-lg/semibold`). **observed** web.js `function EN(`.

When it hides:
- Pure CSS rule `@media (max-width:1200px){.nowPlayingColumn_zrUwGA{display:none}}`.
- Column sizing: `flex:0 1 30%; min-width:360px; max-width:420px`. **observed** css `599618.8b099a3b548d3c11.css`.
- Stream previews inside cards render only when window width > 1200 and the user has consented to PERSONALIZATION. **observed** web.js module `812808` (`e>1200&&t`).

Personalization consent card: shown above the list until consent is given. Copy includes "This section could be better", "We can customize Active Now, and other parts of Discord, based on information like who you talk to and what games you…", and the buttons "Yes, count me in!" and "No thanks". **observed** web.js near `NOW_PLAYING_CONSENT_CARD`.

States:
- Loading: a spinner.
- Empty: "It's quiet for now..." (`heading-sm/semibold`) followed by "When a friend starts an activity—like playing a game or hanging out on voice—we'll show it here!" (`body-sm`). **observed** web.js `ngJ/5u`, `99ZWxQ`.

### Who and what appears (NowPlayingViewStore, module `943577`)

All **observed** in web.js, `displayName="NowPlayingViewStore"`.
- Candidate users: friends, plus (with personalization consent) affinity users with `communicationRank<=15`. Bots are excluded.
- Users are grouped into "parties" by key, in priority order:
  1. Voice channel: `channel-{id}`, if `VIEW_CHANNEL` is allowed.
  2. Activity party: `party-{party.id}`.
  3. Otherwise `user-{id}`.
- Per party:
  - `currentActivities`: the first non-custom-status activity per user, deduped by application. Kept only if the application resolves. Applications can come from:
    - the application id
    - Spotify (a `party.id` on a Spotify listen)
    - a Twitch or YouTube stream URL
    - an `xbox:{name}` entry
  - Joinable embedded activities are kept only if `CAN_JOIN`.
  - `voiceChannels`: excludes the AFK channel.
  - `applicationStreams`: Go Live streams of party members.
- Filter: keep a party only if it has a voice channel, an activity or a stream. Drop it if any member is blocked or ignored, or if all voice states are `discoverable:false`.
- Sort, using lodash `orderBy`:
  1. Non-discoverable voice last.
  2. Multi-member parties first.
  3. Streams first.
  4. Voice first.
  5. Activities first.
  6. Spotify last.
  7. Then by username.
- Recompute is throttled to 1 s and runs only while the column is mounted (`NOW_PLAYING_MOUNTED`). Application metadata is fetched for unknown app ids.

### Card header (`function _X`)

Title:
- One member: their name.
- Two: "**{user1}** and **{user2}**".
- More: "**{user1}**, **{user2}**, and {extras} other(s)".

Subtitle and icon, when the party is a single user with a single activity:

| Activity type | Subtitle | Icon |
|---|---|---|
| PLAYING (Xbox) | "Playing on Xbox" | Xbox |
| PLAYING | elapsed-time component | app icon |
| STREAMING | "Streaming on Twitch" | app icon |
| LISTENING | "Listening to {name}" | Spotify icon, or app icon |
| WATCHING | "Watching **{name}**" | app icon |
| other | "In a Voice Channel" | none |

Subtitle and icon in other cases:
- No activities, but in voice: "In a Voice Channel".
- No activities, but Spotify only: "Listening to Spotify" with the Spotify icon.
- One activity across several members: elapsed time with the app or Xbox icon.
- Several activities: "Playing Multiple Games" with a multi icon.

Quest-promoted cards: "Playing a Quest · Promoted".

### Card body sections (`function _W`), in order

- **VoiceSection**: guild, channel and member avatars, with partySize.
- **ApplicationStreamingSection**: avatar, name, activity text or "Sharing their screen", a live preview thumbnail (click to join the voice channel and watch), and action text.
- Per activity:
  - **GameSection**: game icon, name, and "{n} Person/People" with a member stack.
  - **RichPresenceSection**: large and small assets, details, state, timestamps, and buttons.
  - **EmbeddedActivitySection**: app tile, elapsed time, participants, and a "Join" button.
  - **TwitchSection**: Twitch icon, name, details, and a 900×500 preview linking out.
  - **SpotifySection**: album art (160), song, artists (`"; "` replaced by `", "`), and a member stack.
- Body text helpers: `u9()` picks the line shown for each activity:
  - CUSTOM_STATUS: its state.
  - When streaming: the game name, otherwise "Sharing their screen".
  - LISTENING / WATCHING / COMPETING / STREAMING / PLAYING: the activity name.
  - HANG_STATUS: nothing.

### Card click popout menu (`function EE`, navId `now-playing-menu`, aria "Actions")

Items, in order:
1. "Message": shown for a single member.
2. "Join Channel" (disabled without CONNECT) and "Go to Server": shown if in voice.
3. "Watch {streamerName}'s Stream": one per stream.
4. Spotify: "Play on Spotify" and "Listen Along".
5. Per game:
   - "Joined", "Join"/"Joining", or "Ask to Join {name}"
   - "Play {name}" / "Play Game", with subtext "{name} is already running." or "Launching {name}."
   - "Connect Xbox"
6. Member list grouped as "{n} People", or "{app} - {n}" sections with "Other Members - {n}", plus "{n} Unknown Player(s)".

The popout opens to the left with 8px spacing. **observed** web.js `navId:"now-playing-menu"`.

## Add Friend tab

All **observed** web.js `function Ek(` / `function Ej(`.
- Heading "Add Friend", then "You can add friends with their Discord username." (with the "Wumpus Waving" image).
- Input placeholder "Enter a username", `maxLength:37`. Typing `#` shows a `#0000` ghost suffix for legacy discriminators.
- Submit button "Send Friend Request", disabled until there is input.
- Note textarea placeholder "Personalize your request (Optional)", `maxLength:120`, with a live counter `120-len` that turns critical at 120.
- Helper under an idle form: "What you write here will also show up in your DM if you become friends."
- Error renders in a `role="alert"` critical line. Success renders in a `role="status"` positive line, and the input clears and refocuses.
- Below the form: "Other Places to Make Friends" with the "Explore Discoverable Servers" button.

### Validation and request (`237309` and `395422` in web.js)

1. The input is trimmed. A leading `@` is stripped when there is no `#`.
2. Client regex `^[a-zA-Z0-9_\\.]+$`, or `^(.+?@.+?\..+?|.+?#\d{4})$` when the input contains `#`. On failure: "Hm, didn’t work. Double check that the username is correct." (no request is sent).
3. A note longer than 120 characters gives "Notes can only contain plain text and must be under 120 characters." Newlines in the note become spaces.
4. `POST /users/@me/relationships` with body `{username, discriminator: parseInt(tag)|NaN, note?}`. **observed** `sendRequest`.
5. Success: "Success! Your friend request to **{discordTag}** was sent."

### Error code → inline copy (`vU`)

| `body.code` | Name | Copy |
|---|---|---|
| 80000 | RELATIONSHIP_INCOMING_DISABLED | "**{discordTag}** is not accepting friend requests. They’ll have to add you to become friends." |
| 30002 | TOO_MANY_FRIENDS | "You've maxed out your friend list. Welcome to the elite 1,000 friends club!" |
| 80007 | RELATIONSHIP_ALREADY_FRIENDS | "You're already friends with that user!" |
| 40068 / 340007 | USER_QUARANTINED / USER_FRIEND_REQUEST_LIMITED_ACCESS | "Your account has limited access" |
| 30059 | TOO_MANY_BLOCKED_USERS | "You've maxed out your block list." |
| 30078 | TOO_MANY_PENDING_OUTGOING | "You've maxed out your pending outgoing friend requests." |
| 80001, 80002, 80003, 80004, anything else | INCOMING_BLOCKED, INVALUD_USER_BOT [sic], INVALID_SELF, INVALID_DISCORD_TAG | "Hm, didn’t work. Double check that the username is correct." |
| `body.note` present | (note validation) | "Notes can only contain plain text and must be under 120 characters." |

Numeric codes are **observed** in the error enum (`er[er.RELATIONSHIP_INCOMING_DISABLED=8e4]`).

### HTTP status → modal (`function R` in module `49229`)

These modals run in addition to the inline text. The Add Friend form passes the default `SHOW_ALWAYS` (**observed**), so it shows both. Whether both are visible at once in practice is **inferred**.

| Status | Modal |
|---|---|
| 429 | Title "WOAH THERE. WAY TOO SPICY", body "You're sending friend requests too quickly!", button "Enter the chill zone" |
| 403 + EMAIL_VERIFICATION_REQUIRED | Title "HOLD UP JUST A SEC.", body "You're going to need to claim your account and verify your e-mail before you can send a friend request.", button "Enter the verification zone." |
| USER_QUARANTINED | Opens the quarantine flow |
| Other errors (unless captcha-handled or `RELATIONSHIP_INVALID_NO_CONFIRMATION`) | Title "Friend request failed", body from the table above, button "Okay" |

### Related relationship calls (web.js `addRelationship`)

- Accept, block, etc.: `PUT /users/@me/relationships/{id}` with body `{type, from_friend_suggestion, confirm_stranger_request, ...captcha}`.
- Clear incoming: confirm modal "Are you sure?", body "This will clear {n} incoming friend request(s)…", then `clearPendingRelationships()`.
