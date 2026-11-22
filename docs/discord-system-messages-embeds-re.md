# Discord system messages and embeds (reverse-engineered)

**Source:** the public web client, not the desktop cache. macOS TCC blocked `~/Library/Application Support/discord/` with "Operation not permitted", even with the sandbox disabled.

- Build: `GLOBAL_ENV.BUILD_NUMBER = "678976"`, `VERSION_HASH/buildId = e85e837cc69d2996c2dbf21b620a85d06ac15dab`, `RELEASE_CHANNEL = stable`, `BUILT_AT = 1795242588277`. Fetched 2026-11-22 from `https://discord.com/app`. No auth was used.
- Corpus: 366 entry scripts plus all 6,331 lazy chunks from the webpack chunk map (`.u`: `"<hash>.js"`).
- Strings: Discord uses `@discord/intl` with hashed 6-character keys. The en-US strings are `JSON.parse('{"<key>":[ast]}')` blobs in 95 en-US chunks. The main one is `c30b23bab1166ada.js` (webpack chunk 301911). Code references them as `X.t.<key>` / `X.t["<key>"]`, or `X.default.<key>` for other message modules. The key is cited next to every string below.
- Chunk names used below:
  - **entry** = `web.355a76e660bee7a5.js`
  - **SYS** = `2b1d2576a5862cd7.js` (chunk 975933, module 117096 = `SystemMessage`; 888675 = the one-line system-message layout)
  - **ACC** = `2420b0c09d1ff9a5.js` (message accessories: embeds, suppress, thread chip)
- Labels: **observed** means seen in code or strings, with the citation. **Inferred** means reasoning that goes beyond the code that was read.

## ICU notation used below

`<usernameHook>{username}</usernameHook>` is a rich-text tag. The client wraps the text in a component, here the clickable username. `<$link>…</$link>` is an inline link or anchor with an `onClick`. `<$b>` is bold. In the plain-text variants (notifications and previews, entry module 779014 `stringify`), the same strings use `<$link args=[usernameOnClick]>`.

---

# Feature A — system messages

## A.0 Message type enum (observed, entry module with `RECIPIENT_ADD=1`)

```
0 DEFAULT  1 RECIPIENT_ADD  2 RECIPIENT_REMOVE  3 CALL  4 CHANNEL_NAME_CHANGE  5 CHANNEL_ICON_CHANGE
6 CHANNEL_PINNED_MESSAGE  7 USER_JOIN  8 GUILD_BOOST  9/10/11 GUILD_BOOST_TIER_1/2/3  12 CHANNEL_FOLLOW_ADD
13 GUILD_STREAM  14 GUILD_DISCOVERY_DISQUALIFIED  15 GUILD_DISCOVERY_REQUALIFIED
16 GUILD_DISCOVERY_GRACE_PERIOD_INITIAL_WARNING  17 GUILD_DISCOVERY_GRACE_PERIOD_FINAL_WARNING
18 THREAD_CREATED  19 REPLY  20 CHAT_INPUT_COMMAND  21 THREAD_STARTER_MESSAGE  22 GUILD_INVITE_REMINDER
23 CONTEXT_MENU_COMMAND  24 AUTO_MODERATION_ACTION  25 ROLE_SUBSCRIPTION_PURCHASE  26 INTERACTION_PREMIUM_UPSELL
27 STAGE_START  28 STAGE_END  29 STAGE_SPEAKER  30 STAGE_RAISE_HAND  31 STAGE_TOPIC
32 GUILD_APPLICATION_PREMIUM_SUBSCRIPTION  33 PRIVATE_CHANNEL_INTEGRATION_ADDED  34 PRIVATE_CHANNEL_INTEGRATION_REMOVED
35 PREMIUM_REFERRAL  36 GUILD_INCIDENT_ALERT_MODE_ENABLED  37 GUILD_INCIDENT_ALERT_MODE_DISABLED
38 GUILD_INCIDENT_REPORT_RAID  39 GUILD_INCIDENT_REPORT_FALSE_ALARM  40 GUILD_DEADCHAT_REVIVE_PROMPT  41 CUSTOM_GIFT
42 GUILD_GAMING_STATS_PROMPT  44 PURCHASE_NOTIFICATION  45 VOICE_HANGOUT_INVITE  46 POLL_RESULT  47 CHANGELOG
48 NITRO_NOTIFICATION  49 CHANNEL_LINKED_TO_LOBBY  50 GIFTING_PROMPT  51 IN_GAME_MESSAGE_NUX
52/53/54 GUILD_JOIN_REQUEST_ACCEPT/REJECT/WITHDRAWN_NOTIFICATION  55 HD_STREAMING_UPGRADED
56 CHAT_WALLPAPER_SET  57 CHAT_WALLPAPER_REMOVED  58..62 REPORT_TO_MOD_DELETED_MESSAGE/TIMEOUT_USER/KICK_USER/BAN_USER/CLOSED_REPORT
63 EMOJI_ADDED  64 PREMIUM_GROUP_INVITE  65 VOICE_SESSION  67 FRIEND_REQUEST_ACCEPTED  68 MEDIA_MENTION_MESSAGE
69 GUILD_SPACE_MESSAGE
```

Type sets (observed, same module):

```js
USER_MESSAGE:new Set([0,19,20,23,26,41,45,47,68]), UNDELETABLE:new Set([1,2,3,4,5,21,35,56,57,64,68]),
REPLYABLE:new Set([0,7,19,20,23,24,25,32,...,46,69]), NON_COLLAPSIBLE:new Set([24]), NON_PARSED:new Set([4,12,18,27,28,31]),
NOTIFIABLE_SYSTEM_MESSAGE:new Set([7,22,35,46,64]), REFERENCED_MESSAGE_AVAILABLE:new Set([19,21,23]), AUTOMOD_INCIDENT_ACTIONS:new Set([36,37,38,39])
```

## A.1 Compact one-line notice vs full message (observed)

- `isSystemMessage(m) = !USER_MESSAGE.has(m.type)` (entry module 143413). Every type outside `USER_MESSAGE` goes to the `SystemMessage` renderer (SYS 117096). That renderer looks up a type→component map `la` and renders it inside `childrenSystemMessage`.
- Almost every component uses the **one-line layout** (SYS module 888675):

  ```js
  function c(e){let{icon:t,iconNode:n,timestamp:s,...}=e;return jsxs("div",{className:l()(c,{[o.kL]:!0,[o.oE]:g,[o.E]:!g}),children:[n&&<div className=o.zc>{n}</div>, t&&<div style={{backgroundImage:`url('${t}')`}}/>, <div>{children}{timestamp}</div>]
  c.Action = ({onClick,children}) => <span>"—"<Anchor onClick>{children}</Anchor></span>
  ```

  The layout is: an 18px icon gutter (`icon` is an SVG URL background, `iconNode` a React icon), then the sentence, then an inline timestamp. `.Action` appends an em-dash and a link, as in "… started a call. — Join the call". `compact` only toggles a class.
