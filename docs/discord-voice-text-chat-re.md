# Discord: text chat in voice channels (RE notes)

Source: public web client bundle fetched 2026-11-22 from `https://discord.com/app` (initial chunks + all lazy chunks from the webpack chunk map). Copy resolved from the en-US intl chunk `c30b23bab1166ada.js` (chunk 301911). Static reading only; nothing was executed.

Labels: **[obs]** read in the bundle. **[inf]** inferred from code shape, not seen run. `Not verified:` no evidence found.

## 1. Channels: voice is a text channel

- **[obs]** Channel-type sets include GUILD_VOICE (2) and GUILD_STAGE_VOICE (13) in `TEXTUAL` and `GUILD_TEXTUAL` (`web.355a76e660bee7a5.js`):
  ```
  TEXTUAL:new Set([0,1,2,3,5,10,11,12,13,17,18,19,21]) ... GUILD_TEXTUAL:new Set([0,2,5,10,11,12,13,19,21]),GUILD_VOCAL:new Set([2,11,12,13])
  ```
- **[inf]** The client has no voice-specific message route. It uses the same `/channels/{id}/messages` routes and the same chat component as text channels, so sending is `POST /channels/{voice_id}/messages` and messages arrive as ordinary `MESSAGE_CREATE` with `channel_id` set to the voice channel. The toast hook below subscribes to plain `MESSAGE_CREATE` filtered by `channelId`, which supports this.
- **[obs]** The chat panel is the normal channel chat with `chatInputType === SIDEBAR`. Its textarea aria-label is `Text chat for voice channel {channelName}` (or `Text chat for stage channel {channelName}`) (`ceb73656db6e6af4.js`).

## 2. Permissions

- **[obs]** Channel settings for GUILD_VOICE add a section called `Voice Channel Chat Permissions`, with the description `Automated moderation makes monitoring text chat in voice channels easier. {$link}` (shown when AutoMod applies) (`f1ab8a7ffc23a8ee.js`). That section lists the usual text permissions (SEND_MESSAGES, READ_MESSAGE_HISTORY, ...).
- **[obs]** In the sidebar row, `locked = !can(CONNECT, channel)`, and `renderOpenChatButton` returns nothing when `locked` (`6b05ca0762af596a.js`). Without CONNECT there is no Open Chat button in the row.
- **[inf]** Reading and sending use VIEW_CHANNEL / READ_MESSAGE_HISTORY / SEND_MESSAGES like any text channel, through the shared chat component.
- Not verified: whether the server rejects message reads or sends in a voice channel from a user without CONNECT. The client only hides the entry point.

## 3. Entry points

### Sidebar voice row: "Open Chat"
- **[obs]** Copy: tooltip and aria-label `Open Chat`, with a `ChatIcon` size `xs`. Click handler:
  ```
  onClick:()=>{iw.A.updateChatOpen(e.id,!0),(0,i6.iN)(e.id, ...)}   // 6b05ca0762af596a.js
  ```
  It sets chat-open for that channel and then selects or navigates to the channel (the call view).
- **[obs]** The button is one of the row's trailing children, `[OpenChat, Invite, Edit, ChannelInfo]`. `forceShowButtons = connected && channelInfo == null`. When that is true the button gets the class `Tf`.
- **[inf]** Buttons show on row hover. They stay visible while you are connected and the row has no info pill (user-limit counter or mention pill). Not verified: the CSS, which was not downloaded.
- **[obs]** Clicking the row of a channel that has mentions opens chat automatically: `o=s>0; o&&iw.A.updateChatOpen(n.id,!0)` inside `handleVoiceConnect`, then it routes directly to the channel.
- **[obs]** Voice threads (vocal threads) show two buttons: `Join Voice` and `Open Chat`.

### Unread and mention badges in the sidebar row
- **[obs]** Props on the channel item are `unread: connected ? unread : undefined` and `mentionCount: mentionCount`. The unread dot shows only while you are connected to that channel. Mentions always show.
- **[obs]** Mention pill `n6`: `count` with color `BACKGROUND_FEEDBACK_NOTIFICATION`, or `BACKGROUND_MOD_STRONG` when `isMentionLowImportance`. It renders as channel info next to the user-limit counter.

