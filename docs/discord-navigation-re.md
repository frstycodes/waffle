# Discord navigation: history, pinned DMs, pinned channels, Favorites, opt-in (code-derived)

Source: the public web client (`https://discord.com/app`, build `web.355a76e660bee7a5.js`) plus all 6,598 lazy chunks from its webpack chunk map, read as text on 2026-11-22. Nothing was executed. English labels come from the `en-US` message chunk `301911`. Module ids are written `#NNNNNN`. **observed** means the fact was read in code. **inferred** means it was reasoned from code but not traced end to end. The REA MCP server only offered `open_binary` in this session, and the JS analysis tool was not available, so the bundles were searched with grep and a brace-matching module extractor. The desktop Electron shell (`app.asar`) was not readable, so anything the shell sends is marked as such.

Related spec: `docs/discord-menus-channel-server.md` (menu groups, Favorites-mode menus, the `Show All Channels` server-menu item).

---

## 1. Back / Forward history

There are two separate mechanisms. Which one runs depends on whether channel tabs are on.

**A. Default: browser history.** **observed** `#976860` builds a `history` instance with `createBrowserHistory`. Back and Forward call `history.goBack()` / `goForward()`. The buttons read `window.navigation.canGoBack/canGoForward` (web `rG`, refreshed on `ROUTE_CHANGED`).
- **What pushes an entry: observed** Every `transitionTo(path)` (`#976860.pX`) calls `history.push`. If the target equals the current pathname+search+hash, it calls `replace` instead, which is the only de-dup rule:
  ```js
  n.pathname===l.pathname&&n.search===p(l.search,"?")&&n.hash===p(l.hash,"#")?i.replace(l):null==t?i.push(e):i.push(l)
  ```
  Channel switch, server switch, DM open and Friends/home are all routes, so each one pushes: `/channels/{guild|@me}/{channel}[/{message}]`, with Friends at `/channels/@me` (`#652215` route table, **observed**). Threads opened full-view have their own route, `CHANNEL_THREAD_VIEW` (**observed** route exists; that the sidebar thread view does not push is **inferred**). A `/settings/{section}` route exists (**observed**), but whether opening User Settings pushes it was not traced (**unknown**). Inbox was not traced (**unknown**; believed to be a popout, which would not push).
- **Server switch: inferred** The Favorites rail icon is a router link `to:{pathname: CHANNEL("373", lastChannel)}` (`eZ` in web), and server icons work the same way, so they push.
- **Cap: inferred** None in app code. The browser's own session-history limit applies.
- **Block rules: observed** `history.block` refuses a POP while the top modal does not allow navigation. Every Back/Forward first closes any open context menu (`#712687.close()`).
- **Scroll / anchor: observed** History entries store only the URL. Scroll position is kept per channel, not per entry, in `DimensionStore` `#72314` (`{channelId, scrollTop, scrollHeight, offsetHeight}`, in memory). Channel-list scroll is kept per guild (`UPDATE_CHANNEL_LIST_DIMENSIONS`). So going Back to a channel restores that channel's last scroll. **inferred**: it shows the channel as it was last viewed, not a snapshot of the entry. An entry whose URL has a `messageId` jumps to that message.

**B. Channel tabs: per-tab stacks.** **observed** in `ChannelTabsStore` `#442325`. Active when the user opted in, the experiment `2026-07-desktop-channel-tabs` (`#858760`) is enabled, and the client is native desktop (`#574381.xl`). When the active tab's channel is not the selected channel, Back/Forward fall through to mode A (`#114129` `T()` returns `"passthrough"`).
- Entry = `{kind:"channel", channelId, guildId}` or `{kind:"route", routePath, routeLabel}`. Only Friends, Shop, Quests, Nitro and Conjure become route entries (`#20517.rv`). Entries carry no scroll data.
- A push happens on `CHANNEL_SELECT` and `CHANNEL_TABS_NAVIGATE_ROUTE`. De-dup: if the new location equals the current entry (`#20517.Pp`: same `channelId`, or same `routePath`), nothing is pushed. Pushing truncates the forward entries:
  ```js
  let n=[...t.entries.slice(0,t.index+1),e].slice(-50)
  ```