- **Full message-like layout**, with avatar, a blue **AutoMod** name (`hG1StD` "AutoMod"), a `SYSTEM` tag and a body: type 24 (AutoMod action), 36/37 (security actions), 38 (raid report) and 39 (false alarm). They use the AutoMod avatar (`nq(!0)`) and `T.A` with `iconNode: <avatar>`.
- **Card under the line:** type 8 GUILD_BOOST shows a boost card when an experiment is on. Type 46 POLL_RESULT shows a result card. Type 25 shows a welcome badge and a sticker button. Type 7 shows the "Wave to say hi!" button. Type 44 shows a "Thank you" badge.
- **Normal message:** type 21 THREAD_STARTER_MESSAGE renders the *referenced* message as a full message (see A.21).
- `la[type] === undefined` → no component. The renderer then reports `Error("unknown message type …")` and renders `null`. The undefined entries are 0, 19, 20, 22, 23, 26, 40, 41, 42, 45, 47, 55, 56, 57, 63 and 68. Of these, 0/19/20/23/26/41/45/47/68 are user messages and never reach this renderer. In this web build, **22 GUILD_INVITE_REMINDER, 40, 42, 55, 56, 57 and 63 render nothing in channel** (observed). Type 22 only has a plain-text form, used in notifications and previews (A.22).
- System messages never render embeds. Accessories use `renderEmbeds = setting && !isSystemMessage(message)` (ACC `cK`).

## A.2 Icons and colors (observed)

`icon:` asset modules in SYS resolve to `/assets/<hash>.svg`. The files were downloaded and read:

| module | file | shape / color | used by |
|---|---|---|---|
| 617184 | `8b9de960da063cb9.svg` | 18×18 right arrow →, `#3ba55c` green | 1 RECIPIENT_ADD (group), thread member add, 7 USER_JOIN, bot join, 12 CHANNEL_FOLLOW_ADD, 25, 33, 44, 52 |
| 884797 | `53e42368b1409f1a.svg` | 18×18 left arrow ←, `#ed4245` red | 2 RECIPIENT_REMOVE (left/removed), thread member remove, 34, 53, 54 |
| 175476 | `61ccab047dad737a.svg` | phone handset, `#3ba55c` | 3 CALL (not missed) |
| 130158 | `6f076a64de25de4b.svg` | phone handset, `#99aab5` grey | 3 CALL (missed) |
| 290388 | `49dd33256719a93e.svg` | 24×24 ✕, `#ed4245` | 14 DISCOVERY_DISQUALIFIED |
| 222548 | `5c56b22c17556059.svg` | 24×24 checkmark, `#3ba55c` | 15 DISCOVERY_REQUALIFIED |

Icon components (`iconNode`):

| Icon | Color | Used by |
|---|---|---|
| `PencilIcon` xs | currentColor | 4 name change, 5 icon change |
| `PinIcon` sm | currentColor | 6 pinned message |
| `BoostGemIcon` | `GUILD_BOOSTING_PINK` | 8–11 boosts |
| `ThreadIcon` md | currentColor | 18 thread created, 21 fallback |
| `StageIcon` md | `GREEN_360` | 27 stage start |
| `StageIcon` | currentColor | 28 stage end, 31 stage topic |
| `MicrophoneArrowRightIcon` | — | 29 speaker |
| `HandRequestSpeakIcon` | — | 30 raise hand |
| `TicketIcon` md | — | 32 app premium subscription |
| `CircleErrorIcon` 20px | — | 16/17 discovery warnings |
| `PollsIcon` xs | — | 46 poll result |
| `UserCheckIcon` | `STATUS_POSITIVE` | 67 friend request accepted |
| `VoiceNormalIcon` | `STATUS_POSITIVE` | 65 voice session |
| `RefreshIcon` | `STATUS_POSITIVE` | 49 channel linked to lobby |
| `GameControllerIcon` | — | 51 in-game message NUX |
| `HammerIcon` | — | 58–62 report-to-mod: ban |
| `UserMinusIcon` | — | 58–62 report-to-mod: kick |
| `ClockWarningIcon` | — | 58–62 report-to-mod: timeout |
| `TrashIcon` | — | 58–62 report-to-mod: deleted message |
| `CheckmarkSmallIcon` | — | 58–62 report-to-mod: closed report |
| `ScreenXIcon` / live icon | — | 13 GUILD_STREAM (ended / live) |
| `TrophyIcon`, `MusicIcon`, `PaintbrushThinIcon` | — | 69 guild space (leaderboard / music / whiteboard) |

## A.3 Copy per type (observed; SYS chunk unless noted; key in brackets)

In the copy below, `**[username]**` is the clickable username. It is a `usernameHook` built by `(0,j.P)({user,channelId,guildId,messageId})`. **Inferred:** it opens the user's profile popout, going by its name and arguments; the hook body was not read.

### 1 RECIPIENT_ADD — icon: green →
- Group DM [`MMN2Jq`]: "**[username]** added **[otherUsername]** to the group." Both names are clickable. The other user is `mentions[0]`.
- In a thread [`tusv2h`]: "**[actorName]** added **[targetName]** to the thread."

### 2 RECIPIENT_REMOVE — icon: red ←
- Someone else removed [`L2FyVq`]: "**[username]** removed **[otherUsername]** from the group." Used when `mentions[0]` exists and is not the author.
- The author left [`5v2xa8`]: "**[username]** left the group."
- In a thread [`32QI5/`]: "**[actorName]** removed **[targetName]** from the thread."

### 3 CALL
`missed = !callActive && call && !call.participants.includes(me)`; `joinable = callActive && I'm not already in it`.

- Missed, with duration [`AcqBmO`]: "You missed a call from **[username]** that lasted {callDuration}." Without duration [`43phHx`]: "You missed a call from **[username]**." Icon: grey phone.
- Otherwise [`7TeC1P`]: "**[username]** started a call that lasted {callDuration}." Or [`LuB5RD`]: "**[username]** started a call." Icon: green phone. When joinable, it adds "— **Join the call**" [`oa9mvZ`], which runs `selectVoiceChannel(channelId)`.