### Call view header: chat toggle
- **[obs]** Toolbar button label `Show Chat` / `Hide Chat`. In a popout where chat is disabled the label is `Chat in the popout is coming soon.` (`fc9b5c3443825c91.js`). The aria-label is built as `[label, "{mentionCount}", "unread"].join(", ")`.
- **[obs]** Badge on the chat icon (`108460`): the count is mentions if there are any, otherwise unread. It renders `99+` above 99. It is drawn only when `canBadge`, meaning the user is connected to this voice channel. A typing indicator (three dots) is masked into the icon.
- **[obs]** Button highlight (`isShowing`) is on while mentions > 0, while someone is typing, or for **10 s** after the unread count changes (`R = 10*SECOND`).
- **[obs]** Keybind action `TOGGLE_VOICE_CHANNEL_CHAT`, copy `Toggle Voice Channel Chat` / `Toggle the text chat panel for the voice channel you are connected to.` It works only when connected to a guild voice channel. The RPC error otherwise is "Not connected to a guild voice channel".
- **[obs]** Escape closes the chat panel when the message list is scrolled to the bottom (`getChatOpen(a)?updateChatOpen(a,!1)`).

## 4. Chat panel state machine

State is a `Set` of open channel ids in `ChannelRTCStore` (`G`), written by the `CHANNEL_RTC_UPDATE_CHAT_OPEN {channelId, chatOpen}` action.

```
closed --OpenChat / toggle / keybind / row-click-with-mentions--> open
open   --toggle / Esc at bottom--> closed
open   --VOICE_CHANNEL_SELECT away (leave)--> closed   (when experiment closeChatOnLeave=true, the default)
open   --Stage audience sidebar opened--> closed
```
- **[obs]** `updateChatOpen(id,true)` then focuses the channel textarea on the next tick. `false` focuses the chat button instead.
- **[obs]** Default call layout for a guild vocal channel is `NO_CHAT` (`getLayout` returns `DUB.NO_CHAT`).
- **[obs]** Panel (`CallChatSidebar`, `1c44b4290d7ddc85.js`): resizable from its left edge, `minWidth` 450, and `maxWidth = window width - 550` (`maxSidebarWidth:o-550`). Width is persisted as `callChatSidebarWidth` in unsynced settings. Picture-in-picture `maxX` is reduced by the sidebar width while it is open.

## 5. Floating message previews ("toasts") in the call view

Hook and component in `c309c93572f8b54d.js` (module 20465):
- **[obs]** Source: dispatcher `MESSAGE_CREATE` where `channelId === call channel`, excluding `optimistic` (your own pending sends) and the stage system types (STAGE_START/END/TOPIC/SPEAKER/RAISE_HAND).
- **[obs]** At most **3** visible. On overflow the oldest are dropped: `r.length>3 && slice(len-3)`.
- **[obs]** Linger is **10 s** (`x=10*SECOND`). There is one shared timer, reset on every new message, and when it fires it clears all toasts. Toasts do not each have their own timeout.
- **[obs]** Hovering the stack freezes it (`isFrozen`): the list is snapshotted while hovered. Exit animation: opacity to 0 and translateY up by the toast height plus 8 px. Toasts stack with an 8 px gap. Theme is forced DARK.
- **[obs]** Each toast shows rendered message content (or sticker at 128 px), the author avatar at 32 px, and a mention highlight class. Clicking anything except a link calls `updateChatOpen(channel,true)` and `jumpToMessage({flash:true})`.
- **[obs]** Rendered only when `toastsEnabled && !chatOpen && !poppedOut` (`c7e5dc72699a60bc.js`). Also rendered over a running Activity frame (`64c5528b106130f6.js`).
- **[obs]** Setting: checkbox menu item `Show New Message Previews` (id `show-call-chat-toasts`) in the call's context menu. Stored per channel in `CallChatToastsStore` (persisted key `CallChatToasts`), `toastsEnabledForChannel[channelId] ?? true`. It is a local setting only, not synced.

## 6. Other copy
`NEW! Text chat in voice!`, `Voice Chat Text Channel`, `Show text chat notifications`, `Toggle channel member list or voice text chat`.

## Unverified
- Not verified: server-side permission rules for messages in voice channels (CONNECT requirement).
- Not verified: CSS hover rules for the row buttons.
- Not verified: the exact gateway payload. It was assumed identical to text channels, and no voice-specific field was seen in the client.
