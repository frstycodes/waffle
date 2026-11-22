# Discord: per-send options (@everyone guard, @silent, /tts) — RE notes

Source: public web client `https://discord.com/app`, build `678976`, fetched 2026-11-22 (no auth). Local Electron cache was not readable from the sandbox. Strings resolved from the en-US message chunk `c30b23bab1166ada.js`. **observed** = in bundle (file named); **inferred** = my reading.

## 1. @everyone / @here confirmation

Code: `web.355a76e660bee7a5.js` (message validation, module containing `/@(:?everyone|here)/`), popout UI `ceb73656db6e6af4.js`.

- **observed** It is a **popout** anchored to the composer ("Hold Up!"), not a modal. It's one of an ordered list of pre-send checks (`g`); the other is the token-leak warning.
- **observed** Detection: walks the parsed markdown AST of the content, skipping `inlineCode` and `codeBlock`, and matches `/@(:?everyone|here)/`. The first match (`@everyone` or `@here`) is the "role". **Role mentions (`<@&id>`) do not trigger it.**
  ```
  let f=new RegExp(/@(:?everyone|here)/);
  if("inlineCode"===t.type||"codeBlock"===t.type)return null;let e=t.content?.match(f);
  ```
- **observed** Trigger: guild channel only, `restrictMentions` true, **member count > 30** AND the user **has `MENTION_EVERYONE`** in the channel (without the permission the mention won't ping, so no warning).
  ```
  I=function(e,t){…return h(e,t)>30&&E.A.can(A.xBc.MENTION_EVERYONE,t)}
  check(e,t,n){if(!n||null==t.getGuildId())return!1;let i=p(e,t);if(null==i||!I(i,t))return!1;…
  ```
- **observed** Count `h()`: threads → `channel.memberCount`; otherwise sum of member-list group counts for the channel. `@everyone` counts every group, including offline. `@here` excludes the `OFFLINE` group.
- **observed** Shown count is truncated to its leading digit (`1234 → 1,000`, `57 → 50`), `toLocaleString()`, followed by "+".
  ```
  a=Math.pow(10,Math.floor(Math.log10(r))) … count:(Math.trunc(r/a)*a).toLocaleString()
  ```
- **observed** Copy:
  - Header: "Hold Up!" (`mY3Y38`)
  - Body (channel): "By using **{role}**, you are about to mention {count}+ people. Are you sure?" (`47E5Rz`)
  - Body (forum post): "…{count}+ people in this post. Are you sure?" (`sYW2cy`); thread: "…in this thread. Are you sure?" (`2YaiQ1`)
  - Footer (info icon): "Maybe try mentioning a role or individual users instead?" (`mVyrtu`)
  - Buttons: secondary "Edit Message" (`fsBWmS`) + hint "**ESC** to edit"; primary (autofocus) "Send Now" (`KJnHq3`) + hint "**ENTER** to send". Lottie animation (dark/light variants).
  - Cancel → `failureReason: SHOUTING_CANCELLED`; confirm → send. Analytics type "@Everyone Warning".
- **observed** Sibling check: content matching a Discord token regex triggers the same popout with "This message may contain a Discord authentication token, which can grant an attacker access to your account. Are you sure you want to send it?" (`sTwS1a`).
- **observed** Same validator also enforces length 2000 (4000 with premium) and slowmode before these checks.

## 2. @silent

Code: `web.355a76e660bee7a5.js` module 677413; message header `a3a7a5a9295a781c.js`; composer `22fbc347793fb277.js`.

- **observed** Detection + strip: regex `^@silent(\s|$)`; when it matches, content becomes `content.substring(7).trim()`.
  ```
  let i="@silent",r=RegExp(`^${i}(\\s|$)`);… Ay:function(e){return null==e?[!1,""]:null==e.match(r)?[!1,e]:[!0,e.substring(i.length).trim()]}
  ```
- **observed** Flag: `SUPPRESS_NOTIFICATIONS = 4096` OR'd into `flags`. The REST body field is `flags` (always sent, `0` if none). Applied in `_sendMessage`, forum/thread creation, and scheduled messages (create and edit). Scheduled-message edit re-prepends `"@silent "` to the content when the flag is set.
  ```
  [z,q]=(0,eI.Ay)(l);z&&(l=q,X=(0,a.UI)(X,D.pr7.SUPPRESS_NOTIFICATIONS))
  eN={…,message:{channelId:e,content:l,nonce:em,tts:E,message_reference:O,allowed_mentions:R,flags:X,…}}
  ```
- **observed** Composer indicator: only when the first block starts with `@silent`. The prefix text gets the `silentPrefix` CSS class (styled token). No separate icon or banner found.
- **observed** Rendering for others: messages with flag 4096 get a `BellZIcon` (1rem) in the username header, with tooltip "This is a @silent message." (`RO/KYj`).
  ```
  (0,o.Lt)(l.flags,Q.pr7.SUPPRESS_NOTIFICATIONS)&&e.push((0,i.jsx)(eM,{},"suppress-notifications"))
  ```
- **observed** A should-notify style function (`web.355a76e660bee7a5.js`) returns `false` first thing when `flags & SUPPRESS_NOTIFICATIONS`. **inferred**: this is the desktop-notification gate.

## 3. /tts

Code: `web.355a76e660bee7a5.js` (built-in commands, TTS store), settings `52b6e35ba37f71f0.js`.

- **observed** The built-in command is id `-4`, name `tts`, with description "Use text-to-speech to read the message to all members currently viewing the channel." (`jZcIid`). It takes one required string option `message`, "Your message" (`k+sw9g`). Executing it sends `{content, tts:true}`.
- **observed** Predicate: channel not private (**not offered in DMs or group DMs**), the "Allow playback and usage of /tts command" setting is on, AND `SEND_TTS_MESSAGES` is held.
  ```
  predicate:e=>{let{channel:t}=e;return null!=t&&!t.isPrivate()&&T.on.getSetting()&&N.A.can(D.xBc.SEND_TTS_MESSAGES,t)},execute:e=>({content:w(e,"message")??"",tts:!0})
  ```
- **observed** Wire: top-level `tts` boolean in the message body (see the `eN.message` excerpt above). The web code has no `/tts ` text-prefix parser; only the built-in slash command sets it.
- **observed** Setting `textAndImages.enableTtsCommand` (default `true`):
  - Title: "Allow playback and usage of /tts command" (`btbS+Z`)
  - Subtitle: "Using the /tts command speaks your message out loud. Turn this off if you prefer to see /tts messages as text only. You can control the speed of playback on the [Accessibility] …" (`Q5crhR`)
  - The same flag gates both sending and playback.
- **observed** Receiving: the client **does** speak TTS messages. On `MESSAGE_CREATE` (not optimistic, not self-deafened), it speaks if (a) `enableTtsCommand && message.tts` and the channel is the selected or sidebar channel, or (b) the "Text-to-Speech Notifications" mode (`NEVER | ALL_CHANNELS | SELECTED_CHANNEL`; options "Never" / "For all channels" / "For current selected channel") covers it and the author isn't you.
  - It skips blocked/ignored authors, muted channels, and duplicates (last 10 IDs).
  - Utterance: "{username} said {body}" (`uIj77/`) or "{username} replied to {replyUsername} {body}" (`Ix4H+j`). Mentions, roles, channels and emoji are expanded to names; spoilers are read as "Spoiler". Uses `window.speechSynthesis` with a locale-matched voice.
  ```
  o=eO.on.getSetting()&&n.tts&&l,d=iW.A.getTTSType(),c=n.author?.id!==en.default.getId()&&(d===H.aVn.ALL_CHANNELS||d===H.aVn.SELECTED_CHANNEL&&l)
  ```
- **observed** Permission copy: "Send Text-to-Speech Messages" (`mMbwh7`): "Allows members to send text-to-speech messages by starting a message with /tts. These messages can be heard by anyone focused on the channel." (`D6x8Nr`). There's also a TTS rate setting, "Text-to-Speech rate" (`lsW5Ev`).

## 4. Other per-send options (brief)

- **observed** Scheduled send: `POST /users/@me/scheduled-messages` with `{channel_id, content, scheduled_timestamp, flags, message_reference, allowed_mentions, sticker_ids, poll, attachments}`; edit with `PATCH /users/@me/scheduled-messages/{id}`. Copy: "Schedule Message" (`3+ii4F`), "Send now" (`JLba51`).
- **observed** Suppress embeds is a post-send action, not a send flag. `PATCH /channels/{c}/messages/{m}` with `{flags: flags | SUPPRESS_EMBEDS(4)}`. Shift-click on the embed's remove button skips the confirm modal; copy "Remove Embed" / "Remove Embeds". The `<url>` autolink syntax is a markdown rule (`autolink: before "<" after ">"`). **inferred**: the server suppresses the embed for `<url>`; there's no client flag.
- **observed** Other send-body fields: `allowed_mentions`, `message_reference` (reply/forward), `sticker_ids`, `poll`, `components`, `nonce`. `IS_GUILD_OFFICIAL` is OR'd into flags for official guilds.

## Gaps

- The desktop Electron cache wasn't inspected (sandbox). Web build 678976 only.
- Behaviour below 30 members, or for users without `MENTION_EVERYONE`, is server-side. The client only skips the warning.
- I didn't find a send-button or composer-banner indicator for @silent beyond the styled prefix token. "Silent message" (`t0MA8g`) exists in en-US but its web caller wasn't located.
- I didn't verify where the TTS-notifications setting is rendered or what its exact section heading is. Strings exist: "Text-to-Speech Notifications" (`VIm5MO`) and the search title "Speak all messages out loud" (`JZxxGx`).
- I didn't check whether the server rejects `tts:true` in DMs. The client just doesn't offer `/tts` there.
