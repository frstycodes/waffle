# Discord "Add a Server" modal — reverse-engineered

Source: public web client bundles fetched from `https://discord.com/app` on 2026-11-22 (no auth). English copy from the en-US
message chunks (`c30b23bab1166ada.js` and others); `«…»` = resolved i18n string. Bundle names are the `/assets/<hash>.js` files.
**observed** = read in the bundle; **inferred** = deduced, not read directly.

## Flow and entry points

- Modal key `"create-guild"`; slides `guild-templates` → `creation-intent` → `customize-guild`, plus `join-guild` and `channel-prompt`. **observed** (`web.355a76e660bee7a5.js`, module 819638)
- Slide order in the carousel: `[GUILD_TEMPLATES, CREATION_INTENT, CUSTOMIZE_GUILD, CHANNEL_PROMPT, JOIN_GUILD]`; with `joinFirst` the Join slide comes first. **observed** (`6fff93e818e496c4.js`)
- Back from Customize → Intent; Back from Intent/Join → Templates (clears template). **observed** (`6fff93e818e496c4.js`)
- After a successful create, `onGuildCreated` calls `onSuccess` which closes the modal and transitions to the new guild (`transitionToGuildSync`). The `channel-prompt` slide ("Start a conversation", topic field, placeholder «Drawing, climbing, memes, etc.», creates a text channel with topic «This is a good place to talk about {topic}») exists but this path never navigates to it. **observed** code, **inferred** unreachable from this modal (`bc4f97a0d13a0d1d.js`, `6fff93e818e496c4.js`)
- Keyboard shortcut: `mod+shift+n` (Cmd on mac, Ctrl elsewhere), global. If the modal is already open it jumps to the Join slide; otherwise opens it (`joinFirst` depends on an experiment `guild_list_add_server`). Listed in the shortcuts sheet as «Create or join a server». **observed** (`web.355a76e660bee7a5.js`)
- Guild-list button label: «Add a Server». **observed** (message only; component not traced)

## Step 1 — Create Your Server (templates)

Component: module 209322 in `bc4f97a0d13a0d1d.js`. **observed**
- Title: «Create Your Server» (new-user variant: «Create Your First Discord Server»).
- Subtitle: «Your server is where you and your friends hang out.\nMake yours and start talking.»
- Rows, in render order (icon + label + chevron button, autofocus on the first):
  1. «Create My Own»
  2. section label «Start from a template»
  3. «Gaming», «Friends», «Study Group», «School Club», «Local Community», «Artists & Creators»
- Footer: heading «Have an invite already?» + secondary full-width button «Join a Server».
- A `CLUBS` template («Clubs») is defined but not rendered in the list. **observed**

### Template definitions (module 570465, `bc4f97a0d13a0d1d.js`) — **observed**

Channel `id`s are template-local placeholders; `parent_id` points at the category's placeholder id; `system_channel_id` is a placeholder id.

| id | code | label | channels (`[Category]`, `#text`, `V:` voice) | system channel |
|---|---|---|---|---|
| CREATE | 2TffvPucqHkN | Create My Own | `[]` | null |
| GAMING | hvtBQMfw6uSJ | Gaming | [Text Channels] #general #clips-and-highlights · [Voice Channels] V:Lobby V:Gaming | #general |
| FRIENDS | hgM48av5Q69A | Friends | [Text Channels] #general #games #music · [Voice Channels] V:Lounge V:Stream Room | #general |
| STUDY | FbwUwRp4j8Es | Study Group | [Information] #welcome-and-rules #notes-resources · [Text Channels] #general #homework-help #session-planning #off-topic · [Voice Channels] V:Lounge V:Study Room 1 V:Study Room 2 | #general |
| CLUBS | s4WNnBxTDPsY | Clubs (hidden) | [Information] #welcome-and-rules #announcements · [Text Channels] #general #meeting-plans · [Voice Channels] V:Lounge V:Meeting Room | #general |
| CREATORS | 6exdzMgjZgah | Artists & Creators | [Information] #welcome-and-rules #announcements · [Text Channels] #general #events #ideas-and-feedback · [Voice Channels] V:Lounge V:Community Hangout V:Stream Room | #general |
| LOCAL_COMMUNITY | 64UDvRNCC52Y | Local Community | [Information] #welcome-and-rules #announcements #resources · [Text Channels] #general #meeting-plans #off-topic · [Voice Channels] V:Lounge V:Meeting Room | #general |
| SCHOOL_CLUB | Ctg7PUHcQmZu | School Club | [Information] #welcome-and-rules #announcements #resources · [Text Channels] #general #meeting-plans #off-topic · [Voice Channels] V:Lounge V:Meeting Room 1 V:Meeting Room 2 | #general |

- Only SCHOOL_CLUB has `roles`: `@everyone` (default perms) and `officers (example)` — `mentionable`, `hoist`, `color: 3066993`, permissions = default + MANAGE_ROLES, MANAGE_CHANNELS, KICK_MEMBERS, BAN_MEMBERS, MANAGE_NICKNAMES, MANAGE_GUILD_EXPRESSIONS, MANAGE_MESSAGES, SEND_TTS_MESSAGES. **observed**
- Channel/category names are localized strings (lowercase-dashed for text). **observed**
- A separate Student-Hub set (HUB_SCHOOL_CLUB, HUB_STUDY, HUB_CLASS, HUB_SOCIAL, HUB_MAJOR, HUB_DORM) lives in `0d7f0d96f75da042.js`; not shown in this modal. **observed**

