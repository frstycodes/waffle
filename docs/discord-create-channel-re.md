# Discord "Create Channel" modal — reverse-engineered

Source: public web client bundles fetched from `https://discord.com/app` on 2026-11-22 (no auth). The modal is module 410380 in
`042749a0b7eabc9c.js` unless noted. `«…»` = resolved en-US string. **observed** = read in the bundle; **inferred** = deduced.

## Channel type numbers (`web.355a76e660bee7a5.js`, module 478437) — observed

`GUILD_TEXT 0 · GUILD_VOICE 2 · GUILD_CATEGORY 4 · GUILD_ANNOUNCEMENT 5 · GUILD_STORE 6 · GUILD_STAGE_VOICE 13 · GUILD_FORUM 15 · GUILD_MEDIA 16 · GUILD_APP 21`

## Step 1 — channel info

- Title: «Create Channel» («Create Category» for type 4, «Duplicate Channel» when cloning). Subtitle «in {categoryName}» when opened from a category. **observed**
- Modal width 496. **observed**
- Field order: Channel Type radio → name → store/game/app extras → announcement notes → privacy toggle → server-web-page content select. **observed**
- Opens with the type passed by the caller, default `GUILD_TEXT`; name input is focused and selected. **observed**

### Channel Type options (radio, label «Channel Type») — observed

Rendered in this order; icon swaps to the lock variant while Private is on.

| # | label | description | value | icon (private icon) | shown when |
|---|---|---|---|---|---|
| 1 | Text | «Send messages, images, GIFs, emoji, opinions, and puns» | 0 | TextIcon (TextLockIcon) | always |
| 2 | Voice | «Hang out together with voice, video, and screen share» | 2 | VoiceNormalIcon (VoiceLockIcon) | always |
| 3 | Forum | «Create a space for organized discussions» | 15 | ForumIcon (ForumLockIcon) | always |
| 4 | LFG | «Create a space to invite others.» | forum 15 + flag | ExperimentalLfgIcon | experiment `2026-06-game-invites-channel` |
| 5 | Media | «Share videos, photos, and files with your members.» | 16 | ImageIcon (ImageLockIcon) | guild has `COMMUNITY` and (`CREATOR_MONETIZABLE` or `CREATOR_MONETIZABLE_PROVISIONAL`), or `INTERNAL_EMPLOYEE_ONLY` |
| 6 | Announcement | «Important updates for people in and out of the server» | 5 | AnnouncementsIcon | guild feature `NEWS` |
| 7 | Store | «Sell your game directly on Discord» | 6 | TagIcon | feature `COMMERCE` and the guild has applications |
| 8 | Stage | «Host events, panels, and Q&As for an audience» | 13 | StageIcon | feature `COMMUNITY` and user can MANAGE_ROLES and MANAGE_CHANNELS |
| 9 | App | «Create a space to interact with an app» | 21 | AppsIcon (AppsLockIcon) | experiment `2026-07-app-channels` |

- The type selector is hidden for categories and when duplicating. **observed**
- Turning Private on while Announcement is selected switches the type to Text. Picking Stage turns Private off. **observed**
- Type-specific notes under the name: Announcement → «Users can Follow this channel in their own servers. Posts you "Publish" from this channel will appear in those servers. <$link>Learn more.</$link>» and, if the guild profile is not visible, «By creating an announcement channel, your server profile will be visible to others outside your server members. You can control Profile privacy in Server Settings > Profile.» Forum → helper «Setting a clear title and specific purpose on your forum helps your members engage more! <forumUpsellHook>See Examples</forumUpsellHook>». **observed**

### Name field — observed

