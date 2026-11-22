# Discord: voice channel status (RE notes)

Source: public web client bundle fetched 2026-11-22 from `https://discord.com/app`. Copy resolved from the en-US intl chunk `c30b23bab1166ada.js`. Static reading only.

Labels: **[obs]** read in the bundle. **[inf]** inferred. `Not verified:` no evidence.

## REST

- **[obs]** Route `UPDATE_VOICE_CHANNEL_STATUS: e=>`/channels/${e}/voice-status`` (`web.355a76e660bee7a5.js`).
- **[obs]** Call (`8525118cea2acab5.js`):
  ```
  updateVoiceChannelStatus:function(e,t){return r.Bo.put({url:s.Rsh.UPDATE_VOICE_CHANNEL_STATUS(e),body:{status:t},...})}
  ```
  `PUT /channels/{channel_id}/voice-status` with body `{"status": "<string>"}`. The modal treats **204** as success (`204===e.status`). Any other response is parsed as an error and shown under the field.
- **[obs]** Clearing sends the same PUT with `status: ""` (`updateVoiceChannelStatus(n.id,"")`).
- **[obs]** Before sending, the client rewrites typed `:emoji:` into `<:name:id>` markup (`h.Ay.parse(t,D)` then `r.content`). It validates custom emoji availability with `validateMessage(invalidEmojis)` and shows the error on failure.

## Gateway

- **[obs]** Dispatch `VOICE_CHANNEL_STATUS_UPDATE` → `{id, guild_id, status}`:
  ```
  eH(["VOICE_CHANNEL_STATUS_UPDATE"],(e,t)=>{eY({type:t,id:e.id,guildId:e.guild_id,status:e.status})})
  ```
- **[obs]** Request op **43 `REQUEST_CHANNEL_INFO`**, payload `{guild_id, fields}`. The client sends `fields:["status","voice_start_time"]`:
  ```
  requestChannelInfo(e,t){this.send(eQ.REQUEST_CHANNEL_INFO,{guild_id:e,fields:t})}
  ```
- **[obs]** Reply dispatch `CHANNEL_INFO` → `{guild_id, channels:[{id, status, voice_start_time}]}`. It replaces the whole per-guild map, so channels missing from the reply have no status.
- **[obs]** Related dispatch `VOICE_CHANNEL_START_TIME_UPDATE {id, guild_id, voice_start_time}`.
- **[obs]** Fetch policy (`52074`, `882840`): the first time any GUILD_VOICE channel of a guild needs a status, and the guild is available, the client sends op 43 once per guild. It records the guild in `hasRequestedStatuses`. That set is cleared on `CONNECTION_OPEN` / `CONNECTION_RESUMED`, and the guild's entry on `GUILD_CREATE` / `GUILD_DELETE`, which triggers a re-request.
- **[obs]** Store: `VoiceChannelStatusStore`, `o[guildId][channelId] = status`. Only `type === GUILD_VOICE` returns a status. Stage channels have none.
- **[inf]** Statuses are not sent in READY or GUILD_CREATE channel objects. The client always fetches them lazily through op 43.

## Permission

- **[obs]** `SET_VOICE_CHANNEL_STATUS: 1<<48` (`M.vI(48)`), in both `web.*.js` and `fast-connect.*.js`.
- **[obs]** Helper `532622`:
  ```
  let l=[SET_VOICE_CHANNEL_STATUS,CONNECT,VIEW_CHANNEL],o=[SET_VOICE_CHANNEL_STATUS]; Ay(channel, onlyStatusPerm=false)
  ```
  The default check requires all three bits. Some callers pass `true` to check only bit 48.
- **[obs]** Permission copy: name `Set Voice Channel Status`. Descriptions: `Allows members to create and edit voice channel status.` and, under an experiment, `... While connected to voice, members can also set an image banner for the call.`

### Who can edit (observed gating)
| Surface | Shows edit when |
|---|---|
| Sidebar row (`r9`) | connected to this channel **and** has bit 48 |
| Call header | in call **and** status non-empty. Clickable if it passes the full check (48+CONNECT+VIEW) |
| Context menu `Set Channel Status` | connected (`isInChannel(self)`) **and** full check |
| Context menu `Clear Channel Status` | **not** connected **and** MANAGE_CHANNELS **and** status non-empty |

