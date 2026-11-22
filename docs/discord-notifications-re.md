# Discord notifications: reverse-engineered reference

Source: public web client, fetched 2026-11-22 with no auth (`curl https://discord.com/app`). `BUILD_NUMBER` 678976, buildId `e85e837cc69d2996c2dbf21b620a85d06ac15dab`.
The local desktop cache (`~/Library/Application Support/discord/`) could not be used: macOS privacy protection denied the read, even outside the sandbox.

Files read (all from `/assets/`):
- `web.355a76e660bee7a5.js`: main bundle. Contains the endpoints, enums, ReadStateStore, keybinds and the recent-mentions actions.
- `0a2ba4661cec17b3.js`: guild Notification Settings modal (module 131527).
- `e787a25bbc61388a.js`: mute duration options and override filter (module 769591).
- `1a7a109f727a66ca.js`, `27a3efd351a1f2d2.js`, `87c6c8ea471f335f.js`, `bd33ee8764f79945.js`: channel header bell button, its popout, the mute submenu and the level items.
- `5f9459e4b1c56cb8.js`: markUnread (module 439147). `2420b0c09d1ff9a5.js`: Alt+click. `ceb73656db6e6af4.js`: message keyboard handler. `d5b297eb0233cd09.js`: "Mark Unread" message context-menu item.
- `e79003ed31999f0b.js`: Inbox popout (Unreads and Mentions tabs).
- en-US i18n chunks (`c30b23bab1166ada.js` and others): message keys resolved to English text. Labels below are quoted after that resolution.

How the evidence is tagged:
- **observed**: read directly in minified JS. Short identifiers like `X.orn` are the minifier's names.
- **inferred**: reasoned from that code, not seen working.

## A. Server "Notification Settings" modal

### Structure and labels
- **observed**: Modal title "Notification Settings". The one footer action is "Done", which only closes the modal (`onClick:i`, which is `onClose`). There is no save step. Every control writes as soon as it changes.
  `(0,n.jsxs)(g.a,{title:"Notification Settings",actions:[{text:"Done",onClick:i}],onClose:i,...`
- **observed**: Sections, top to bottom: mute block (`eo`), level radios (`ed`), Community Activity Alerts (`eh`), suppress and other toggles (`ec`), override picker (`eu`), override table (`eg`).
  `children:[(0,n.jsx)(eo,{...}),(0,n.jsx)(ed,{...}),(0,n.jsx)(eh,{guildId:t}),(0,n.jsx)(ec,{...}),(0,n.jsx)(eu,{...}),(0,n.jsx)(eg,{...})]`
- **observed**: Mute block. A toggle labelled "Mute {name}" (bold server name) with the description "Muting a server prevents unread indicators and notifications from appearing unless you are mentioned." While the server is muted, a single-select "Mute duration" appears below it, defaulting to `selected_time_window ?? ALWAYS`.
  `label:"Mute {$b}",{name:l.name}),description:"Muting a server prevents unread indicators…",checked:i,onChange:e=>s("muted",e,…)`
- **observed**: Mute durations (`Z.b8()`). The values are in seconds, from enum `ee.Xx`:
  | id | label | value |
  |---|---|---|
  | `15-minutes` | For 15 Minutes | 900 |
  | `1-hour` | For 1 Hour | 3600 |
  | `3-hours` | For 3 Hours | 10800 |
  | `8-hours` | For 8 Hours | 28800 |
  | `24-hours` | For 24 Hours | 86400 |
  | `forever` | Until I turn it back on | -1 |

  `i[i.MINUTES_15=900]…i[i.HOURS_24=86400]="HOURS_24",i[i.ALWAYS=-1]="ALWAYS"`
- **observed**: Level radio group labelled "Server Notification Settings". It is disabled while the server is muted. Options:
  - "All Messages". When the member count is 2500 or more it adds the note "Due to this server's size, you won't get mobile push notifications for non-@mention messages."
  - "@mentions + Unreads", only when the `useNewNotifications` experiment is on.
  - "Only **@mentions**"
  - "Nothing"

  `options:[{name:"All Messages",value:X.orn.ALL_MESSAGES,desc:null!=s&&s>=2500?…:null},...r?[{name:"@mentions + Unreads",value:es}]:[],{name:"Only {$b}"…`