### 4 CHANNEL_NAME_CHANGE — PencilIcon
- Guild channel [`oItgEw`]: "**[username]** changed the channel name: **{channelName}**". `content` is the new name.
- Forum post [`SOQ4hJ`]: "**[username]** changed the post title: **{channelName}**"
- Group DM [`geP3rU`]: "**[username]** changed the group name: **{channelName}**. Edit Group". When `content === ""` [`AI1ZZr`]: "**[username]** removed the custom group name. Edit Group". "Edit Group" [`5Q9+/L`] is a `text-link` span that opens the Edit Group modal.

### 5 CHANNEL_ICON_CHANGE — PencilIcon
[`4wLp25`]: "**[username]** changed the group icon. Edit Group". "Edit Group" is a link to the edit-group modal.

### 6 CHANNEL_PINNED_MESSAGE — PinIcon sm (not in the requested list; included)
- With `messageReference` [`yIDvPL`]: "**[username]** pinned [a message] to this channel. See all [pinned messages]."
  - "a message" runs `jumpToMessage({channelId, messageId, flash:true})`.
  - "pinned messages" switches to the channel if needed, then dispatches `TOGGLE_CHANNEL_PINS`, which opens the pins popout.
- In the overlay (`__OVERLAY__`) there is no pins link [`lD5tup`]: "… pinned [a message] to this channel."
- Without a reference [`R7vZGZ`] / [`vfkjqx`]: "**[username]** pinned a message to this channel. See all [pinned messages]." / "… pinned a message to this channel."

### 7 USER_JOIN — green → (see A.4 for the variants and A.5 for the wave button)
- Human: `su` renders the chosen welcome line with the username hook. `tX` then appends the wave button.
- Bot (`author.bot`) [`/qIck6`]: "**[username]** has added **[appName]** to the server." `username` is `message.actor`, the user who added the bot. With no actor [`LlJum4`]: "**[appName]** was added to the server." The app name hook also renders a bot tag.

### 8–11 GUILD_BOOST / TIER_1..3 — BoostGemIcon (`GUILD_BOOSTING_PINK`)
`numSubscriptions = Number(content)`, or 1 when that is NaN or 0.

- Without a tier (or the guild is unknown):
  - [`yfC9ds`] "**[username]** just boosted the server **{n}** times!" (n > 1)
  - [`57St/7`] "**[username]** just boosted the server!"
- Tier reached:
  - [`PO9uJD`] "**[username]** just boosted the server **{n}** times! {guildName} has achieved **{newTierName}!**"
  - [`cUfTTE`] "**[username]** just boosted the server! {guildName} has achieved **{newTierName}!**"
- Clicking the line opens the boost upsell (`BOOST_ANNOUNCEMENT_UPSELL`). Hovering the gem fires confetti. A 1-in-50 random hover plays a "falling leaf" easter egg, which leads to the Poggermode settings panel.
- Experiment `2026-03-new-server-boost-system-messages`: for type 8 only, and not in compact mode, it renders a card instead:
  - Title, first booster (flag `IS_FIRST_BOOSTER`) [`/1IQGD`]: "**@[username]** was the first to Boost this server!"
  - Title, otherwise [`SbSWvY`]: "**@[username]** Boosted this server {n, plural, one {# time} other {# times}}"
  - Body [`dWm8Iu`]: "Server Boosts help unlock perks for everyone in the server. Learn more"
  - Body when `showCta` is false [`0Mdw0t`]: "Server Boosts help unlock perks for everyone in the server."
  - Button "Boost This Server" [`gKmQ1G`], with a BoostGem icon.
- Plain-text form (entry) [`ihxM9x`]: "{username} just boosted the server!"

### 12 CHANNEL_FOLLOW_ADD — green →
[`47CZc2`]: "**[username]** has added **{webhookName}** to this channel. Its most important updates will show up here."

`webhookName` is `content`. It opens a guild popout for `messageReference.guild_id`, with the guild name taken from `content` minus its last word.

### 13 GUILD_STREAM (extra)
- Live [`FKXvaI`]: "**[username]** is live! Now streaming {activityName}. Join Stream". `activityName` falls back to "unknown".
- Ended [`NEFxtd`]: "**[username]** was live for {duration} in {channelName}"

### 14 GUILD_DISCOVERY_DISQUALIFIED — red ✕
[`bYdG3f`]: "[{guildName}] has been removed from Server Discovery because it no longer passes all the requirements. Check [Server Settings] for more details."

- `guildName` falls back to the literal `"this server"`.
- The guild name selects the guild.
- "Server Settings" opens guild settings at `ACCESS` / `ACCESS_DISCOVERABLE`.

### 15 GUILD_DISCOVERY_REQUALIFIED — green ✓
[`tu6tOR`]: "This server is eligible for Server Discovery again and has been automatically relisted!"

### 16 GRACE_PERIOD_INITIAL_WARNING — CircleErrorIcon
[`vzfXql`]: "[{guildName}] has failed Discovery activity requirements for 1 week. If it fails for 4 weeks in a row, it will be automatically removed from Discovery."

### 17 GRACE_PERIOD_FINAL_WARNING — CircleErrorIcon
[`/TfvyR`]: "[{guildName}] has failed Discovery activity requirements for 3 weeks in a row. If it fails for 1 more week, it will be removed from Discovery."

### 18 THREAD_CREATED — ThreadIcon md
[`cZ9Uf2`]: "**[actorName]** started a thread: **[{threadName}]**. See all [threads]."

- `threadName` is the name of the channel `messageReference.channel_id`, falling back to `content`.
- Clicking the thread name runs `loadThread(id)` and then `openThread(channel, e.shiftKey)`. **Inferred:** shift opens it differently, for example full view instead of the sidebar.
- Right-clicking the thread name opens the thread channel context menu.
- "threads" opens the threads browser modal for the parent channel (lazy module 126768).
- Plain-text form for type 21 (entry) [`B8H+Cl`]: "{username} started a thread, \"{threadName}\"".

