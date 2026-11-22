# Discord announcement channels (type 5): Publish & Follow, reverse-engineered

Source: Discord web client from `https://discord.com/app`, fetched 2026-11-22. The entry bundle is `web.355a76e660bee7a5.js`. Lazy chunks come from its `p.u` map (6597 chunks, served as `assets/<hash>.js`; a few use `<id>.<hash>.js`). They were downloaded to scratchpad `bundles/lazy/`. Copy comes from the en-US intl chunks in `bundles/intl/`. Everything here comes from reading minified code. None of it was seen running.

Tags: **[O]** observed in code or copy. **[I]** inferred from code shape or known API behaviour, not proven by this build.

Main files (scratchpad `bundles/`):
- `lazy/ceb73656db6e6af4.js`: message hover toolbar (`canPublish`), chat-input blocker (Follow), experimental announcement composer
- `lazy/2420b0c09d1ff9a5.js`: `ChannelFollowingPublishBumpStore` and the inline "Share with your followers!" bump, follower REST actions, crosspost author popout
- `lazy/78daf0e81489424c.js`: Publish confirm modal (module 10502)
- `lazy/9827a3a225b5c7c1.js`: Follow modal (module 464035). `lazy/296f27e9f3cb3000.js`: FollowSuccessModal
- `lazy/9b3cd218fe829ac6.js`: channel header `renderFollowButton`
- `lazy/e751ad05577cfaf5.js`: channel settings "Announcement Channel" toggle
- `lazy/8da2d6fa30f1639c.js`: delete and edit confirms for crossposted messages
- `lazy/2d8c427656447f31.js` / `c9a54cbdbf12488d.js`: SERVER tag selection
- `js/web.*.js`: message flags, REST routes, `crosspostMessage`, error codes, channel icons

## States

### Message flags
`CROSSPOSTED=1`, `IS_CROSSPOST=2`, `SUPPRESS_EMBEDS=4`, `SOURCE_MESSAGE_DELETED=8` **[O]**
```js
var na=((D={})[D.CROSSPOSTED=1]="CROSSPOSTED",D[D.IS_CROSSPOST=2]="IS_CROSSPOST",D[D.SUPPRESS_EMBEDS=4]="SUPPRESS_EMBEDS",D[D.SOURCE_MESSAGE_DELETED=8]="SOURCE_MESSAGE_DELETED",...
```

### Publishable message (module 812930, `tX.A`) **[O]**
A message is publishable when it is `type === DEFAULT`, has no poll, is not EPHEMERAL, and is not a forward (`messageReference.type === FORWARD`). System messages and replies (type 19) are excluded because only DEFAULT passes **[O]**.
```js
A:function(e){return!(e.type!==s.lAJ.DEFAULT||null!=e.poll||(0,i.Lt)(e.flags??0,s.pr7.EPHEMERAL)||("messageReference"in e?(0,l.A)(e):(0,l.m)(e)))}
```

### Who can publish, from the hover toolbar (`canPublish`) **[O]**
All of these must hold: the channel is `GUILD_ANNOUNCEMENT`, the guild has feature `NEWS`, the viewer has `SEND_MESSAGES`, and the viewer is the author or has `MANAGE_MESSAGES`. The message must also be publishable.
```js
L=t.type===es.rbe.GUILD_ANNOUNCEMENT&&null!=d&&d.features.has(es.GuildFeatures.NEWS)&&I&&(T||E)&&(0,tX.A)(n)
```
The permission copy agrees. SEND_MESSAGES in an announcement channel: "Allows members to publish their own messages to all servers following this Announcement Channel." MANAGE_MESSAGES: "…They can also publish messages by other members…" **[O]**

### Publish button placement
1. **Hover toolbar, always visible** (not only in the expanded/shift state). Key `"publish"`, icon `AnnouncementsIcon`. It sits after edit/forward/thread/guild-official and before delete/more **[O]**. Once published, the label changes to **"Published"** and the button is `disabled` **[O]**.
   ```js
   d?(0,a.jsx)(ns.qv,{label:L?eG.intl.string(eG.t["1kWJAr"]):eG.intl.string(eG.t.MFGE51),icon:t$.AnnouncementsIcon,onClick:()=>(0,tj.Le)(t,n),disabled:L},"publish"):null
   ```
