# Discord Invite dialog — reverse-engineered

Source: public web client bundles fetched from `https://discord.com/app` on 2026-11-22 (no auth). The modal is module 137811 in
`a65390956ad8489f.js`; option tables in `79cd0aef8ef5e60c.js` (modules 735547 + 558213); link block in `567c71395304dded.js`
(module 312556). `«…»` = resolved en-US string. **observed** = read in the bundle; **inferred** = deduced.

## Entry and channel choice — observed

- Menu item `id:"invite-people"`: «Invite to Server» from the guild header / guild context menu, else «Invite to Voice» or «Invite to Channel» (`46f08f87fc61c511.js`).
- Can invite = `CREATE_INSTANT_INVITE` on the channel/guild, or the guild has a vanity code, or a stage instance carries an `invite_code`. Otherwise the item opens «Invite permission required» / «To add a friend to this server, ask someone with invite permissions for help.» / «Got It».
- Target channel: the passed channel, else the selected channel in this guild (thread → its parent) if viewable, else the guild's default channel that allows `CREATE_INSTANT_INVITE`.

## Main page — header — observed

- Title: «Invite friends to <$b>{name}</$b>» (guild name). Variants: «Invite to Stream», «Invite friends to {applicationName}», «Invite friends to event», «Invite friends to Stage».
- Subtitle: «Recipients will land in <channelNameHook>{channelName}</channelNameHook>» with the channel-type icon; «No Access» stands in for the name when it can't be shown.
- Student Hub guild (`HUB` feature): «Invite friends to the Student Hub for your school» / «A student email address is required to join.»
- Private target channel adds a warning row (yellow CircleErrorIcon): «This channel is private, only select members and roles can view this channel.»

## Friend list — observed

- Search input (size md, autofocus): placeholder «Search for friends». Shown only when there is at least one suggestion.
- Empty states: «No results found»; «Fetching your friends list...» while there is no invite code yet.
- Rows (48px): avatar 32 with status, name (nickname/display name, or group-DM name), second line = voice channel or activity (green) or username (muted). Group DMs use the channel icon, else guild initials.
- Row source with an empty query (`InviteSuggestionsStore`, `60686cd581ea2082.js`; builder `Us` in module 735547), capped at 100 rows:
  1. suggested users: user affinities plus the most recent DM recipient (kind `FRIEND`, `isSuggested: true`);
  2. private channels in `getPrivateChannelIds()` order: group DMs, and 1:1 DMs that have a last message (kind `DM`);
  3. all remaining friends (kind `FRIEND`).
- Excluded: blocked/ignored users, bots (DMs), users already shown, users already in the guild (except voice channels), users already in the voice channel or the channel's member list, activity participants.
- With a query: suggested users, DMs (limit 50), group DMs (limit 50), friends (limit 500), sorted by match score; guild members are kept and flagged.
- Voice channels split rows into two collapsible sections «Server Members» (already in the guild) and «Invite to Server».
- Row button: secondary small «Invite» → loading → «Sent» (disabled). On send failure it returns to «Invite». Rows for existing guild members show the text «Member» instead. Voice: optional ring button («Ring to Voice», «Stop Ringing», «You can only ring Friends», «Do Not Disturb»).
- Sending (module 774300 `InviteQueue`, `web.355a76e660bee7a5.js`): group DM → that channel; user → `ensurePrivateChannel(user.id)` (opens/creates the 1:1 DM) then sends. The message is a normal chat message whose `content` is the invite URL:
  ```js
  sendInvite(e,t,n,i,r){let a=(0,k.A)(t);return null!=r&&(a=`${r}\n${a}`),
    e$._sendMessage(e,{content:a,tts:!1,validNonShortcutEmojis:[],invalidEmojis:[]},…)}
  ```
  The URL builder (module 279208) is `${protocol}//${GLOBAL_ENV.INVITE_HOST}/${code}` → `https://discord.gg/<code>`. **observed**; that `k` is module 279208 is **inferred** from the nearest import.
- The queue is constructed with a 1010 ms constant (`SECOND + 10`). **observed**; that it spaces sends **inferred**.

## Link section — observed

- With friends shown: footer section labelled «Or, send a server invite link to a friend» (stream/activity/event/stage variants: «Or, send a stream invite link to a friend», «Send an activity invite link to a friend», «Or, send an event invite link to a friend», «Or, send a Stage invite link to a friend»).
- Without friends: body «Share this link with others to grant access to your server!» (owner) or «Share this link with others to grant access to this server»; footer checkbox «Set this link to never expire» (swaps max_age with 0 in state) and a gear button «Link Settings».
- Copy field: read-only input with the full URL and a brand button; after copying the label becomes «Copied» for 1 s and announces «Copied». Default button label comes from the shared CopyInput component — exact text not traced (**inferred** «Copy»). Streamer mode shows «Hidden; Use copy button».
- Expiry line (module 735547 `Be(maxAge, maxUses)`), followed by the link «Edit invite link.» (friends view only):

| max_age | max_uses = 0 | max_uses = N |
|---|---|---|
| 1800 | «Your invite link expires in 30 minutes.» | «Your invite link expires in 30 minutes, or after {N} use(s).» |
| 3600/21600/43200 | «Your invite link expires in {n} hour(s).» | «…in {n} hour(s), or after {N} use(s).» |
| 86400…5184000 | «Your invite link expires in {n} day(s).» | «…in {n} day(s), or after {N} use(s).» |
| 0 | «Your invite link will never expire.» | «Your invite link expires after {N} use(s).» |