**Thread chip** (ACC, the accessory on the parent message when it has the `HAS_THREAD` flag; observed):
- Layout: a "Thread" [`7Xm5QI`] label, the thread name in bold, then the count [`rfAXDV`] "{count, plural, one {# Message ›} other {# Messages ›}}", or "See Thread ›" [`eXHkhl`] when the count is 0.
- Count formatting: threads created before snowflake `992549565104128000` cap at "50+". Otherwise counts of 100,000 or more show as "100k+" (module 321045).
- Second line options:
  - The most recent message: 16px avatar, name and one-line content preview. When the thread is a "view in channel" type, the preview is replaced with "View in channel" [`5uaI/7`].
  - Archived [`ZTo4HS`]: "There are no recent messages in this thread."
  - Count 0 [`HYtNyE`]: "There are no messages in this thread yet."
- Click, or Enter/Space, opens the thread. Shift+click opens it the shift way. Right-click opens the thread context menu. `aria-roledescription` is "Open Thread Button" [`8ipxiY`].

### 21 THREAD_STARTER_MESSAGE (observed, lazy message-list chunk, function `nJ`)
- When `getMessageByReference(messageReference).state === LOADED`, it renders the **referenced parent message** as a full message (`nX`). That rendering has:
  - a "Jump" [`k5WiPf`] button that selects the parent channel and message
  - `renderSuppressEmbeds:false`, `renderReactions:false`, `renderThreadAccessory:false`, `disableComponentInteractivity:true`
  - reply previews shown for REPLY parents
- Not loaded or deleted: it falls back to the system line, ThreadIcon plus [`mE3KJN`] "Original message was deleted" (state DELETED) or [`OCs36J`] "Sorry, we couldn't load the first message in this thread".
- Undeletable: 21 is in the `UNDELETABLE` set.

### 22 GUILD_INVITE_REMINDER
- No in-channel component in this web build (`la[22] = void 0`; observed).
- Plain-text and notification form (entry) [`gxyKvr`]: "Wondering who to invite? Start by inviting anyone who can help you build the server!"

### 24 AUTO_MODERATION_ACTION — full message-like layout (see A.6)

### 25 ROLE_SUBSCRIPTION_PURCHASE — green →
Copy (entry module 942075):

| Case | Key | Copy |
|---|---|---|
| has months, renewal | `Iy66M7` | "**[username]** renewed **{tierName}** and has been a subscriber of [{guildName}] for {months, plural, one {# month} other {# months}}!" |
| has months, new | `eCgb2W` | "**[username]** joined **{tierName}** and has been a subscriber of [{guildName}] for {months …}!" |
| no months, renewal | `mPTTdv` | "**[username]** renewed **{tierName}** in their [{guildName}] membership!" |
| no months, new | `mYjFFx` | "**[username]** joined **{tierName}** as a subscriber of [{guildName}]!" |

- The guild name opens the guild's Role Subscriptions page.
- When `total_months_subscribed <= 1`, a welcome badge follows, with aria text [`utf8+W`] "A special access badge containing {username}'s avatar. Next to it reads, \"Welcome, {username}!\"" and the heading [`MLKSln`] "Welcome,\n**{username}**".
- Then a sticker button, unless the guild has `SUPPRESS_ROLE_SUBSCRIPTION_PURCHASE_NOTIFICATION_REPLIES`:
  - Stickers: 5 Choco stickers. The ids are 781323471249604648 Surprise, 781324642736144424 Affection, 781323769960202280 OMG, 781324722394103808 Cheer and 813951723822645278 Nice.
  - Label, new purchase: ["Welcome them aboard!" `b6wEe6`, "Wave to say hi!" `i8o9hX`].
  - Label, renewal: ["Say congrats!" `vqnToc`, "Give them a high five!" `9yh+dM`].
  - Both the sticker and the label are picked by `(ts(me.id) + ts(msg.id)) % len` (A.5) and sent with `sendGreetMessage`.

### 27 STAGE_START — StageIcon green
- [`Rv+TSM`]: "**[username]** started **{topic}**". `topic` is `content`.
- When the active scheduled event's name equals `content`, a guild-event invite embed (`code: guildId-eventId`) follows.

### 28 STAGE_END — StageIcon
[`zla/ux`]: "**[username]** ended **{topic}**"

### 29 STAGE_SPEAKER — MicrophoneArrowRightIcon
[`dKW5C+`]: "**[username]** is now a speaker."

### 30 STAGE_RAISE_HAND — HandRequestSpeakIcon
- [`tv2DNy`]: "**[username]** requested to speak."
- Users with MUTE_MEMBERS, when the request is still pending, see an "Invite to Speak" [`f0T7hI`] button. It approves the request and deletes this message.

### 31 STAGE_TOPIC — StageIcon
[`Yy6vLs`]: "**[username]** changed the Stage topic: **{topic}**"

### 32 GUILD_APPLICATION_PREMIUM_SUBSCRIPTION — TicketIcon
- [`dXdgno`]: "**[username]** upgraded {applicationName} to premium for this server! 🎉"
- Deleted app [`O829Uu`]: "**[username]** upgraded a deleted application to premium for this server! 🎉"

### 33/34 PRIVATE_CHANNEL_INTEGRATION_ADDED/REMOVED — green → / red ←
Entry module 552691:

| Case | Key | Copy |
|---|---|---|
| added | `J8SaGy` | "**[username]** added the **[otherUsername]** app. See our [help center] for more info." |
| added, deleted app | `+6V2sd` | "**[username]** added a deleted application. See our [help center] for more info." |
| removed | `eGCDak` | "**[username]** removed the **[otherUsername]** app. See our [help center] for more info." |
| removed, deleted app | `sAX6rs` | "**[username]** removed a deleted application. See our [help center] for more info." |

### 36/37 GUILD_INCIDENT_ALERT_MODE_ENABLED/DISABLED — AutoMod-styled full layout
- Enabled [`+m8eD7`]: "**[username]** enabled security actions until {time}." `time` is `new Date(content)` formatted as `{hour, minute, month, day, year}`.
- Disabled [`BHeke+`]: "**[username]** disabled security actions."
- Plain-text forms: [`iOuWPk`] "{username} enabled security actions for {guildName} until {time}" and [`axmbpm`] "{username} disabled security actions for {guildName}".

### 38 GUILD_INCIDENT_REPORT_RAID — AutoMod layout
[`W0UBI3`]: "**[username]** reported a raid in {guildName}."

### 39 GUILD_INCIDENT_REPORT_FALSE_ALARM — AutoMod layout
[`qntXNQ`]: "**[username]** resolved an Activity Alert."