2. **Inline bump under the message** (`renderPublishBump`, only when the channel is `GUILD_ANNOUNCEMENT` and the message is not a snapshot) **[O]**. Layout: an `AnnouncementsIcon` (xs), the heading "Share with your followers!", a brand-coloured "Publish" link, and a CircleX dismiss. Below that sits a muted "Don't show again" link **[O]**.
   - The bump shows on `MESSAGE_CREATE` when the message is not optimistic, the channel is not in the persisted hidden set, and these hold: the channel type is announcement, the message is publishable, the viewer has SEND_MESSAGES, the viewer is the author or has MANAGE_MESSAGES, and the message is not already CROSSPOSTED **[O]**. This store does **not** check the `NEWS` feature (the toolbar does) **[O]**.
   - The bump is removed on `MESSAGE_UPDATE` that sets CROSSPOSTED, on `CHANNEL_SELECT` (all cleared), on dismiss (`CHANNEL_FOLLOWING_PUBLISH_BUMP_DISMISSED`), and on "Don't show again" (`…_HIDE_PERMANENTLY`, which adds the channel to the local-storage key `ChannelFollowingBumpChannels`) **[O]**.
   - Clicking Publish on the bump dismisses the bump first, then opens the same confirm modal **[O]**.
   ```js
   if(!(null!=l&&l.type===er.rbe.GUILD_ANNOUNCEMENT&&(0,tn.A)(n)&&tt.A.can(er.xBc.SEND_MESSAGES,l)&&(r||tt.A.can(er.xBc.MANAGE_MESSAGES,l))&&!c.Lt(Number(n.flags),er.pr7.CROSSPOSTED)))return!1;ts.add(n.id)
   ```
3. **No message context-menu Publish was found.** The `Publish` key (MFGE51) appears only in the toolbar, the bump, and the experimental composer menu across all 6597 chunks, and `crosspostMessage` is referenced only by the confirm modal **[O, bounded negative]**.
4. **Experimental "send and publish" composer.** Experiment `2026-02-announcement-composer` (`announcementComposer`, default false) replaces the announcement-channel composer with a `CREATE_ANNOUNCEMENT_POST` input that has a title ("Announcement Name") and a hero image. Its "Send Announcement Options" menu has checkboxes "Create thread" and "Publish" (both default on). It sends `POST /channels/{c}/messages/announcement` with `publish`, `create_thread`, `title` **[O]**. This is off by default and should not be copied yet **[I]**.

### Published state (source channel)
- There is no badge on the message itself. The only marker is the toolbar label "Published", shown disabled **[O]**.
- Editing a CROSSPOSTED message as its author or as a manager opens a confirm first: "Are you sure?" / "This message will also be updated in all servers following this channel. It may take some time before all servers are updated." / Confirm, Cancel **[O]**. The edit PATCH sets `failImmediatelyWhenRateLimited` when crossposted **[O]**.
- Deleting a CROSSPOSTED message in an announcement channel changes the delete body to: "This message will be deleted in all servers following this channel. It may take some time before it's removed from all servers." **[O]**

### Crossposted message in a follower server
- The message arrives as a **webhook message** with `IS_CROSSPOST` and `message_reference {guild_id, channel_id, message_id}` pointing at the source **[O]**. The client's crosspost check (module 943667):
  ```js
  A:function(e){return e.type===r.lAJ.DEFAULT&&(0,s.Lt)(e.flags,r.pr7.IS_CROSSPOST)&&null!=e.messageReference&&e.messageReference.type!==r.SH7.FORWARD}
  ```
- **Author**: the webhook username has the form `"<Source Guild Name> #<channel>"`. The client gets the guild name by dropping the last space-separated token **[O]**. That this is how the server builds the name is **[I]**. The avatar comes from the webhook; that it is the source guild icon is **[I]** (server-side).
- **Tag**: a crosspost gets the bot-tag type `SERVER`, labelled **"SERVER"** with the aria/tooltip "Server". When `message_reference.guild_id` is Discord's own community-updates guild, it gets `OFFICIAL` instead **[O]**.
  ```js
  null!=n&&(0,i.A)(n)?t=(0,r.MZ)(n)?u.A.Types.OFFICIAL:u.A.Types.SERVER:l?.bot?t=u.A.Types.BOT:...
  ```