- Errors under the link: `TOO_MANY_INVITES` (30016) → managers: «The maximum number of server invites have been generated. <inviteListHook>View invites for this server.</inviteListHook> <inviteHelpHook>Learn more</inviteHelpHook> about how server invitations work on Discord!»; others: «The maximum number of server invites have been generated. Admins can manage the list of invitations.»; other API errors → server message; fallback «Seems like something's gone wrong. Sorry about that.»

## Edit invite link page — observed

- Title «Server invite link settings»; actions «Cancel» (back to main) and «Generate a New Link» (creates, then returns).
- «Expire After» select (`MAX_AGE_OPTIONS`, seconds): 30 minutes `1800`, 1 hour `3600`, 6 hours `21600`, 12 hours `43200`, 1 day `86400`, 7 days `604800`, 14 days `1209600`, 30 days `2592000`, 60 days `5184000`, «Never» `0`. 14/30/60 days appear only when the default-expiration experiment sets that value as the default. An 8-hour option (`28800`) is defined but not listed.
- «Max Number of Uses» select: «No limit» `0`, 1 use `1`, 5 `5`, 10 `10`, 25 `25`, 50 `50`, 100 `100`.
- «Roles (Optional)» multi-select (placeholder «Select Roles») when the user can MANAGE_ROLES and has assignable roles; disabled while temporary is on.
- Toggle «Grant temporary membership» — «Temporary members are automatically kicked when they disconnect unless a role\nhas been assigned». Disabled while roles are picked; hidden when the guest-invite toggle applies or the guild uses manual membership approval.
- Voice + experiment `2023-02_guest_voice_invites`: toggle «Invite a Guest» — «Guests are automatically kicked when they leave the voice channel. They cannot see other channels in the server.» (flag `IS_GUEST_INVITE = 1`).
- Manual-approval guilds with KICK_MEMBERS: «Bypass Join Applications» — «New members joining through this link will join immediately without going through join applications.» (flag `IS_APPLICATION_BYPASS = 8`).

## Request — `POST /channels/{channel_id}/invites` — observed

On open (if the user can create invites):
```js
C.Ay.createInvite(i,{validate:t??null,max_age:A,max_uses:m,target_user_id:o,target_type:r,
  target_application_id:d?.id,temporary:v,flags:x},n)
```
On «Generate a New Link»: same fields without `validate`, plus `role_ids` (dropped when empty).
- `validate` = the currently stored invite code for that channel (lets the server reuse it), else `null`. **observed**; server-side reuse semantics **inferred**.
- `unique`: not sent anywhere in this flow. **observed** (absence)
- `target_type`: `1` STREAM (with `target_user_id`), `2` EMBEDDED_APPLICATION (with `target_application_id`); omitted for plain server invites.
- Defaults: `max_age` = existing invite's value, else 604800 (7 days; experiments `2025-08_default_invite_expiration_guild` / `2026-05_default_invite_expiration_guild_web` can make it 14/30/60 days; HUB guilds 0); `max_uses` = existing non-zero value else 0; `temporary` = existing else `false`; `flags` = existing else 0.
- Mobile/GDM share helper uses `{max_age: 86400}` only. **observed**
- Response dispatches `INSTANT_INVITE_CREATE_SUCCESS`; the code is read back from the invite store.
- Changing the target channel or the bypass flag regenerates automatically. Toggling «Set this link to never expire» on the main page only changes state; no regenerate call was found for it. **observed**

## Vanity URL — observed

- `showVanityURL` starts true when the user cannot create invites, the channel is not voice, and `guild.vanityURLCode` is non-empty; the link becomes `https://discord.gg/<vanity>` and the expiry line is replaced by «Using this server's custom invite link.»
- Generate with max_age 0, max_uses 0, not temporary, and vanity allowed shows the vanity link instead of calling the API.
- Event invites append the scheduled-event id to the code (`el.WU({baseCode, guildScheduledEventId})`); exact URL form not traced.

## Invites paused — observed

- Detection (module 347951): `guild.features.has("INVITES_DISABLED")` or the guild incident's `invitesDisabledUntil` is in the future.
- The invite modal itself has no paused-state branch (not found). Users see it as: guild tooltip «Invites paused for this server» (shown only to members with MANAGE_GUILD); join error code `INVITES_DISABLED` (40069) → «Invites are currently paused for this server. Please try again later. <$link>Learn More</$link>»; raid notice «Invites are paused» / «Direct messages and invites are paused».
- Managers toggle it in «Security Actions»: «Pause Invites» — «Temporarily stop new members from joining this server via invite or vanity links.»; when the feature is set permanently the switch is disabled with «Invites to this server are currently permanently disabled.» (`fc9b8e4a746831a7.js`).
- Creating an invite while paused: server presumably rejects and the error renders under the link. **inferred**
- Unused-in-web copy: «Invites Paused», «Invites are paused for this server. <$link>View invite settings</$link>», «Invites are paused in this server» exist in messages but no web component references them (probably mobile). **observed** absence