### 44 PURCHASE_NOTIFICATION — green →
- Only rendered when `purchaseNotification.type === GUILD_PRODUCT`.
- [`TQs67g`]: "**[username]** has purchased [{productName}]!" The product name navigates to the guild product listing (`listing_id`).
- A badge follows: aria text [`utf8+W`] and the heading [`52BAtC`] "Thank you,\n**{username}**".

### 46 POLL_RESULT — PollsIcon
- [`VJcK41`]: "**[username]**'s poll [{title}] has closed." `title` is `poll_question_text` truncated to 50 characters. Clicking it runs `jumpToMessage(ref, flash, returnMessageId)`.
- Then a result card built from `embeds[0].fields` raw names: `poll_question_text`, `victor_answer_id`, `victor_answer_text`, `victor_answer_votes`, `total_votes`, `victor_answer_emoji_id/name/animated`.
  - `total_votes === 0` → [`a+lSZ/`] "There was no winner".
  - Victor → the answer text, then [`ufIDIx`] "Winning answer" " • " {round(votes / total × 100)}%.
  - No victor id → [`kPN9si`] "The results were tied" {pct}%.
  - Button: "View Poll" [`Jw7Vbf`].

### Other types (extras found in the enum)
- **49 CHANNEL_LINKED_TO_LOBBY** [`mAtJTC`]: "**[username]** has linked this channel to **{applicationName}**. Messages are now syncing. Learn More"
- **51 IN_GAME_MESSAGE_NUX** [`m7Lwas`]: "**[username]** messaged you from {gameName}. In-game chat may not include rich messaging features such as images, polls, or apps. Learn More"
- **52 GUILD_JOIN_REQUEST_ACCEPT_NOTIFICATION** (green →)
  - [`21R6Ch`] "**[username]'s** application to [{guildName}] was approved! Welcome!"
  - Fallback [`2VLV0d`] "This application was approved! Welcome!"
- **53 GUILD_JOIN_REQUEST_REJECT_NOTIFICATION** (red ←)
  - [`Bz/QC2`] "**[username]'s** application to [{guildName}] was rejected."
  - Fallback [`FVF6qU`] "This application was rejected."
- **54 GUILD_JOIN_REQUEST_WITHDRAWN_NOTIFICATION** (red ←)
  - [`Kpkesg`] "… has been withdrawn."
  - Fallback [`BMlbE7`] "This application has been withdrawn."
- **58–62 REPORT_TO_MOD_*** (`307805` messages module):

  | Type | Key | Copy |
  |---|---|---|
  | ban | `komFTv` | "**[actorName]** banned **[targetName]**" |
  | kick | `FAJmJV` | "**[actorName]** kicked **[targetName]**" |
  | timeout | `90r/sR` | "**[actorName]** timed out **[targetName]**" |
  | deleted message | `4023i8` | "**[actorName]** deleted the message" |
  | closed report | `UE81zV` | "**[actorName]** resolved this flag" |

- **65 VOICE_SESSION** (VoiceNormalIcon green)
  - Ongoing [`eX6e/3`]: "**[username]** started [a voice hangout]."
  - Ended [`YUbgR8`]: "{1–3 names | 2 names and N others} {was|were} hanging out for {duration}."
- **67 FRIEND_REQUEST_ACCEPTED** (UserCheckIcon green)

  | Case | Key | Copy |
  |---|---|---|
  | me, with note | `IquUXd` | "You accepted **[username]**'s friend request. They said: **{note}**" |
  | them, with note | `ZOZy2q` | "**[username]** accepted your friend request. You said: **{note}**" |
  | me | `S6PRsh` | "You accepted **[username]**'s friend request." |
  | them | `H/9Tlj` | "**[username]** accepted your friend request." |

- **69 GUILD_SPACE_MESSAGE**: whiteboard [`Sxxqdx`] "The whiteboard is active." or `fcmI49`; leaderboard with "See the leaderboard" [`O1h3uU`]; music with "Check it out" [`RzWDqY`].
- **Plain-text-only strings** (entry `stringify`, used for notifications and previews):
  - [`DbgSA0`] "{username} started a call."
  - [`XCPMEG`] "… changed the channel name: **{channelName}**"
  - [`wypJZ0`] "… changed the group icon."
  - [`7/Xl0S`] "… added {otherUsername} to the group."
  - [`Qn5+Lf`] "… left the group."
  - [`QtZ0RD`] "… removed {otherUsername} from the group."
  - [`w4iXs+`] "{username} has purchased {productName}!"

## A.4 Join message variants and selection formula (observed)

SYS `su` calls `sd.A.getSystemMessageUserJoin(t.id)` (entry module 779014):

```js
getSystemMessageUserJoin:function(e){let t=[o.t["0cuj7l"],o.t["MuW+CN"],o.t.osqpHX,o.t["5ToSh2"],o.t.JEB8ps,o.t.pkOV5T,
  o.t["kRb1J+"],o.t["EmKLY+"],o.t.rPtBnb,o.t["5B/ekS"],o.t.ESNC3Y,o.t.Iw6d8w,o.t["WecSZ/"]],
  n=S.default.extractTimestamp(e)%t.length;return t[n]}
```

`extractTimestamp` (entry module 363270):

```js
function a(e){return Math.floor(Number(e)/4194304)+14200704e5}
```

**Formula:** `index = (floor(Number(messageId) / 2^22) + 1420070400000) % 13`. That is the message's Unix-ms creation timestamp mod 13.

**Precision note (observed in the code shape):** `Number(id)` is a lossy float64 of the 64-bit snowflake. To match exactly, divide the double, not a BigInt. In practice the result is almost always identical to `(id >> 22) + DISCORD_EPOCH`.

Ordered list (index 0–12). The desktop `usernameHook` variants are shown; `getSystemMessageUserJoinMobile` and `stringify` use the same order with `$link` variants.

| # | key (hook / plain) | copy |
|---|---|---|
| 0 | `0cuj7l` / `Jm6e0x` | "**[username]** joined the party." |
| 1 | `MuW+CN` / `MGRnRT` | "**[username]** is here." |
| 2 | `osqpHX` / `EXOEGh` | "Welcome, **[username]**. We hope you brought pizza." |
| 3 | `5ToSh2` / `5uCTFN` | "A wild **[username]** appeared." |
| 4 | `JEB8ps` / `rl45Qo` | "**[username]** just landed." |
| 5 | `pkOV5T` / `Bh9zpQ` | "**[username]** just slid into the server." |
| 6 | `kRb1J+` / `RdEy1J` | "**[username]** just showed up!" |
| 7 | `EmKLY+` / `qcdp00` | "Welcome **[username]**. Say hi!" |
| 8 | `rPtBnb` / `F7w2Ru` | "**[username]** hopped into the server." |
| 9 | `5B/ekS` / `gSyOgK` | "Everyone welcome **[username]**!" |
| 10 | `ESNC3Y` / `uYgqv7` | "Glad you're here, **[username]**." |
| 11 | `Iw6d8w` / `b/1SBX` | "Good to see you, **[username]**." |
| 12 | `WecSZ/` / `LhebZF` | "Yay you made it, **[username]**!" |