- **[obs]** Context-menu precedence (`de16409460678b24.js`):
  ```
  !C&&h&&v ? clear-status : C&&p ? set-status : null     // C=connected, h=MANAGE_CHANNELS, v=has status, p=perm
  ```
- **[inf]** Members who are not connected cannot set a status from the client. Moderators with MANAGE_CHANNELS can only clear it from outside the channel. Not verified: what the server enforces.

## Rendering

- **[obs]** Sidebar row subtitle (`6b05ca0762af596a.js`, `r9`):
  - Status non-empty: the status is rendered with `parseVoiceChannelStatus(status, true, {channelId})`, `body-xs/medium`. It is clickable (opens the modal) only when connected and the user has bit 48 (`handleVoiceStatusClick` checks `connected`). It fires the `VOICE_CHANNEL_TOPIC_VIEWED` analytics event.
  - Status empty and connected, with permission, no hangout window, and (no other subtitle or hovered): placeholder `Set a channel status` plus a `PencilIcon` size `xxs`.
  - Otherwise: the stage topic or other subtitle, or nothing.
- **[obs]** Call header (`9cf4c852c9727cd1.js`): rendered in `childrenBottom` under the title when `isGuildVoice && inCall && status.length>0`. Tooltip `Set a channel status` (500 ms delay, bottom) shows only when editable. Variant `body-xs/normal`.
- **[obs]** User profile voice card (`9a8ff8962727393c.js`): shows the status, or `Set a channel status` plus a pencil (aria `Edit`) when connected and permitted.
- **[obs]** Markdown subset: `VOICE_CHANNEL_STATUS_RULES` is the default rules minus `inlineCode, codeBlock, br, blockQuote, autolink, url, attachmentLink, mention, roleMention, channelMention, channelOrMessageUrl, mediaPostLink, subtext, soundboard, gameMention`. That leaves text, emphasis, strikethrough, spoiler, unicode and custom emoji (emoji click disabled), and timestamps. No links and no mentions.
- **[obs]** Before parsing, game-mention tokens are replaced (`208971.G`).
- **[obs]** Other copy: `Channel status: {channelStatus}`, `Status for {channelName}`.

## Edit UI: modal (not popover)

`VoiceChannelStatusModal`, module `136523` in `web.355a76e660bee7a5.js`:
- **[obs]** Title `What are we chatting about?`. Subtitle `Let others know what you're up to in the voice channel!`. Header image graphic.
- **[obs]** Field label `Status`. Placeholder `Status for {channelName}`. It is a rich chat-input (`type: VOICE_CHANNEL_STATUS`) with an emoji picker. `canMentionRoles:false`, `canMentionChannels:false`, `allowNewLines:false`, autofocus.
- **[obs]** Limits: `maxCharacterCount:500` and `showRemainingCharsAfterCount:250`, so the remaining-chars counter appears at 250. `Set Status` is disabled when `length > 500`, measured on the raw input string before emoji conversion.
- **[obs]** Buttons: `Cancel` (secondary) and `Set Status` (primary, shows loading). Enter submits. If the value equals the current status, the modal closes without a request.
- **[obs]** Analytics: `VOICE_CHANNEL_TOPIC_SET {raw_length, text_length}`, where text_length counts custom emoji as 2 chars (`replace(/<(a)?:[^:]+:[0-9]+>/g,"--")`).

State machine:
```
idle --open(modal)--> editing --Enter/Set Status--> submitting --204--> closed
                                            \--error--> editing(errorMessage)
editing --Cancel/Esc--> closed ; value unchanged + submit --> closed (no request)
```

## Audit log

- **[obs]** Action types `VOICE_CHANNEL_STATUS_CREATE: 192`, `VOICE_CHANNEL_STATUS_DELETE: 193`.
- **[obs]** Copy: `{userHook} set a Voice Channel Status in {channelHook}`, `{userHook} removed a Voice Channel Status in {channelHook}`. Filter labels `Create Voice Channel Status` / `Delete Voice Channel Status`.
- **[obs]** AutoMod copy: `has blocked a voice channel status for {channelHook}`, `has flagged a voice channel status for {channelHook}`. AutoMod applies to statuses.

## Unverified
- Not verified: whether the server clears the status automatically when the channel empties.
- Not verified: the server-side length limit. The client blocks above 500.
- Not verified: whether `VOICE_CHANNEL_STATUS_UPDATE` reaches clients that have never sent op 43 for that guild. The store accepts it either way.
- Not verified: the server-side rule for a non-connected user who has bit 48.