## Step 2 — Tell Us More About Your Server (intent)

Module 632656, `bc4f97a0d13a0d1d.js`. **observed**
- Title: «Tell Us More About Your Server»
- Body: «In order to help you with your setup, is your new server for just a few friends or a larger community?»
- Two option rows: «For me and my friends» (`FRIENDS`) and «For a club or community» (`COMMUNITY`). Their order is randomized 50/50 at module load (`.5>Math.random()`).
- Footer text: «Not sure? You can <$link>skip this question</$link> for now.» (skip → intent `null`).
- Footer button: «Back».
- Intent only sets `isCommunity`, used for analytics (`is_community_intent`); it does not change the request body. **observed**

## Step 3 — Customize Your Server

Module 424912, `eac274ef20ad4dc6.js`. **observed**
- Title: «Customize Your Server»
- Body: «Give your new server a personality with a name and an icon. You can always change it later.»
- Icon uploader (module 356343, `b7df1e9224e03d2d.js`): 80×80 dashed-circle SVG with a blurple `+` badge, overlay label «Upload», file input `aria-label` «Upload a Server Icon», preview `alt` «Your icon».
- Icon minimum size: not found in this flow (no size copy rendered). «We recommend an image of at least 512x512.» exists but belongs to guild profile settings (`916d0bef82cfb632.js`). **observed**
- Name field: label «Server Name», required, `maxLength: 100`, default `getGuildNameSuggestion()` = «{username}'s server» using the user's display name (empty string if none). **observed** (`web.355a76e660bee7a5.js` module 695184)
- Staff-only toggle (Discord staff only): «Staff Only» / «Only Discord employees would be able to join this server.» **observed**
- Legal line: «By creating a server, you agree to Discord's <$b><$link>Community Guidelines</$link></$b>.» linking `${MARKETING_ENDPOINT}/guidelines`. **observed**
- Footer: «Back» (secondary) and «Create» (primary; «Next» in the hub variant); disabled while name is empty; shows loading while submitting. **observed**
- Errors: field error from `getFirstFieldErrorMessage("name")`; otherwise non-field error message below the form. **observed**

## Join a Server step

Module 130565 (`V`), `6fff93e818e496c4.js`. **observed**
- Title «Join a Server»; subtitle «Enter an invite below to join an existing server».
- Input label «Invite link» (required), placeholder `https://discord.gg/hTKzmak` (built from `GLOBAL_ENV.INVITE_HOST`, which is `discord.gg` in `app.html`).
- Default layout: caption «Invites should look like» with three clickable examples that fill the field: `hTKzmak`, `https://discord.gg/hTKzmak`, `https://discord.gg/wumpus-friends`; then a card «Don't have an invite?» / «Check out Discoverable communities in Server Discovery.» (navigates to Discovery). Footer: «Join Server» (primary) + «Back».
- Join-first layout: helper text «e.g. {inviteCode}, discord.gg/{inviteCode}, discord.gg/{vanityCode}» (hTKzmak / wumpus-friends), buttons «Browse Discovery» and «Join Server», footer «Want to start your own? <createServerHook>Create a server</createServerHook>».
- Parsing: trims input, takes the segment after the last `/` as the code, resolves it, then accepts. **observed**
- Validation errors:
  - empty → «Please enter a valid invite link or invite code.»
  - resolved invite without a channel → «The invite is invalid or has expired.»
  - API error with a known code → mapped text (e.g. TOO_MANY_USER_GUILDS → «You must leave a server first to join.», GUILD_AT_CAPACITY → «This server is currently full. Please try again later.», INVITES_DISABLED → «Invites are currently paused for this server. Please try again later. <$link>Learn More</$link>»); otherwise «Unable to accept invite». **observed** (`3aaa987fad4d03aa.js` module 4274)

## Requests

- Create (all template rows, including Create My Own): `POST /guilds` **observed** (`eac274ef20ad4dc6.js` module 943457)
  ```js
  body:{name:e,icon:t,channels:s.channels,system_channel_id:s.system_channel_id,roles:s.roles,
        guild_template_code:s.code,staff_only:!!d||void 0}
  ```
  `icon` is the data URL from the uploader (or `null`); `roles` is `undefined` except SCHOOL_CLUB. **observed**. Create My Own sends `channels: []`, `system_channel_id: null`; the server then creates its default channels. **inferred**
- Template links (`discord.new/<code>`, `/template/<code>` page): `POST /guilds/templates/{code}` with `{name, icon}`; the name default comes from the template's `defaultName` formatted with `{username}`; same 100-char name field and guidelines copy. **observed** (`220ec6518cfdb663.js`)
- Template resolve: `GET /guilds/templates/{code}`. **observed** (`web.355a76e660bee7a5.js` module 292572)

## Server limit copy

- Join error title: «You are at the [quantity] server limit.» with 200 when the user can use the increased cap (Nitro) or is staff, else 100; description «You must leave a server first to join.» **observed** (module 4274)
- Accept-invite modal button when capped: «You are at the 100 server limit». **observed** (`f58b1301353d44b9.js`)
- Upsell strings: «We love the enthusiasm, but you've hit the 100 server limit.», «Join up to 200 servers, use your server emoji everywhere, and more with Discord Nitro!», «You're in {guildCount} servers and might hit the 100 server limit soon.», «You can join up to 100 servers, and up to 200 with Nitro!» **observed** (messages). Which surface shows the create-time limit: not found; a create over the cap surfaces the API error message in the Customize step. **inferred**