The username is the only link. The icon is the green → arrow (617184).

## A.5 "Wave to say hi!" sticker button (observed, SYS `tX` / `tW`)

**When it shows.** Every condition must hold:

```js
l=X.A.can(SEND_MESSAGES,n), s=isArchivedLockedThreadOrMediaThread(n), a=member(guild,me)?.isPending, r=t.author.bot,
c=(guild.systemChannelFlags & SUPPRESS_JOIN_NOTIFICATION_REPLIES)==0;  return l&&!s&&!a&&!r&&c   // and guildId != null
```

- **Who sees it:** any viewer who can send messages in the channel, is not a pending (membership-screening) member, and is not looking at a media thread or an archived-locked thread without MANAGE_THREADS. The joining user also sees it on their own join message: there is no `author !== me` check.
- **No time limit** in the web code: no timestamp or age check exists in `tX`/`tW`. This is a bounded negative.
- It is hidden for bot joins and when the server disabled "sticker reply" on join notifications (`SUPPRESS_JOIN_NOTIFICATION_REPLIES`).

**Sticker choice.** The six `tZ` stickers:

| id | name | description |
|---|---|---|
| 749054660769218631 | Wave | Wumpus waves hello |
| 751606379340365864 | Wave | Nelly peeks around a wall and waves hello |
| 754108890559283200 | Wave | Clyde cheerfully waving |
| 781291131828699156 | Wave | Choco waves in greeting (format_type 2, APNG) |
| 816087792291282944 | Sup | Doggo spins and greets you |
| 819128604311027752 | Scream | Peach raising her arms in distress |

All are format_type 3 (Lottie) except Choco.

```js
i=((me?extractTimestamp(me.id):0)+extractTimestamp(r.id))%tZ.length
```

So the sticker is per viewer and per message: `(ts(currentUserId) + ts(messageId)) % 6`.

**Button.** `Button variant:"secondary"`, sticker icon, text "Wave to say hi!" [`7Tj6HT`].

**Click.**
1. It validates the send first: `(0,q.i)({type:FORM, content:"", channel})`, which runs slowmode, verification and similar checks.
2. It then sends:

```js
POST /channels/{channel_id}/greet
body: { sticker_ids: [stickerId], allowed_mentions: undefined, message_reference: { guild_id, channel_id, message_id: joinMessageId } }
```

- `getSendMessageOptionsForReply({shouldMention:true})` → `allowedMentions` is undefined, so the server default applies and the reply pings the joiner.
- On success it runs `receiveMessage(channelId, body)` and `STICKER_TRACK_USAGE`.
- On failure it runs `sendClydeError` (unless 429) and `MESSAGE_SEND_FAILED`.
- Analytics: `WELCOME_CTA_CLICKED {is_reply:true, sticker_id, target_user, sender}`.

**After sending.** The button component has no "sent" state, so it stays rendered. Inferred: the result is a new reply message that contains the sticker. The server may reject a repeat, but that is not visible client-side.

## A.6 AutoMod action (24) rendering (observed, SYS `it` / `iN`, entry module 233444)

**Branch.**
- An embed of type `AUTO_MODERATION_NOTIFICATION` → `iN`, the safety-alert card.
- Otherwise → `it`, the blocked or flagged message card.

**Fields.** They are read by `embed.fields[].rawName`:

```
rule_name, channel_id, decision_id, keyword, keyword_matched_content, flagged_message_id, timeout_duration,
quarantine_user, quarantine_user_action, decision_reason, alert_actions_execution, quarantine_event,
block_profile_update_type, voice_channel_status_outcome, application_name, interaction_user_id,
interaction_callback_type, decision_outcome, guild_room_note_outcome
```

The flagged content is `embeds[0].rawDescription`.

**Message card (`it`).** It has the AutoMod avatar, "AutoMod" [`hG1StD`], the `SYSTEM` tag, and a header sentence (`fu`). The `channelHook` links to `channel_id`; if you can't view that channel, it shows [`J90oLW`] "unknown-channel". Header variants:

| Case | Key | Copy |
|---|---|---|
| message flagged | `IZg0VQ` | "has flagged a message in **#{channelName}**" |
| message blocked | `lOIOSK` | "has blocked a message in **#{channelName}**" |
| app message flagged | `AXQufN` | "has flagged **{applicationName}**'s message in **{channelName}**, sent by [owner]" |
| app message blocked | `s3tjMN` | "has blocked **{applicationName}**'s message in **{channelName}**, sent by [owner]" |
| modal flagged | `4xL9Sk` | "has flagged **{app}**'s modal, shown to [user]. The interaction with **{app}** was started by [owner]." |
| modal blocked | `S3lNIT` | "has blocked **{app}**'s modal from being shown to [user]. …" |
| voice channel status | `cLQrqz` / `bma6cs` | "has blocked / has flagged a voice channel status for **{channelName}**" |
| nickname | `t98DPb` / `7u/rlU` | "blocked a nickname update" / "blocked a nickname reset" |
| quarantine | `PmSMMS` / `m9wWzo` / `KNSkC6` / `qV4K6j` | "quarantined a member at message send / server join / username update / tag adoption" |
| guest | `MrYeyS` | "blocked a guest at server join" |

The header sentence uses `"blocked"` vs `"flagged"` from `decision_outcome`.

**Body.**
- The flagged message is rendered like a message with `keyword_matched_content` highlighted.
- Clicking it navigates to `flagged_message_id` in the embed channel, when present.
- Footer line, muted and separated by dots:

  | Key | Copy |
  |---|---|
  | `SYIUTR` | "Keyword: {keyword}" |
  | `ZoOyKB` | "Rule: {ruleName}" |
  | `3LYql6` | "Timeout: {duration}" |
  | `26bB2M` | "Reason: {reason}" |

- `decision_reason` is shown in a box when present.