- **observed**: "Community Activity Alerts" ("Receive notifications for DM or join activity that exceeds usual numbers for your server.") is shown only when `(0,M._g)(guildId)` is true. It has two toggles, "In-app alerts" and "Push notifications". These are written to user settings proto (`disableRaidAlertNag` and `disableRaidAlertPush`), not to guild settings.
- **observed**: Toggle block, in this order:
  1. "Suppress **@everyone** and **@here**" → `suppress_everyone`
  2. "Suppress All Role @mentions" → `suppress_roles`
  3. "Suppress Highlights". Description: "Highlights provide occasional updates when your friends are chatting in busy servers, and more." Followed by a "Learn more about Highlights" link. Shown checked and disabled while the server is muted.
  4. "Mute New Events" → `mute_scheduled_events`
  5. "Mobile Push Notifications" → `mobile_push`. Disabled while muted and shown unchecked (`checked:!o&&a`).

  None of these toggles has a sublabel except Highlights. There is no "notify on all messages for new channels" toggle.
  `l("suppress_everyone",e,…)` … `l("suppress_roles",e,…)` … `l("mute_scheduled_events",e,…)` … `l("mobile_push",e,…)`
- **observed**: Override picker. A select labelled "Select a channel or category…" with the description "Add a channel to override its default notification settings"; its placeholder is the same "Select a channel or category…". It lists channels (`tr(type)`) and non-empty categories. Choosing one adds a row locally and makes no request. The row is autofocused, scrolled into view and highlighted for 1 s.
- **observed**: Overrides table, `aria-label` "Notification Overrides". Column headers:
  - "Channel or Category"
  - "All"
  - "Mentions + Unreads" (new-notifications experiment only)
  - "Mentions"
  - "Nothing" (hidden when the experiment is on)
  - "Mute", with tooltip "Muting a server prevents unread indicators…"

  With no rows, a placeholder row reads "Add a channel to override its default notification settings".
- **observed**: Each row shows the channel icon and name. Its subtitle is "in {categoryName}", "No Category", or the child count for a category. The level radios are checkbox-styled. Each is `checked:!guildMuted && selected` and disabled while the guild is muted. When the row's level is NULL, the guild's level shows as selected. The row also has a mute checkbox and a remove button whose `aria-label` is "Remove notification override for {name}". Remove writes `{message_notifications: 3 (NULL), muted: false}`.
  `this.setMessageNotification(X.orn.NULL,!1,V.fd.OverrideDeleted,…)`
- **observed**: Existing rows come from `channel_overrides` entries that are not default: the level is not NULL, the override is muted, or (unless ignored) an unread flag is set.
- **inferred**: The modal is guild-only and DMs cannot reach it. `NOTIFICATION_SETTINGS_MODAL_OPEN` carries a `guildId`, and the mute block returns null without a guild.

### REST and gateway
- **observed**: Every modal write goes through `updateGuildNotificationSettings(guildId, partial)` → `saveUserGuildSettings`. The client updates its own state at once (`USER_GUILD_SETTINGS_GUILD_UPDATE`) and sends the request immediately. There is no debounce; the 15 s timer only batches category collapse.
- **observed**: The request for a guild is the bulk endpoint, **`PATCH /users/@me/guilds/settings`** with body `{guilds: {[guild_id]: partial}}`. The single endpoint **`PATCH /users/@me/guilds/{id}/settings`** is used only when `guildId` is null or ME, which is the DM pseudo-guild.
  `null==e||e===o.ME?await i.Bo.patch({url:o.Rsh.USER_GUILD_SETTINGS(o.ME),body:t}):await f({[e]:t})`
  `i.Bo.patch({url:o.Rsh.USER_GUILD_SETTINGS_BULK,body:{guilds:e}})` with `USER_GUILD_SETTINGS_BULK:"/users/@me/guilds/settings"`
- **observed**: Body fields sent by the modal and menus:
  - `muted: bool`
  - `mute_config: {selected_time_window: <seconds or -1>, end_time: ISO string | null}`. `end_time` is null when the value is not above 0.
  - `message_notifications`, plus `flags` when the new-notifications experiment is on
  - `suppress_everyone`, `suppress_roles`, `notify_highlights`, `mute_scheduled_events`, `mobile_push`
  - `channel_overrides: {[channel_id]: {message_notifications, muted, mute_config, flags, collapsed}}`

  `{muted:!0,mute_config:{selected_time_window:e,end_time:t}}`
- **observed**: The plain "Mute {name}" toggle sends `{muted: true|false}` with no `mute_config`. Choosing a duration sends `{muted: true, mute_config: {…}}`.
- **observed**: Gateway `USER_GUILD_SETTINGS_UPDATE` is dispatched internally as `USER_GUILD_SETTINGS_FULL_UPDATE` with `userGuildSettings: [payload]`, i.e. the full settings object for one guild.
  `eH(["USER_GUILD_SETTINGS_UPDATE"],e=>{eY({type:"USER_GUILD_SETTINGS_FULL_UPDATE",userGuildSettings:[e]})})`

