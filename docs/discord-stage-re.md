# Discord Stage channels (type 13): client reverse-engineering notes

Source: public web client, `curl -s https://discord.com/app` on 2026-11-22. Entry bundle `web.355a76e660bee7a5.js`. Lazy chunks come from the webpack chunk map in that bundle (`p.u`): 269 chunks are named `<id>.<hash>.js`, the other ~6,300 only `<hash>.js`. All 6,600 chunks were downloaded and grepped statically; none were executed.

Strings come from the en-US message bundles `c30b23bab1166ada.js` (main) and `6772296caa105809.js` (secondary, `X.default.KEY`). Code looks up copy by a 6-char key: `intl.string(t.t.KEY)`. In the copy below, ICU plurals are written out by hand.

Tags: **observed** means read in the bundle, with an excerpt. **inferred** means reasoned from observed code but not seen directly.

## 1. Roles

| Role | Rule | Tag |
|---|---|---|
| Stage moderator | Has **all** of MANAGE_CHANNELS, MUTE_MEMBERS and MOVE_MEMBERS on the channel. `kg` ORs the bits together and `zy(e,t)` checks `(e&t)==t`. | observed |
| Speaker | Voice-state status is `ON_STAGE`, i.e. `suppress == false && request_to_speak_timestamp == null` | observed |
| Audience | Not a speaker and not a moderator (`isAudienceMember`). In the participant index, any non-speaker goes into the `AUDIENCE` bucket. | observed |
| Hand raised | `suppress == true && rts != null`, status `REQUESTED_TO_SPEAK` (1) | observed |
| Invited, waiting for the user | `suppress == false && rts != null`, status `REQUESTED_TO_SPEAK_AND_AWAITING_USER_ACK` (2) | observed |
| None | `suppress == true && rts == null`, status 0 | observed |

Voice-state status function (`web.355a76e660bee7a5.js`, module 105530):
```js
NONE=0, REQUESTED_TO_SPEAK=1, REQUESTED_TO_SPEAK_AND_AWAITING_USER_ACK=2, ON_STAGE=3
function l(e){return null==e?0:e.suppress&&null!=e.requestToSpeakTimestamp?1:
  e.suppress||null==e.requestToSpeakTimestamp?3*(!e.suppress&&null==e.requestToSpeakTimestamp):2}
```
Permission sets (module 233993):
```js
let a=i.kg(r.xBc.MANAGE_CHANNELS,r.xBc.MUTE_MEMBERS,r.xBc.MOVE_MEMBERS); // QY = moderator
o=i.kg(r.xBc.CONNECT,r.xBc.VIEW_CHANNEL)                                 // Gk = can join
```
`StageChannelRoleStore` (module 312006) computes `moderator` with `E.$3({permission:A.QY, user, context:guild, overwrites, roles})` and caches the result per channel and user. The cache is cleared on CHANNEL_UPDATES, GUILD_ROLE_UPDATE, GUILD_MEMBER_UPDATE and VOICE_STATE_UPDATES. **observed**

Which actions each permission unlocks (**observed** unless marked):
- "Speak on Stage" in the tray (go up without asking) needs MUTE_MEMBERS on the channel (`776781.Ni`). It shows while you are suppressed.
- "Invite to Speak" and "Move to Audience" in the user context menu need MUTE_MEMBERS.
- The "Allow requests from @everyone" toggle needs MANAGE_CHANNELS or MANAGE_ROLES.
- "End Stage for Everyone" needs the moderator role, or permission to manage the stage's scheduled event.
- Raising a hand needs REQUEST_TO_SPEAK on the channel (`370228.L`). Without it the button is disabled and reads "Raise Hand Disabled".

## 2. REST