**Action row.**
- Shield icon plus "Actions" [`DEoVWZ`], which opens the moderate-user context menu.
- "Jump to Surrounding" [`jtkj06`], when an embed `channel_id` exists.
- "Report Issues" [`94JbM3`].
- "Delete User's Message" [`3A52tY`]: needs MANAGE_MESSAGES and a `flagged_message_id`, and the message must not already be deleted per `alert_actions_execution`.
- Executed actions show as icons with tooltips:

  | Icon | Key | Tooltip |
  |---|---|---|
  | trash | `BtKE9h` | "{userName} deleted message {timestamp}" |
  | check | `dyo9US` | "{userName} marked complete {timestamp}" |
  | flag | `C9/kIO` | "{userName} reported issues {timestamp}" |

**Safety alert card (`iN`).** The header is "AutoMod" plus "safety alert!" [`ufawcw`] for raids. The fields are `notification_type`, `join_attempts`, `raid_datetime`, `dms_sent`, `raid_type`, `resolved_reason`, `decision_id` and `suspicious_mention_activity_until`.

- **`raid` (or none):** red ChatWarningIcon. For DM raids it shows CircleErrorIcon with [`8+lHUb`] "Unusual DM Activity Detected"; otherwise [`xMwcwV`] "Unusual Join Activity Detected".
  - Subheader: "~{joinCount} Joins Actioned" [`4ylIiu`], "~{dmsSent} DMs Detected" [`5C8Mh3`], and a relative time.
  - Body [`4QIIZl`]: "Activity began on {dateTime}. Discord is serving CAPTCHA to all joiners and blocks suspicious joins and DMs. Enable security actions and review members, if needed."
  - Footer: "Actions" and a resolve button.
- **`mention_raid`:** [`C2uIXE`] "Unusual Mention Activity Detected" / [`SWIWEV`] "We're restricting some messages due to a potential mention raid." Buttons: "Remove Restrictions" [`oX14El`], "Configure Settings" [`1R7QIx`].
- **`activity_alerts_enabled`:** green [`lVLiFp`] "Activity Alerts Enabled", "Enabled by" @user, and [`QV/8u5`] "New and improved Activity Alerts are monitoring your server safety. AutoMod will send a message here if there is any DM or join activity that exceed what is normal for your server."
- **`interaction_blocked`:** [`2qTBw/`] "App Action Blocked" / [`Dc9wCp`] "The application's response contained content blocked by this server."
- **Unknown:** [`VdZCcC`] "Unsupported Message" / [`NxHYX/`] "Your Discord client doesn’t support this message type. Please update it to see this message."

---

# Feature B — embeds

## B.1 SUPPRESS_EMBEDS (flag 1<<2 = 4)

**Request (observed, entry `suppressEmbeds`):**

```js
async suppressEmbeds(e,t){await ep.A.unarchiveThreadIfNecessary(e);let n=eC.A.getMessage(e,t);null!=n&&s.Bo.patch({url:D.Rsh.MESSAGE(e,t),
  body:{flags:(0,a.lA)(n.flags,D.pr7.SUPPRESS_EMBEDS,!0)},oldFormErrors:!0,rejectWithError:(0,s.fT)()})}
```

The request is `PATCH /channels/{channel_id}/messages/{message_id}` with body `{ "flags": currentFlags | 4 }`. It unarchives the thread first if needed. It is not optimistic: the UI updates from the server's MESSAGE_UPDATE.

**Who sees the ✕ (observed, ACC `cK`):**

```js
j=(me===n.author.id||canManageMessages)&&"1"!==n.author.id&&!1!==renderSuppressEmbeds&&!hasFlag(n.flags,EPHEMERAL)&&isActiveChannelOrUnarchivableThread&&(0,aD.A)(n)>=1
```

All of these must hold:
- You are the author, **or** you have MANAGE_MESSAGES in the channel.
- The author is not the system user (id `"1"`).
- The message is not ephemeral.
- The channel is active or the thread can be unarchived.
- `aD.A(message) >= 1`: the message has **at least one other content part**. The count includes non-empty `content`, an activity, stickers, components, message snapshots or a poll. Embed-only messages, such as webhook or bot embeds with no text, get no ✕.

The ✕ is not shown on thread-starter copies (`renderSuppressEmbeds:false`), forwarded-snapshot previews or other previews (`cW` passes `canSuppressEmbeds:false`).

**The button (observed, entry `renderSuppressButton`):**
- `XSmallIcon` xs, rendered first inside the embed card's grid.
- No tooltip; the `aria-label` is "Remove all embeds" [`GT3fNz`].
- It only appears on **card-style embeds** (`renderEmbedContent`). Inline media embeds (`isInline`: image or video present, and either type `gifv` or non-rich with no author and no title) use `renderInlineMediaEmbed`, which has no ✕. Component-type embeds also receive `onSuppressEmbed`.

**Click behaviour (observed, ACC `handleEmbedSuppressed`):**

```js
handleEmbedSuppressed=e=>{let{channel:t,message:n}=this.props;e.shiftKey?x.A.suppressEmbeds(t.id,n.id):this.setState({showSuppressModal:!0})}
```

- **Shift-click** skips the confirmation.
- A plain click opens a confirm dialog:
  - Header: "Are you sure?" [`VL1KOk`]
  - Body: "This will remove all embeds on this message for everyone." [`vXZ+Fo`]
  - Muted second line: "Hold shift when clearing embeds to skip this modal." [`5j2by3`]
  - Confirm: "Remove All Embeds" [`YEHppG`]. Cancel: "Cancel" [`ETE/oC`].
  - Dismissable.

**Message context-menu item:**
- Not found in this web build.
- "Remove Embeds" [`wUIMqa`] and "Remove Embed" [`4sxKOb`] exist in en-US.
- Neither key is referenced by any of the 6,699 downloaded JS files.
- `suppressEmbeds(` is called only from ACC (the ✕ and the modal).

Bounded negative: there is no "Suppress/Remove embeds" menu item on web/desktop at build 678976. **Inferred:** the strings are used by mobile.

The permission copy references MANAGE_MESSAGES:
- [`RXMG/+`] "Allows members to delete or remove embeds from messages by other members."
- [`FUvPcJ`] "Members with this permission can delete or remove embeds from messages by other members."

## B.2 `<url>` angle-bracket suppression

**Observed (entry markdown rules).** The client parses `<https://…>` with the `autolink` rule `match:/^<([^: >]+:\/[^ >]+)>/`. It produces a normal `link` node (the text without brackets), the same as a bare URL. The client has no embed-generation path for user messages; embeds arrive on the message object from the server.