### Enums
- **observed**: Message notifications (`orn`): `ALL_MESSAGES=0, ONLY_MENTIONS=1, NO_MESSAGES=2, NULL=3`. NULL means inherit from the category or server.
  `Q[Q.ALL_MESSAGES=0]…Q[Q.ONLY_MENTIONS=1]…Q[Q.NO_MESSAGES=2]…Q[Q.NULL=3]`
- **observed**: Highlights (`guM`): `NULL=0, DISABLED=1, ENABLED=2`. The Suppress toggle sends `DISABLED` (1) when checked and `ENABLED` (2) when unchecked.
  `rh=((tC={})[tC.NULL=0]="NULL",tC[tC.DISABLED=1]="DISABLED",tC[tC.ENABLED=2]="ENABLED"`
- **observed**: "@mentions + Unreads" is not a separate level. It is sent as `message_notifications: 1` plus a `flags` bit `UNREADS_ALL_MESSAGES`; "Only @mentions" sets `UNREADS_ONLY_MENTIONS` instead. The bit values were not extracted.

## B. Channel header bell popout
- **observed**: The bell is a header icon with tooltip and `aria-label` "Notification Settings". It shows `BellSlashIcon` when the channel is muted or its resolved level is not ALL_MESSAGES. **Shift+click toggles channel mute without opening the popout.**
  `e.shiftKey?c.A.updateChannelOverrideSettings({…settings:{muted:!j}…}):N(e=>!e)`
- **observed**: The popout is a menu with `navId:"channel-context"` and `aria-label` "Channel Actions". It has two groups, separated:
  1. **Mute.** For a text channel, "Mute Channel" with a submenu of the 6 durations from section A (same labels and values). Clicking the parent item mutes indefinitely (`{muted: true}`). Once muted, the item becomes "Unmute Channel" with the remaining time as subtext. Categories read "Mute/Unmute Category", group DMs "Mute/Unmute Conversation", and DMs "Mute {name}".
  2. **Level radios** (group `channel-notifications`), in this order:
     - "Use Category Default" if the channel has a parent, otherwise "Use Server Default" (NULL = 3). Its subtext is the inherited level's label.
     - "All Messages" (0)
     - "@mentions + Unreads" (1 plus the unreads flag, experiment only)
     - "Only **@mentions**" (1)
     - "Nothing" (2)

  Stage channels instead offer: default, "All Live Stages" (1), "Nothing" (2). Voice channels get no level items. Forum channels add a "New Posts Created" checkbox at the top.
  `{setting:u.orn.NULL,label:null!=t.parent_id?"Use Category Default":"Use Server Default"},{setting:u.orn.ALL_MESSAGES,label:"All Messages"},…`
- **observed**: Each item writes `updateChannelOverrideSettings` → `{channel_overrides: {[id]: {message_notifications}}}` (plus `flags` under the experiment), through the same bulk PATCH as A. A duration pick writes `{muted: true, mute_config: {selected_time_window, end_time}}`.

## C. Mark Unread
- **observed**: The request is **`POST /channels/{channel_id}/messages/{ack_id}/ack`** with body **`{manual: true, mention_count: N}`**. There is no `token`.
  `t.Bo.post({url:h.Rsh.MESSAGE_ACK(e,f),body:{manual:!0,mention_count:m}})`
- **observed**: `ack_id` is the newest loaded message older than the clicked one. If none is loaded, it is a snowflake one millisecond before the clicked message (`atPreviousMillisecond`).
  `p=d.toArray().filter(e=>0>compare(e.id,r)).sort(…).reverse()[0],f=null==p?atPreviousMillisecond(r):p.id`
- **observed**: `mention_count` counts loaded messages newer than `ack_id` for which `ey(msg, me)` is true. That is a message that mentions the user, with suppress-everyone and suppress-roles applied and blocked or ignored authors excluded, or any message in an unmuted DM. For threads, the client first unarchives an archived thread, then joins if not joined.
- **observed**: Normal reads (non-manual) post the same endpoint with `{token, last_viewed, flags}`. `flags` is 1 for a guild channel, 2 for a thread, and only sent when it changed.
- **observed**: **Alt+click** on a message calls markUnread when `altKey` is held, using `preventDefault`.
  `e.altKey&&(e.preventDefault(),(0,y.A)(i,n))`