- **Author click** opens a *guild* popout (`guildId = messageReference.guild_id`, `name` = parsed guild name), not a user popout **[O]**.
  ```js
  if((0,f.A)(t)){let n=(t?.author?.username??"").split(" ").slice(0,-1).join(" "),{guild_id:l}=t.messageReference;if(null!=l)return(0,i.jsx)(r.default,{setPopoutRef:e.setPopoutRef,guildId:l,name:n})}
  ```
- **Source deleted**: when the message has `SOURCE_MESSAGE_DELETED`, its content renders as **"[Original Message Deleted]"** **[O]**.
  ```js
  t.hasFlag(g.pr7.SOURCE_MESSAGE_DELETED)?f.intl.string(f.t.JOtgSw):e
  ```
- Visibility and analytics use the source ids (`messageReference.message_id/channel_id/guild_id`) for crossposts **[O]**. Links clicked inside a crosspost are attributed to the source message **[O]**.
- There is no "FOLLOWED" badge and no reply-style header for crossposts in this build **[O, bounded: searched tag types and copy]**.

### Follow entry points
1. **Channel header button**: a secondary, small "Follow" button. It shows when `channel.type === GUILD_ANNOUNCEMENT && guild.features.has(NEWS)`. There is no permission check, so members see it too **[O]**.
   ```js
   showFollowButton:C?.type===eo.rbe.GUILD_ANNOUNCEMENT&&_?.features.has(eo.GuildFeatures.NEWS)||!1
   ```
2. **Chat-input blocker** (it replaces the composer): shown when the channel is "followable" (announcement plus NEWS) **and** the viewer lacks SEND_MESSAGES **[O]**.
   - Message: "Follow to get this channel's updates in your own server."
   - Subtitle, only when `usersSeenEver >= 1000`, rounded down to the thousand: "Over {count} other people are also following."
   - Primary button "Follow". If the viewer is lurking, a secondary "Join Server" is added.
   - Follower stats load on mount through `GET /channels/{c}/follower-stats`.

### Follow modal (`FollowModal`)
- Header: the source guild icon (LARGE), then the channel icon and channel name **[O]**.
- Title "Add this channel's updates to your server!". Body: "Select where you want these updates to go. You can remove this at any time in Server Settings > Integrations > Channels Followed." For an NSFW source it adds, in red: "Note: This is an age-restricted channel." **[O]**
- **"Send to"** is a server select (guild icon MINI + name). **"Select channel"** is a channel select (channel icon + name) **[O]**.
- Candidate channels: in every guild the user is in, the SELECTABLE channels whose type is in `{GUILD_TEXT}` only and where the user has `MANAGE_WEBHOOKS`. Guilds with no such channel are omitted **[O]**.
  ```js
  .filter(e=>N.XD.has(e.type)&&w.A.can(_.xBc.MANAGE_WEBHOOKS,e))   // XD = new Set([GUILD_TEXT])
  ```
- Defaults: the last destination (`lastChannelFollowingDestination`, updated on `CHANNEL_FOLLOWER_CREATED`). Picking a new guild selects its channel named `general`, or else the first channel **[O]**.
- Helper text under the selects: "Only servers and channels you have webhook permissions in are shown." When no guilds qualify, it reads in red: "Uh oh, you don't have webhook permissions in any server you're in. Ask your server admin to give you the permission or ask them to come follow themselves!" and the server select is disabled **[O]**.
- Footer: primary "Follow" (loading while submitting, disabled until both are picked) and secondary "Cancel" **[O]**.
- Errors appear inline in the helper slot. `30007 TOO_MANY_WEBHOOKS` gives "The selected channel has too many webhooks. Please remove a webhook or select another channel." Anything else gives "Failed to follow the channel, please try again." **[O]**
- **Success**: dispatch `CHANNEL_FOLLOWER_CREATED`, close, then open `FollowSuccessModal`. It shows a random illustration (theme-dependent set) and a random title from 10 ("What a nice addition to your server. Lookin' real {spiffy|good|dapper|swanky|snazzy|dandy|cozy|nifty|neato|lively} in here."). Body: "The most important updates from this channel will now show up directly in your server. Enjoy!" Button: "Got it!". There is no toast **[O]**.