**Inferred:** the suppression is **server-side**. The unfurler skips URLs wrapped in `<>`, and the client just renders whatever `embeds[]` it gets.

## B.3 Inline video and YouTube play (observed, entry modules 265705 / 693942 / 875149)

**Inline embed predicate (`NV`):**

```js
(null!=image||null!=video)&&(type===GIFV||type!==RICH&&null==author&&null==rawTitle)
```

**Video rendering (`eu`):**
- If `embed.video.proxyURL` is present, the video is a direct file. It renders the native video player.
- Otherwise it is an **iframe-playable provider** and the steps are:
  1. Render the **thumbnail image**, size-fitted with a minimum of 150×144.
  2. Overlay play controls (`875149`):
     - a round **Play** button (`PlayIcon`, aria "Play" [`RscU7I`])
     - an **Open Link** button (custom "open in new window" SVG, aria "Open Link" [`wuRE8M`]) that links to `embed.url` with `target="_blank"`
  3. On play click, set state `playing=true` and replace the thumbnail with an iframe:

```js
<iframe src={x7(video.url, provider, {autoMute})} allow={fs?"autoplay; fullscreen":"autoplay"} frameBorder=0 scrolling="no"
  sandbox="allow-forms allow-modals allow-popups allow-popups-to-escape-sandbox allow-same-origin allow-scripts[ allow-fullscreen]" allowFullScreen/>
```

**URL params (`x7`).** `provider = QY(provider.name, video.url)`: the provider is "YouTube" or "TikTok" by name, or by host `www.youtube.com` / `www.tiktok.com`.

```js
YouTube:{params:{autoplay:"1",auto_play:"1",pageType:"2"},dynamicParams:({autoMute})=>({mute:autoMute?"1":void 0})},
TikTok:{params:{utm_source:"discord.gg"}},
Twitch:{stripParams:["parent"],appendParams:{parent:["discord.co","discord.com","discordapp.com","ptb.discord.com","canary.discord.com","localhost"]},params:{autoplay:"true"},dynamicParams:({autoMute})=>({muted:autoMute?"true":void 0})}
```

- YouTube: the iframe `src` is `embed.video.url`, which is already `https://www.youtube.com/embed/<id>` from the server, plus `autoplay=1&auto_play=1&pageType=2`, plus `&mute=1` when auto-muted. Params are set with `searchParams.set`, so existing ones are overwritten.
- TikTok: postMessage `{type:"play"}` (and `"mute"`) to the player on `onPlayerReady`.
- Other providers: `video.url` unchanged.

**Other playback details:**
- **One at a time.** For iframe videos, play dispatches `VIDEO_EMBED_PLAYBACK_STARTED` and `subscribeOnce` to it. When another iframe embed starts, this one resets to its thumbnail.
- **Which embeds get inline play:** any embed with `embed.video` and a thumbnail (`renderVideo` returns null without `url` and `thumbnail`), when `playable` is true and the embed is not spoilered. The provider only changes the URL params.
- **Sizing:** max media box is 400×300. It is 400×450 for TikTok, and for YouTube when the video is portrait (Shorts).
- **No pop-out or PiP** for embed iframes in this code. The only "out" is the Open Link button. (Bounded: not searched beyond this component.)

**Special providers (observed, `eA.render`):**
- **PlayStation** (session-share hosts) → its own iframe.
- **Spotify:** only when `provider.name==="Spotify"`, `type==="link"`, and the host is `open.spotify.com` (with any `intl-xx/` stripped).
  - The iframe is `https://open.spotify.com/embed/<type>/<id>`.
  - Heights: track 80, episode/show 232, album/artist/playlist 352. Width 300–400.
  - Sandbox: `allow-forms allow-modals allow-popups allow-popups-to-escape-sandbox allow-same-origin allow-scripts`.
  - Old `/user/<u>/playlist/<id>` paths are supported.
- **Apple Music** (`music.apple.com`, article/link) → `https://embed.music.apple.com/<path>?theme=<dark|light>&i=<id>`. Heights: song 175, music-video 371, otherwise 450.
- **Amazon Music** (rich, `music.amazon.*`) → an iframe from the `iframe_url` query param.
- **Twitter/X:** not checked by provider name. Embeds with an `images[]` array render as an image mosaic (`eE`). **Inferred:** that is how multi-image tweets show.

## B.4 Settings that gate embeds (observed, entry module 253932; settings UI in `52b6e35ba37f71f0.js`)

All are proto user settings in `textAndImages`, defaulting to `true`:

| Setting key (proto field) | UI label (key) | Effect |
|---|---|---|
| `textAndImages.renderEmbeds` (`render_embeds`, no.12) | "Show embeds and link previews" [`5bK9vw`], under "Embeds and Link Previews" [`PWZOn4`] | `false` → `renderEmbeds()` returns null, so no embeds at all. Embeds are also always off for system messages. |
| `textAndImages.inlineEmbedMedia` (`inline_embed_media`, no.10) | "When posted as links to chat" [`U47N1p`], in the field set "Show images, videos, and lolcats..." [`9nyle0`] | `false` → embeds render with `hideMedia` (no inline media) |
| `textAndImages.inlineAttachmentMedia` (`inline_attachment_media`, no.9) | "When uploaded directly to Discord" [`VP11No`] | attachments only |
| `textAndImages.gifAutoPlay` (no.11) | "Automatically play GIFs when possible." [`9ptHSs`] | GIFV embeds autoplay |
| `textAndImages.renderReactions` (no.13) | "Show emoji reactions" [`zge/fP`] | — |

Also observed: when `inlineEmbedMedia && renderEmbeds`, message content parsing uses `hideSimpleEmbedContent`. A message that is just a media link hides the URL text.

---

## Not found / limits

- **Desktop cache:** not readable (TCC). Everything comes from the web client at build 678976. Desktop shares this bundle, but **not verified** against the user's installed desktop build.
- **Wave button after send:** there is no client state change. Whether the server enforces one greet per user is unknown.
- **Message context-menu "Remove Embeds":** strings exist but no web code references them (bounded search over all chunks).
- **Type 22 GUILD_INVITE_REMINDER**, and 40, 42, 55, 56, 57, 63: no in-channel renderer in this build. The rendered CTA layout of 22 (if mobile has one) was not found.
- **Username hook behaviour** (`j.P`): **inferred** to open the user profile popout. The hook body was not read.
- **Twitter/X-specific embed handling** beyond the image mosaic was not investigated.