- Label «Channel Name» («Category Name»); `maxLength: 100`; placeholder «new-channel» («New Category» for categories, «forum-discussion» for forum); leading icon = current type icon (none for category); trailing emoji button (Unicode emoji only, inserted at the caret).
- Normalization runs on every keystroke and on type change (module 965805 → 842937, `web.355a76e660bee7a5.js`). It applies only to `LIMITED_CHANNEL_NAME = {0,5,10,15,16,19,21}` (text, announcement, forum, media, app, threads):
  ```js
  e.replace(/[\s-~]+/g,"-").replace(/^-+/,"")
   .replace(/[\\'!"#$%&()*+,./:;<=>?@[\]^`{|}~]/g,"").toLowerCase()
  ```
  So runs of whitespace, `-` or `~` collapse to one `-`, leading dashes are stripped, that ASCII punctuation set is removed, and the result is lowercased. A trailing dash is kept; emoji and non-ASCII letters pass through. **observed**
- Voice, Stage and Category names are not normalized (free text, spaces and case kept). **observed**
- Submit disabled when the name is empty or whitespace-only. **observed**

### Private toggle — observed

- Hidden for Announcement, Stage and when duplicating.
- Channel: label «Private Channel», description «Only selected members and roles will be able to view this channel.», LockIcon.
- Category: label «Private Category», description «By making a category private, only select members and roles will be able to view this category. Linked channels in this category will automatically match to this setting.»
- Requires the creator's own permissions, else an inline error and disabled submit: text/announcement/app need View Channels («<$b>View Channels</$b> permission required to create this private channel.»); voice needs View + Connect («<$b>View Channels</$b> and <$b>Connect</$b> permissions required to create this private channel.»); category the same with «…private category.» (`a4e2a48686b4c1f5.js` module 495273).
- Footer: «Cancel» + primary «Create Channel»/«Create Category»; when Private is on (or type is Stage) the primary becomes «Next» and opens step 2.

## Step 2 — add members or roles — observed

- Title «Add members or roles» (Stage: «Add Stage moderators» plus «Stage moderators are Speakers who can add and remove other Speakers. They can also start the Stage event. Stage moderators don't have to be mods of the server.»).
- Subtitle: the typed channel name with the type icon.
- Search placeholder «e.g. Moderators, @wumpus»; hint «Add individual members by starting with @, or type a role name». List shows role and member sections.
- Actions: «Back» (returns to step 1, state kept) and primary «Skip» when nothing is picked, else «Create Channel» / «Create Category».

## Request — `POST /guilds/{guild_id}/channels` — observed

Builder: `createChannel` in module 755584 (`web.355a76e660bee7a5.js`).
```js
let g={type:r,name:A,permission_overwrites:E};
null!=m&&64e3!==m&&(g.bitrate=m),null!=u&&u>0&&(g.user_limit=u),null!=N&&(g.parent_id=N)
```
- Always: `type`, `name`, `permission_overwrites` (`[]` default). Optional: `bitrate` (omitted when 64000; only set when duplicating), `user_limit` (>0, duplicating), `parent_id` (the opened-from category; never for categories), `flags`, `available_tags`, `game_id`, `sku_id`+`branch_id` (store), `application_id` (app). **observed**
- `name` is sent as displayed (already normalized). **observed**
- Success requires HTTP 201; for some types the client then navigates to the channel. Error body fields are shown inline; fallback «An error occurred, please try again.» **observed**

### permission_overwrites — observed

Overwrite `type`: `0` role, `1` member (module 155718). `@everyone` uses the guild id. Builders in module 147036.

| case | overwrites |
|---|---|
| public text/voice/forum/media/app | `[]` |
| announcement | `[{id: guild_id, type: 0, deny: SEND_MESSAGES, allow: 0}]` |
| private text-like (0,5,15,16,21) | `@everyone` deny `VIEW_CHANNEL`; each picked role/member allow `VIEW_CHANNEL` |
| private voice (2) | `@everyone` deny `VIEW_CHANNEL \| CONNECT`; picks allow `VIEW_CHANNEL \| CONNECT` |
| private category (4) | `@everyone` deny `VIEW_CHANNEL \| CONNECT`; picks allow `VIEW_CHANNEL \| CONNECT` |
| stage (13) | `@everyone` untouched; each pick allow `MANAGE_CHANNELS \| MUTE_MEMBERS \| MOVE_MEMBERS` (stage moderator), deny 0 |
| duplicate | the source channel's overwrites, bitrate, user_limit |

- Private: the `@everyone` deny is added even with zero picks ("Skip"). If none of the creator's roles is among the picked ids and the creator is neither admin nor owner, a member overwrite for the creator (same allow) is appended. **observed**
- `allow`/`deny` are permission bitfields serialized as strings by the HTTP layer. **inferred**

### Other flags — observed

- LFG option: sends `type: 15`, `flags` includes `IS_GAME_INVITES_CHANNEL`, `available_tags: [{name: "No Mic"}]`, `game_id`.
- Server-web-page select, label «Discoverable» (`SWP_CHANNEL_SUMMARIES_EXCLUDED` / `SWP_CHANNEL_MESSAGES_EXCLUDED` channel flags, options «All Content» / «Summaries» / «Messages» / «None») shows only when the guild has that feature; private channels show a read-only «None». Sent in `flags` only when non-zero.

## Category create — differences — observed

- Same modal with `channelType: 4`: no type selector, label «Category Name», placeholder «New Category», no leading icon, no normalization, no `parent_id`.
- Private Category copy as above; deny/allow include `CONNECT` as well as `VIEW_CHANNEL`.
- Primary button text «Create Category» on both steps.