| Action | Method and path | Body | Tag |
|---|---|---|---|
| Start a stage | `POST /stage-instances` | `{channel_id, topic, privacy_level, guild_scheduled_event_id, send_start_notification}` | observed |
| Edit a stage | `PATCH /stage-instances/{channel_id}` | `{topic, privacy_level}` | observed |
| End a stage | `DELETE /stage-instances/{channel_id}` | none | observed |
| Raise or lower your hand | `PATCH /guilds/{gid}/voice-states/@me` | `{request_to_speak_timestamp: ISO now \| null, channel_id}` | observed |
| Become a speaker, accept an invite, or decline an invite | `PATCH /guilds/{gid}/voice-states/@me` | `{suppress: false\|true, request_to_speak_timestamp: null, channel_id, silent?}` | observed |
| Leave the stage (yourself to audience) | `PATCH /guilds/{gid}/voice-states/@me` | `{suppress: true, channel_id, self_video: false, self_stream: false}` | observed |
| Invite a user to speak (context menu) | `PATCH /guilds/{gid}/voice-states/{uid}` | `{suppress: false, request_to_speak_timestamp: ISO now, channel_id}` | observed |
| Invite from the request list, or dismiss a request | `PATCH /guilds/{gid}/voice-states/{uid}` | `{suppress: false\|true, channel_id}` | observed |
| Move a user to the audience | `PATCH .../voice-states/{uid}` `{suppress:true, channel_id}`, then a second PATCH with `{suppress:true, channel_id, self_video:false, self_stream:false}` | | observed |
| Turn requests to speak on or off | `PUT /channels/{cid}/permissions/{guild_id}` | the @everyone role overwrite with the REQUEST_TO_SPEAK bit moved between allow and deny | observed |