### Channel settings toggle
- Shown when the channel type is in `{GUILD_TEXT, GUILD_ANNOUNCEMENT}`, the guild has `NEWS`, and the channel is **not** the rules channel or the public-updates channel **[O]**. It is disabled without Manage Channels (`canManageChannels`) **[O]**.
- Label "Announcement Channel". Description: `Users can Follow this channel in their own servers. Posts you "Publish" from this channel will appear in those servers. Learn more.`, followed by an info notice: "By creating an announcement channel, your server profile will be visible to others outside your server members. You can control Profile privacy in Server Settings > Profile." **[O]**
- On change it calls `updateChannel({type: on ? GUILD_ANNOUNCEMENT : GUILD_TEXT})` and saves through `saveChannel`. `"type"` is in the saved field list, which gives `PATCH /channels/{id} {type: 5|0}` **[O for the client payload; the PATCH verb comes from saveChannel, I]**.
  ```js
  handleNewsChange=e=>{(0,r.fy)({type:e?eW.rbe.GUILD_ANNOUNCEMENT:eW.rbe.GUILD_TEXT})}
  ```
- "Community-only": `NEWS` is the gate. That Community servers are what grant `NEWS` is **[I]**. The create-channel modal shows the same description (and the profile notice when the guild profile is not visible) for the "Announcement" type, with the description "A text channel that can post to servers following it" **[O]**.

### Sidebar icon and labels
- Icon (`gU`): NSFW gives `AnnouncementsWarningIcon`, a spoiler channel gives `AnnouncementsSpoilerIcon`, a limited/private channel gives `AnnouncementsLockIcon`, and otherwise `AnnouncementsIcon`. The rules channel overrides all of these with `BookCheckIcon` **[O]**.
- Aria labels: "Announcements", "Announcements (Limited)", "Announcements (Age-Restricted)", "Announcements (Spoiler)". The type name is "Announcement Channel" **[O]**.
- Composer placeholder: no announcement-specific placeholder exists in the default path. It is the same channel placeholder used by text channels **[O, bounded: placeholder branches only special-case voice/stage/threads]**.

## Copy (en-US)

| Key | String |
|---|---|
| MFGE51 | Publish |
| 1kWJAr | Published |
| hasZoU | Share with your followers! |
| efSkDm | Don't show again |
| aIz1oV / cY+Oob | Are you sure? / Confirm |
| 8FpqOs | This message will be sent to all servers following this channel. You can edit or delete this message later, and it will update correspondingly in each following server. |
| GCGrNP | This message will be sent to **{n} server(s)** following this channel. You can edit or delete this message later, and it will update correspondingly in each following server. |
| IMhGZz | Announcement stats can be found in [Server Insights.] |
| Vd1hs6 | Uh-oh, a pop up box. (title of the publish error alert) |
| 77cuqz | You've reached your 10 published messages per-hour limit. But we love the enthusiasm, so please try again in {retryAfter} minute(s). |
| z2gyNF / BddRzS | Sorry, something went wrong. / Okay |
| Whhv4w | You are being rate limited. |
| qoxdQB | To keep announcements speedy, published messages can only be edited 3 times per hour. Please try again in {retryAfterMinutes} minute(s). |
| grBcM8 | This message will also be updated in all servers following this channel. It may take some time before all servers are updated. |
| 2kHABX | This message will be deleted in all servers following this channel. It may take some time before it's removed from all servers. |
| PuJGuM / 39trQT | SERVER / Server |
| JOtgSw | [Original Message Deleted] |
| 3aOv+h | Follow |
| Hl0Mqh | Follow to get this channel's updates in your own server. |
| C5bgrC | Over {count} other people are also following. |
| RLch70 | Join Server |
| mvPFbA | Add this channel's updates to your server! |
| kbpkxJ | Select where you want these updates to go. You can remove this at any time in Server Settings > Integrations > Channels Followed. |
| DrNm/5 | Note: This is an age-restricted channel. |
| xFn72s / PDn2fR | Send to / Select channel |
| Z0quyN | Only servers and channels you have webhook permissions in are shown. |
| 6b6QoF | Uh oh, you don't have webhook permissions in any server you're in. … |
| 1eZ4aB | The selected channel has too many webhooks. Please remove a webhook or select another channel. |
| LgwhuN | Failed to follow the channel, please try again. |
| 2QbSea / +IrDzN | The most important updates from this channel will now show up directly in your server. Enjoy! / Got it! |
| Au2b7m | Announcement Channel (settings toggle) |
| tI7KNX | Users can Follow this channel in their own servers. Posts you "Publish" from this channel will appear in those servers. [Learn more.] |
| 2Ab4Id | By creating an announcement channel, your server profile will be visible to others outside your server members. … |
| sysCVV | A text channel that can post to servers following it |