- **observed**: **Alt+Enter** on a keyboard-focused message calls markUnread (handler `case"enter":s&&…`, where `s` = alt only). The shortcut sheet lists it as "Mark Unread" → `binds:["alt+enter"]`.
- **observed**: Message context-menu item `id:"mark-unread"`, label "Mark Unread", icon `ChatMarkUnreadIcon`. It is hidden for inactive threads that are locked or not accessible (`Id(channel)`).
- **observed**: The gateway `MESSAGE_ACK` payload is `{channel_id, message_id, manual, mention_count, version}`. When `manual` is set, the read state is rebuilt: unread count recomputed from `message_id` and `mentionCount = mention_count`. When it is not set, the client simply acks locally.
  `eY({type:"MESSAGE_ACK",channelId:e.channel_id,messageId:e.message_id,manual:e.manual,newMentionCount:e.mention_count,version:e.version})`
- **inferred**: The client does not update its read state optimistically after the POST. The unread marker appears when the gateway echoes `MESSAGE_ACK` with `manual: true`.

## D. Inbox
- **observed**: Toggle keybind `TOGGLE_INBOX: {binds:["mod+i"], comboKeysBindGlobal:true}`. Mark top inbox channel read: `binds:["mod+shift+e"]`, debounced 100 ms.
- **observed**: Mentions list request **`GET /users/@me/mentions`** with query `before, limit (25), guild_id, roles, everyone, feature`. `guild_id` is null unless the filter is "This server".
  `query:{before:t,limit:n,guild_id:s,roles:l,everyone:o,feature:d}`
- **observed**: Mentions filter menu "Filter" (`navId:"mentions-filter"`) has three checkboxes: "Include @everyone mentions", "Include @role mentions", and "Include all servers" (hidden in DMs).
- **observed**: Removing a mention uses a button with **`CheckmarkLargeIcon`**, not an ✕. Its tooltip and `aria-label` are **"Mark As Read"**. It calls **`DELETE /users/@me/mentions/{message_id}`** and dispatches `RECENT_MENTION_DELETE` optimistically. The channel's read state is not acked. Each mention also has "Jump to Message". The empty state reads "Whenever someone mentions you it will be saved here for 7 days."
  `onClick:()=>tt.A.deleteRecentMention(t.id)` → `i.Bo.del({url:a.Rsh.MENTIONS_MESSAGE_ID(e)})`
- **observed**: In the Unreads tab, the per-channel button "Mark As Read" (`CheckmarkLargeIcon`) dispatches `CHANNEL_ACK` with `messageId = newestUnreadMessageId`. That becomes the normal `POST /channels/{id}/messages/{newest_unread_id}/ack` `{token,last_viewed,flags}`. The action supports undo with mod+Z, which re-acks `oldestReadMessageId`. Under a feature flag (`tO.A()`), the guild-row variant is "Mark Server As Read" (`DoubleCheckmarkIcon`).
- **observed**: "Mark All as Read" (`DoubleCheckmarkIcon`) opens a confirm: header "Mark all as read?", body "This will mark all your unmuted channels as read. Are you sure you want to continue?", buttons "Mark As Read" and "Cancel". **Shift+click skips the confirm.** It sends `BULK_ACK` → **`POST /read-states/ack-bulk`** `{read_states:[{channel_id, message_id, read_state_type}]}`, in chunks of 100 with 1 s between chunks.
- **observed**: The Unreads tab hides:
  - channels where the guild, category or channel is muted
  - muted or unjoined threads
  - channels with no mentions whose unread setting is not ALL_MESSAGES
  - guild events when the guild is muted or `mute_scheduled_events` is set

  Section dividers read "Messages older than 10 days" and "Notifications disabled". The empty-state copy reads "Unread messages from all your unmuted channels will show up here."
  `if(nw.Ay.isGuildOrCategoryOrChannelMuted(e.guildId,e.channelId))return!0;…0===e.mentionCount&&nw.Ay.resolveUnreadSetting(t)!==nU.e.ALL_MESSAGES`
- **inferred**: "Mark Server As Read" calls the guild-ack helper (`nM.A([guildId])`), which most likely goes through the same bulk ack. This was not traced.

## Not found or not verified
- The numeric values of the `UNREADS_ALL_MESSAGES` and `UNREADS_ONLY_MENTIONS` flag bits (`ee.vv`, `ee.n3`).
- That `o.ME === "@me"` (inferred from the endpoint shape).
- Whether the desktop build differs from this web build. The desktop cache could not be read.