Excerpts from `web.355a76e660bee7a5.js`. Routes:
```js
STAGE_INSTANCES:"/stage-instances",STAGE_INSTANCE:e=>`/stage-instances/${e}`,
UPDATE_VOICE_STATE:function(e){let t=arguments.length>1&&void 0!==arguments[1]?arguments[1]:"@me";
  return`/guilds/${e}/voice-states/${t}`}
```
Stage-instance calls (module 602146):
```js
RR:function(e){return i.Bo.del({url:r.Rsh.STAGE_INSTANCE(e)...
RY:...i.Bo.patch({url:r.Rsh.STAGE_INSTANCE(e),body:{topic:t,privacy_level:n}...
yG:...i.Bo.post({url:r.Rsh.STAGE_INSTANCES,body:{channel_id:e,topic:t,privacy_level:n,guild_scheduled_event_id:s,send_start_notification:a}
```
Voice-state calls (module 849736: `J7` raises a hand, `e7` sets your own suppress, `SA` invites, `lL` sets another user's suppress, `Tf` and `gt` move to the audience):
```js
J7: body:{request_to_speak_timestamp:t?new Date().toISOString():null,channel_id:e.id}
T/e7: body:{suppress:t,request_to_speak_timestamp:null,channel_id:e.id,...n?{silent:n}:{}}
SA: url:UPDATE_VOICE_STATE(n,t),body:{suppress:!1,request_to_speak_timestamp:new Date().toISOString(),channel_id:e.id}
```
- Starting a stage (`Nx`) joins the voice channel if you aren't in it, POSTs the instance, then calls `e7(channel, false, silent=true)`, which puts the starter on stage. **observed**
- Raising a hand while an invite is pending declines the invite instead (`e7(ch, true)`). **observed** (module 361619)
- Error 150027 `STAGE_CHANNEL_USER_NOT_ALLOWED_TO_SPEAK` on invite or promote shows a generic failure toast. **observed**
- An invite from the request list sends only `suppress:false`. The user's request timestamp stays set, so they enter the awaiting-acceptance state and the row shows "Invite Sent". **inferred** from the status function and `awaitingUserAck`.
- Moderators get the `REQUEST_TO_SPEAK_INITIATED` and `PROMOTED_TO_SPEAKER` analytics events. **observed**
- Public stages: joining from Discovery calls `joinGuild(guildId, {lurker:true})` first (module 129095). **observed**

## 3. Gateway

| Event / field | Use | Tag |
|---|---|---|
| `STAGE_INSTANCE_CREATE` (op name 83), `_UPDATE` (84), `_DELETE` (85) | dispatched as `{type, instance}`. The store keys instances by `channel_id` and indexes them by `guild_id`. DELETE removes by `instance.guild_id` and `instance.channel_id`. | observed |
| `GUILD_CREATE.guild.stage_instances[]`, `READY guilds[].stage_instances` | seed the store | observed |
| Stage-instance fields the client reads | `id, guild_id, channel_id, topic, privacy_level (1 PUBLIC, 2 GUILD_ONLY), guild_scheduled_event_id, send_start_notification, host_id` | observed |
| Voice state | `suppress`, `request_to_speak_timestamp` (kept as `requestToSpeakTimestamp`, default null), `self_video`, `self_stream`, `discoverable` | observed |
| `CHANNEL_DELETE` | removes the instance | observed |

```js
eH(["STAGE_INSTANCE_CREATE"],e=>{eY({type:"STAGE_INSTANCE_CREATE",instance:e})})
CONNECTION_OPEN:function(e){...t.forEach(e=>o(e.id,e.stage_instances))},
GUILD_CREATE:function(e){let{guild:t}=e;o(t.id,t.stage_instances)},
```
A stage is live exactly when it has an instance: `isLive(ch) = getStageInstanceByChannel(ch) != null`. **observed** (module 446600)

Client-side reactions to voice-state updates (**observed**):
- If you are a moderator in the channel and another user is suppressed with a request timestamp, the client creates a local ephemeral `STAGE_RAISE_HAND` message in the stage's text chat (module 529533).
- When your own state becomes awaiting-acceptance, the client opens the "invited to speak" modal and plays the `reconnect` sound. The sound setting is labelled "Invited to Speak". A desktop notification also fires: "You have been invited to speak in {channelName} about {channelTopic}."
- On `STAGE_INSTANCE_UPDATE` with `send_start_notification` set, a desktop notification fires: "{guildName} is hosting a live Stage" / "{username}: @everyone "{topic}" is starting. Listen in!"

## 4. UI states and copy

### Start / edit modal (`0e43f0dd6dd22acb.js`)
- Title: "What will you be talking about?". Subtitle: "Write a topic to start your Stage.". When editing, the title is "Edit Stage" and the subtitle is "Change the topic".
- Topic field: label "Stage Topic", placeholder "The debate is over: diet is better than regular", `maxLength:120`, required.
- Buttons: "Cancel" and "Start Stage", or "Save Changes" when editing. The primary button is disabled while the topic is empty.
- Checkbox "Notify @everyone". It appears only when no stage is running and you are allowed to notify. It is pre-checked when the guild has at most 50,000 members. **observed**
- Privacy is set with no picker. It defaults to `GUILD_ONLY`; an existing instance keeps its value. If a public stage has a topic under 20 characters, the modal shows "Is your topic descriptive enough? …" once. **observed**
```js
eu=Q.dD.GUILD_ONLY,[ec]=a.useState(V?.privacy_level??eu)
label:...q.t["5FPBOB"]/*Stage Topic*/,placeholder:...ZwWruY,maxLength:120
text:K?...Create Event:null==V?...s8mM8A/*Start Stage*/:...K344S7/*Save Changes*/
```
Before the stage starts, moderators see "Start the Stage" with "Pick a topic to start the conversation.". Everyone else sees "Waiting for the Stage to start" with "What will you be talking about today?". A waiting sound (`stage_waiting`) loops, and "Mute Music" / "Unmute Music" toggles it. **observed** (`fc9b5c3443825c91.js`, module 30108)

### Stage call view (`c7e5dc72699a60bc.js`)
- Two sections with collapsible headers, "Speakers" and "Audience", each with a count. A header is hidden when its count is 0.
- Speakers are video-style tiles. The number of columns depends on the view width: under 424 px → 1, under 624 → 2, under 824 or with chat open → 3, otherwise 4. Tile width is `floor(width/cols - 8)` and the aspect ratio is 16/9.
- The audience is a grid of avatar tiles. Each row holds `floor((width-32)/102)` tiles, padded with placeholders.
- Speaker tiles carry a "Stage Moderator" badge with a tooltip.
```js
j=a<424?1:a<624?2:a<824||p?3:4, f=Math.floor((a-32)/102)
label:es.intl.string(es.t.CduOkx/*Speakers*/) ... label:...es.t["3foUu5"]/*Audience*/
```
- Header button: "Show Requests" / "Hide Requests", with a badge count (shown as "99+" above 99). Only users with MUTE_MEMBERS see it.
- Summary strip: "{count} speaking" and "{count} in audience". For a count of 0 the first one reads "{count} is speaking". **observed** (`9cf4c852c9727cd1.js`)
- Header subtitle reads "Stage Channel" where a voice channel shows "Voice Channel".

### Audience sidebar (`9cf4c852c9727cd1.js`)
- Title "Audience", close button "Close", and a search field with placeholder "Search audience".
- Sections "Requests to Speak — {numHands}" and "Audience — {audienceSize}". While searching, the second reads "Audience — {filteredSize} of {totalSize}".
- Empty states: "No listeners yet" / "Audience members will show up here.". With no search match: "Nobody found".
- Header toggle: "Show Audience" / "Hide Audience" / "Show Audience, {count} request(s) to speak pending".

### Requests-to-speak panel (moderators, `fc9b5c3443825c91.js`)
- Header "Requests to Speak". The first row is a switch: "Allow requests from @everyone" (long form: "Allow requests to speak from @everyone"). The switch is visible only with MANAGE_CHANNELS or MANAGE_ROLES.
- Each row has two buttons:
  - "Invite to Speak". It switches to "Invite Sent" and disables while the user hasn't accepted. It is also disabled for the first 1 s after the row mounts.
  - "Dismiss".
- Empty state: "No Requests" / "Requests to speak will show up here.". The section header reads "Requests to Speak — {numHands}".
```js
let o=s?V.intl.string(V.t.h9rsTd/*Invite Sent*/):V.intl.string(V.t.f0T7hI/*Invite to Speak*/);
(0,h.K,{onClick:()=>(0,R.lL)(n,t,!1),disabled:s||a, ... onClick:()=>(0,R.lL)(n,t,!0) /*Dismiss*/
```

### Tray (bottom controls, `fc9b5c3443825c91.js`)
- Hand button (shown when connected):
  - "Request to Speak" normally.
  - "Cancel Request" when your hand is raised. The button turns green.
  - "Raise Hand Disabled" when you lack REQUEST_TO_SPEAK. The button is disabled.
  - "Speaker abilities are only for confirmed adults" when age-gated.
- Suppressed and holding MUTE_MEMBERS: "Speak on Stage" (moderator self-promotion).
- Not suppressed: "Leave the Stage" (move yourself to the audience).
- Moderators with a live instance: the disconnect button "Disconnect Quietly" has a caret menu containing "End Stage for Everyone".
```js
t?..NzITVo:e?..GCimTk/*Cancel Request*/:n?..hLbG5N/*Request to Speak*/:..e4WMvx/*Raise Hand Disabled*/
p&&c&&a&&(0,i.jsx)(ev,{channel:n}) /*Speak on Stage*/, p&&!a&&(0,i.jsx)(e_,{channel:n}) /*Leave the Stage*/
```

### Invited-to-speak prompts
- Modal (`1e5810fe3468efbd.js`):
  - Title "You have been invited to speak.", subtitle "Join the speakers in [stage icon] {channel}".
  - "Accept" calls `e7(ch,false)`. "Not Now" only closes the modal.
  - Closing the modal itself (its onClose) calls `e7(ch,true)`, which declines.
- Notice bar (web, the `INVITED_TO_SPEAK` notice): "You have been invited to speak." with "Accept" (`e7(false)`) and "Decline" (`e7(true)`). If blocked or ignored users are present it adds "Reminder, blocked or ignored accounts will hear you". **observed**
```js
title:..Ul1RJQ/*You have been invited to speak.*/,..actions:[{text:..L5eIZ2/*Not Now*/,onClick:()=>e()},
{text:..MMlhsr/*Accept*/,onClick:()=>k(!1)}], onClose:()=>(k(!0),Promise.resolve())
```

### User context menu
- Audience user (`b6a485e3fd34980d.js`): "Invite to Speak" (`SA`) when you have MUTE_MEMBERS. On yourself the item reads "Speak on Stage" (`e7(false)`). It is hidden for current speakers.
- Speaker (`c1c025b1a3712109.js`): "Move to Audience" (`gt`). On yourself, "Leave the Stage" (`Tf`). Needs MUTE_MEMBERS or being yourself.

### End stage
- Explicit confirm (`1715005c16609357.js`): "Are you sure you want to end the Stage for everyone?" / "The Stage will end immediately." with "Cancel" and "End Stage". Confirming DELETEs the instance and disconnects you.
- Channel-header "End Stage" (`455341c02bc5364b.js`): "Do you want to end the Stage?" / "A Stage with no speakers will automatically end after a few minutes."
- On disconnect (`008d0330c01c41d9.js`): "Do you want to also end the Stage?" with "No, just disconnect quietly" and "Disconnect and End". The client shows it only when the stage is live, you are a moderator and a speaker, and no other moderator is in the channel or speaking (module 106044). **observed**

### Full / limits
- Stage full modal (`cb9bee0241b8b38d.js`): title "Sorry, this Stage is full.". The body depends on boost tier:
  - "This Stage is at maximum capacity."
  - "This server will need to be Boosted to the next level to support more people. …"
- Video-stage caps follow `guild.max_stage_video_channel_users`: "This Video Stage is full!" / "This Video Stage is maxed out.".
- In the sidebar user-limit display, a `userLimit` of 10000 counts as unlimited for stages (`l&&1e4===s?0:s`). **observed**
- User-limit help text: "Limits the number of users that can connect to this Stage channel. Users with the Move Members permission ignore this limit…"

### Channel list / sidebar (`6b05ca0762af596a.js`, `a9aa27c6bf6acb8d.js`)
- The live stage's topic is the channel's subtitle: `renderSubtitle → this.props.stageInstance?.topic`. **observed**
- Under the channel, only speakers are listed as voice users (`speakerVoiceStates`). They are followed by one row with a headphones icon and "{count} in audience"; when collapsed it shows just the number. **observed**
- On the guild icon, the a11y label gets "Live stage active" appended. **observed**
- A "Live" pill exists, but it is generic (`402216`). No evidence ties it to the stage row specifically. **inferred, unverified**
```js
renderSubtitle=e=>{let{disableTooltips:t}=e,n=this.props.stageInstance?.topic;return null==n?null:(0,r.jsx)(t1.A,{...,children:n})}
aI:function(e){let{numAudience:i,collapsed:t}=e,l=t?i:w.intl.formatToPlainString(w.t["+v2pN2"]/*{count} in audience*/,{count:i})
```

### Join flow
- Join sounds: the client skips both the `user_join` and the `disconnect` sound when its own connection is to a stage channel. **observed**
  ```js
  if(null!=n&&np.Do.has(n))return;  // np.Do: L=new Set([I.rbe.GUILD_STAGE_VOICE])
  ```
- First-join coachmark (`a057c347da156ed6.js`): "Welcome to Stages!" with "Your mic will be muted unless you are invited to speak.", "You can leave and rejoin anytime without making noise.", "Request to speak to join the conversation!" and "Got it!". **observed**
- Audience members join suppressed. **inferred**: the server sets this; the client only reads `suppress`.
- Non-moderators get a warning modal before joining if blocked or ignored users are in the stage (`993838.j6`, `W0`). **observed**
- Stage-channel preview card (not yet joined): up to 5 speaker avatars, the topic, and "Join Stage", or "Channel Locked" without CONNECT. **observed** (module 623413)

## 5. Ordering and layout

- Participant sort key (module 597886, `C`): speakers first, then streams, then not self-muted, then camera on, then hoisted role position zero-padded (999 for no role), then `connectedOn`, then display name. **observed**
  ```js
  return`${t?"\0":"\x01"}${"STREAM"===o?"\0":"\x01"}${d}${c}${u}${a}${(0,s.A)(r,i)}`
  // d=selfMute?"\x01":"\0", c=selfVideo?"\0":"\x01", u=`${n?.position??999}`.padStart(3,"0")
  ```
- Buckets (`L`): `SPEAKER`, or `AUDIENCE` plus the hoisted role id (or `NO_ROLE`). Status adds `ALL_REQUESTED_TO_SPEAK` / `REQUESTED_TO_SPEAK_ONLY`, and the user's relationship adds `BLOCKED` / `IGNORED` / `FRIEND`. **observed**
- The requests list is sorted by `Date.parse(request_to_speak_timestamp)` followed by the user id, so oldest hand first. **observed**
- Screen-share streams count as separate `STREAM` participants and always sort into speakers. **observed**
- Above 100 audience members the client starts throttling list updates (`5e3`), and stops below 75. The unit of `5e3` is not confirmed; probably ms. **observed** (module 224648)
- Speaker cap: none found in the client. Caps exist only for video (`max_stage_video_channel_users`) and for the user limit. **not found**

## Copy table (en-US)

| Key | String |
|---|---|
| 5FPBOB | Stage Topic |
| ZwWruY | The debate is over: diet is better than regular |
| DDF0cJ | What will you be talking about? |
| bqQIwa | Write a topic to start your Stage. |
| YPdQOp | Edit Stage |
| I+9bLx | Change the topic |
| s8mM8A | Start Stage |
| K344S7 | Save Changes |
| Pe+Pwp | Notify @everyone |
| OYbHfv | Start the Stage |
| QGnDLs | Waiting for the Stage to start |
| CduOkx / 3182VD | Speakers |
| 3foUu5 / suRApw | Audience |
| chmM9N | {count} speaking ("{count} is speaking" at 0) |
| +v2pN2 | {count} in audience |
| vUrsZ9 | Audience — {audienceSize} |
| FsP3VU | No listeners yet |
| hLbG5N | Request to Speak |
| GCimTk | Cancel Request |
| e4WMvx | Raise Hand Disabled |
| ImQ4dW / gKGz7A | Show Requests / Hide Requests |
| TYZgzW | Requests to Speak |
| 5z7q5a | Requests to Speak — {numHands} |
| 7R24mX | No Requests |
| Rpr2s0 | Requests to speak will show up here. |
| GYCh0W | Allow requests from @everyone |
| f0T7hI / VUCWcO | Invite to Speak |
| h9rsTd | Invite Sent |
| moABMy | Dismiss |
| 8Joh+p / W6c/Vf | Speak on Stage |
| ezLpY6 / 6C6PJx | Leave the Stage |
| VK3vQy | Move to Audience |
| Ul1RJQ | You have been invited to speak. |
| Z+3bWy | Join the speakers in {stageChannel} |
| MMlhsr / MpO0px | Accept |
| L5eIZ2 | Not Now |
| 1YDv7a | Decline |
| sqnsSP | You have been invited to speak in {channelName} about {channelTopic}. |
| Fmx5y/ | End Stage for Everyone |
| saZaRb | End Stage |
| xrYNBI | Are you sure you want to end the Stage for everyone? |
| TP1D8l | The Stage will end immediately. |
| pADdJu | Do you want to also end the Stage? |
| mT7jwN | A Stage with no speakers will automatically end after a few minutes. |
| c6qKwr | Disconnect Quietly |
| pqPQL0 | Sorry, this Stage is full. |
| 7FHbPG | This Stage is at maximum capacity. |
| GMZqSi | Stage Moderator |
| 7vb2cc | Join Stage |
| sBDfo6 | Your mic will be muted unless you are invited to speak. |
| UVuXCs | Welcome to Stages! |
| OO7ndG | Live stage active |
| bZ4Okd | {guildName} is hosting a live Stage |
| qTelnO | {username}: @everyone "{topic}" is starting. Listen in! |
| CP3DC3 | Invited to Speak (sound setting) |
| EbvdH9 | (Required for Stage moderator) Allow members to add or remove Speakers. |
| SDX669 | (Required for Stage moderator) Allow members to set a topic to start the Stage and enable or disable requests to speak. They can also delete the Stage channel. |
| bizKz6 | (Required for Stage moderator) Allow members to force others to disconnect from this channel. |

## Open questions / not found

- **"Become a Speaker" and "Add to Speakers"**: neither string exists in any en-US bundle. The moderator self-promote copy is "Speak on Stage".
- **"Remove from Stage" (`r23NoB`), "Disconnect and end the Stage" (`wnWqGg`), "@everyone can request to speak" (`JcFI/U`), "Open Raised Hands List" (`KJnyvh`)**: the strings exist, but no code references them. Probably dead.
- **Stage header and Live pill**: how the stage view's header shows the topic and a "Live" pill was not traced. The header title comes from a helper (`F.nd`) that was not resolved. A listener count in the header was not found; the client shows "{count} speaking · {count} in audience" instead.
- **Live-stage bar under the server name**: not found. The only guild-level indicator found is the guild-icon a11y label "Live stage active". The guild header probably has an active-event banner, but that is unverified.
- **"Not Now" in the invite modal**: it only closes the modal. Whether that leaves the invite pending server-side was not verified; the code suggests it does.
- **Dismissing a request**: it sends `suppress:true` only. Whether the server clears `request_to_speak_timestamp` was not verified, though the panel appears to rely on it.
- **Speaker count cap**: none in the client. Any server limit is unknown.
- **Privacy**: the start modal never sets PUBLIC (no picker). Public-stage copy exists ("Your Stage is going public!"), but the flow that sets it was not traced.