## Limits

- Publish: **10 per hour** per channel, from the 429 copy. Minutes are computed as `Math.floor(body.retry_after/60)`. The request uses `failImmediatelyWhenRateLimited`, so the client does not retry **[O]**. That the limit is per channel rather than per user is **[I]**.
- Editing a published message: **3 per hour**. The error code `20022 CHANNEL_FOLLOWING_EDIT_RATE_LIMITED` shows an alert with "You are being rate limited." and the `qoxdQB` body, using `Math.ceil(retry_after/60)` **[O]**.
- Follow: `30007 TOO_MANY_WEBHOOKS` on the destination channel. That the webhook cap is 15 is **[I]**.
- The follower-stats cache is refetched when older than 1 hour (`927813.A.Millis.HOUR`) **[O]**.

## Requests and events

| What | Request | Notes |
|---|---|---|
| Publish | `POST /channels/{c}/messages/{m}/crosspost` (no body) | 429 shows `77cuqz`, other errors show `z2gyNF`, in an alert with title `Vd1hs6` and button "Okay" **[O]**. Afterwards the flag update arrives by `MESSAGE_UPDATE` with CROSSPOSTED; the bump store listens for it **[O]**. That the gateway emits this event is **[I]**. |
| Confirm modal stats | `GET /channels/{c}/follower-stats` → `guildsFollowing`, `usersSeenEver` | `CHANNEL_FOLLOWER_STATS_FETCH_START/SUCCESS` **[O]** |
| Follow | `POST /channels/{source}/followers {webhook_channel_id: dest}` | Then a client-only dispatch, `CHANNEL_FOLLOWER_CREATED {channelId, guildId}` **[O]** |
| Edit a published message | `PATCH /channels/{c}/messages/{m}` | With `failImmediatelyWhenRateLimited` when CROSSPOSTED **[O]** |
| Toggle the type | `PATCH /channels/{c} {type: 5 or 0}` via saveChannel | **[O/I]**, see above |
| Send and publish (experiment) | `POST /channels/{c}/messages/announcement {…, publish, create_thread, title}` | **[O]**, gated |
| Other routes | `CHANNEL_FOLLOWER_MESSAGE_STATS: /channels/{c}/follower-message-stats` | Exists; not traced **[O]** |
| Client actions | `CHANNEL_FOLLOWING_PUBLISH_BUMP_DISMISSED {messageId}`, `CHANNEL_FOLLOWING_PUBLISH_BUMP_HIDE_PERMANENTLY {channelId}` | **[O]** |

## Observed vs inferred (summary)

Observed in code: flag values, publishability rules, the canPublish permission formula, toolbar and bump placement and behaviour, confirm copy with follower count, rate-limit copy and math, follow modal filters (GUILD_TEXT plus MANAGE_WEBHOOKS), follow errors and success modal, header and blocker Follow entry points, the settings toggle gate and payload, icons, the SERVER tag, the crosspost author popout, and "[Original Message Deleted]".

Inferred: that the server builds the webhook name as `"<Guild> #<channel>"` with the source guild icon as avatar (the client parsing is observed), that the gateway sends MESSAGE_UPDATE after a crosspost, that the publish limit is per channel, that NEWS comes from Community, the webhook cap value, and the HTTP verb of saveChannel.

Not covered: unfollow and the "Channels Followed" settings list (copy exists: "Channels Followed", "Unfollow", "Are you sure you want to unfollow this channel? …"), and mobile clients.