- **Max stack: 50 per tab** (also enforced on load). A new tab inherits the source tab's last 10 entries plus the new one (`slice(-11)`). Max tabs: 9, or 25 for staff. Recently closed tabs: 10.
- **Pinned tab:** navigating inside a pinned tab opens a new tab instead (when at the tab limit, it unpins the tab and navigates in place).
- **Target gone: observed** If the entry's channel is no longer cached and it has no `guildId`, the result is `"noop"` (nothing happens). If it has a `guildId`, the client navigates to `/channels/{guild}/{channel}` anyway and the route's fallback decides (**inferred**: the guild default channel). On `CHANNEL_DELETE`/`THREAD_DELETE`, tabs (other than the active one) whose channel was deleted are closed.

**Shortcuts: observed** `#980372`:
| Action | macOS (native `darwin` or a Mac UA) | Windows / Linux / web |
|---|---|---|
| Back (`GY`) | `mod+[` (Cmd+[) | `alt+left` |
| Forward (`M$`) | `mod+]` | `alt+right` |
| Next / previous channel in the server (or DM, with home routes first) | `alt+down` / `alt+up` | same |
| Toggle between the last server and DMs | `mod+alt+right` | same |

Back/Forward are also user-bindable global keybinds (`NAVIGATE_BACK`/`NAVIGATE_FORWARD`, which fire on keyup).

**Mouse buttons 4/5: observed** (consumer only) On desktop, the shell emits native `NAVIGATE_BACK`/`NAVIGATE_FORWARD` events and the web code runs Back/Forward for them. It skips this when the user has bound mouse button Back (3) or Forward (4) as a keybind (`kw()`; button enum `Left=0…Back=3,Forward=4`). **inferred**: on the web, the browser handles mouse buttons natively through popstate.

The buttons' tooltips are `Go back` / `Go forward`. Analytics event `APP_BACK_FORWARD_NAVIGATED{nav_direction, nav_trigger: nav_button|keybind|global_keybind|native}`.

## 2. Pinned DMs

- **Storage: observed** The same flag as channel pins: user-guild-settings `channel_overrides[dmId].flags |= FAVORITED (2048)`, under the null/`@me` guild. `#816662.xl`:
  ```js
  xl:function(e,t){let n=A.Ay.getChannelIdFlags(null,e);l.A.updateChannelOverrideSettings({guildId:null,channelId:e,settings:{flags:(0,r.lA)(n,p.vv.FAVORITED,t)},…
  ```
  Wire: `PATCH /users/@me/guilds/@me/settings {channel_overrides:{<dmId>:{flags}}}` (`#931260`, the null guild maps to `@me`). It is not in settings-proto. Read back through `isMessagesFavorite(id)` (`#543465`).
- **Limit: observed (bounded)** None in the toggle, the store, or the menu item.
- **Menu: observed** `#439191` (chunk 17244): id `pin-dm`, label `Pin` / `Unpin`, PinIcon (leading). Shown for any `isPrivate()` channel, both DM and group DM. Menu placement is in the prior doc §2.8/2.9.
- **Ordering: observed** `PrivateChannelSortStore` `#645959` puts pinned DMs first, then the rest. Both groups are sorted by recency (`-timestamp(lastMessageId)`; message-request timestamp when later). There is no manual order and no pin-time order. Message requests and spam are excluded from both.
  ```js
  new d.J(function(e){let{isRequest:t,isFavorite:n}=e;return t?[]:[n?"FAVORITE":"DEFAULT"]},function(e){let{lastMessageId:t}=e;return-m.default.extractTimestamp(t)})
  ```
- **Rendering: observed** The DM list (web `Z`) is a single section over `getPrivateChannelIds()`, the flat list with pins first. There is **no "Pinned" header or divider** and nothing to collapse. A pinned row shows a small PinIcon (`size:"xs"`) in its trailing slot. The icon is replaced by the Deny icon when the user is blocked and by EyeSlash when ignored. The row's a11y description adds `Pinned` (`cCPjSK`).
- **Unread/mentions: inferred** No special-casing. Pinned DMs badge, mute and ack like any DM. Alt+Up/Down channel cycling walks the same pins-first order (`#980372`).
- **Unpin: observed** Clears the bit, and the DM falls back into the recency-sorted default group. Closing a DM removes it from the list (`CHANNEL_DELETE`). The override flag itself is left alone (**inferred**).

## 3. Pinned channels in a server

- **Storage:** `FAVORITED` 2048 in the guild's `channel_overrides` (prior doc). Toggle `#816662.jA`. If the channel is not opted in, turning the pin on also sets `OPT_IN_ENABLED` (4096) (**observed**). Wire: `PATCH /users/@me/guilds/settings {guilds:{<gid>:{channel_overrides:{…}}}}` (bulk, debounced) (**observed** `#931260`).
- **Where: observed** `GuildChannelList` `#791513`, section 2 (`favoritesSectionNumber=2`). It comes after the notice row and action rows, before section 3 (recents) and section 4 (uncategorized), and categories start at 5. Header `Pinned Channels` (`mlPMCy`), rendered as a plain section header in chunk 441477. A pinned channel is **removed from its category** (its renderLevel becomes 1 in the category through `eT()`).
- **Ordering: observed** By the channel's server `position`, with voice channels after text (`position+1e4`). No manual order.
- **Visibility: observed** Rows in this section render whenever the user can VIEW_CHANNEL (`class ef` → renderLevel 4), whatever the mute state, the category collapse, hide-muted, or opt-in.
- **Suggested: observed** `FavoritesSuggestionStore` adds one suggested channel to the same section after more than 50 selections of an un-pinned, opted-in channel (dismissable).
- **Drag: observed** A pinned channel cannot be dragged. Trying opens the modal `Trying to reorder a pinned channel?`: "Unpin this channel before reordering. Pinning is only for you, but channel order applies to everyone." with confirm `Unpin Channel` (`#933752`, chunk 280545). Nothing can be dropped onto a pinned channel either.
- **Opt-out interplay: observed** Removing a channel from the channel list (`#816662.AC/zk`, opt-in off) also clears `FAVORITED`.
- **Limit / gate: observed (bounded)** No count limit and no experiment gate on `#704543` (`pin-channel`) or `jA`. The Favorites pseudo-server and full-server preview are excluded.

## 4. Favorites server

- **Rail: observed** The rail order is Home → [Conjure] → **Favorites** → unread-DM bubbles → (component `q`, **inferred** separator) → lurked/guest servers → servers (`rn` in web). Favorites is a `StarIcon` 20px, aria `"{guildName}, {mentions}"` with name `Favorites`. It is rendered only when `favoriteGuildEnabled` (`#749884`). Its badge sums mentions across favorite channels and their joined threads. Its unread pill is suppressed when Favorites is muted. Context-menu navId `favorite-server-context`.
- **Pseudo-guild: observed** `#349828`: id `"373"`, name `Favorites`. `#5180.ai(id)` tests for it.
- **Storage: observed** settings-proto **PreloadedUserSettings field 15 `favorites`** (`Favorites`), saved with `PATCH /users/@me/settings-proto/1 {settings:<base64>, required_data_version}`.
  | `Favorites` field | no | type |
  |---|---|---|
  | `favorite_channels` | 1 | `map<fixed64, FavoriteChannel>` |
  | `muted` | 2 | bool |
  | `guild_visible` | 3 | BoolValue (absent = visible iff any favorites) |
  | `auto_add_joined_threads` | 4 | bool |
  | `prune_inactive_threads` | 5 | BoolValue (default true) |
  | `prune_inactive_threads_days` | 6 | uint32 |
  | `prune_all_favorited_threads` | 7 | bool |
  | `auto_add_joined_threads_per_guild` | 8 | bool |

  | `FavoriteChannel` field | no | type |
  |---|---|---|
  | `nickname` | 1 | string ("" = none; the category name for categories) |
  | `type` | 2 | enum `UNSET=0, REFERENCE_ORIGINAL=1, CATEGORY=2` |
  | `position` | 3 | uint32 (new = max+1) |
  | `parent_id` | 4 | fixed64 (`0` = root; must point at a CATEGORY entry, otherwise reset to 0) |
  | `channel_type` | 5 | wrapped int (cached channel type) |
  | `collapsed` | 6 | bool (categories) |
- **Limits: observed** `#93055`: Nitro (TIER_2) users get 200 channels, everyone else 3, with an upsell. There is also a hard cap of 200 entries including categories. Categories do not count against the per-tier limit. Over-limit alert: `Oh no!` / `You cannot have more than {count} …`.
- **Add/remove: observed** `#794412`. Adding a channel sets `guild_visible=true` if Favorites was hidden. Toast `Added to Favorites!`. Removing a category re-parents its children to root. On save, entries are pruned when the channel is gone (unless it is a thread type) or no longer viewable. Menu labels and ids: `Add to Favorites` / `Remove from Favorites` (`favorite-channel`), `Change Nickname` / `Rename Category` (`set-channel-nickname`), `Go to Server` (`go-to-original-guild`), header menu `Thread Settings` / `Hide Favorites` / `Reset Favorites`, "+" menu `Add to Favorites` / `Create Category`. All of these are in the prior doc.
- **Channel list in Favorites: observed** `#796637`. Uncategorized first, then categories by `position`. Inside each group, `position` order with voice +1e4. Category names are unique case-insensitively (an add by name reuses the existing category). Rows follow the normal collapse/mute hiding rules, except that selected, mentioned or thread-active channels always show. The empty state reads `Favorites will show up here!`. The setting toggle in User Settings → Chat is labelled (beta) and calls `#794412.kG`.

## 5. Show All Channels / channel opt-in

- **Flags: observed** `#355097`:
  - guild `flags`: `OPT_IN_CHANNELS_ON 16384`, `OPT_IN_CHANNELS_OFF 8192`
  - channel override `flags`: `OPT_IN_ENABLED 4096`

  `isOptInEnabled(g)` is true when `OPT_IN_CHANNELS_ON` is set. **"Show All Channels" checked = opt-in OFF.**
- **Menu: observed** `#36942` (chunk 317699): id `opt-in`, check, label `Show All Channels`, EyeIcon in the header-dropdown variant. Shown only for COMMUNITY servers or staff users. Action `#234053.Lu`:
  ```js
  Lu:function(e){if(c(e))return void o(e);{let t=(0,h.WW)(e);(0,d.e4)(e,!t)}}
  ```
  First use (opt-in off, onboarding not completed, nothing opted in yet) calls `onboardExistingMember` with every non-thread text and voice channel. Otherwise it toggles `OPT_IN_CHANNELS_ON` through `PATCH /users/@me/guilds/{gid}/settings {flags}`.
- **What it hides: observed** `#791513` `eA.computeState`. With opt-in on, a channel shows only if it or its parent category is opted in, or it is selected, has a mention, or has active threads. Resource channels can be hidden too. The `voiceChannelsCategory` section collects un-opted voice channels that have people in them. Pinned channels always show.
- **Browse Channels: observed** A guild action row (`CHANNELS_AND_ROLES`, chunk 441477) for COMMUNITY non-hub servers, shown when onboarding has prompts or opt-in is on. Its label is `Channels & Roles` (route `/channels/{gid}/customize-community`) when onboarding has prompts, otherwise `Browse Channels` (route `CHANNEL_BROWSER`). It shows a "new" badge when more than 2 new channels exist.
- **Per-channel items:** `Add to Channel List` / `Remove from Channel List` (prior doc). Opting into a channel while opt-in is off also turns guild opt-in on (`#816662.zk`).

---

## Endpoints & wire

| Purpose | Method / path | Body | Status |
|---|---|---|---|
| Pin/unpin DM | `PATCH /users/@me/guilds/@me/settings` | `{channel_overrides:{<id>:{flags}}}` (bit 2048) | observed |
| Pin/unpin server channel, opt a channel in/out | `PATCH /users/@me/guilds/settings` | `{guilds:{<gid>:{channel_overrides:{<id>:{flags}}}}}` | observed |
| Show All Channels | same bulk, or `/users/@me/guilds/{gid}/settings` | `{flags}` (16384) | observed |
| First-time opt-in | `POST /guilds/{gid}/onboarding-responses` | `onboarding_responses…` | route observed; link from `onboardExistingMember` inferred |
| Favorites | `PATCH /users/@me/settings-proto/1` | `{settings: base64(PreloadedUserSettings), required_data_version}` (field 15) | observed |
| Load settings | `GET /users/@me/settings-proto/1` | | observed |
| Gateway | `USER_GUILD_SETTINGS_UPDATE`, `USER_SETTINGS_PROTO_UPDATE` | | inferred (store handlers observed) |

## Open questions

- Does opening User Settings push `/settings/{section}` into history? The route exists, but the modal path was not traced.
- What exactly does the router do on `/channels/{g}/{deletedChannel}` (fallback channel)?
- How does the Electron shell map mouse buttons 4/5 and the macOS swipe gesture to `NAVIGATE_BACK/FORWARD`? `app.asar` was unreadable.
- Does a pinned DM's override survive closing the DM? Probably yes, but this was not traced on the server side.
- Pinned-channel count limit: none in the client. A server-side cap is unknown.
