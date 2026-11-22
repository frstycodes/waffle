# Discord moderation surfaces: role-mention popout, Mod View, Members page (code-derived spec)

Source: the public web client (`discord.com/app`, build `web.355a76e660bee7a5.js` plus 6,600 lazy chunks from `discord.com/assets/`), read as text on 2026-11-22. Nothing was executed. Module ids are minified and change between builds. English labels come from the client's own en-US intl bundles (hashed key → string). Chunk file names are given where a module lives outside `web.*.js`.

Tags: **observed** = read in the bundle. **inferred** = deduced from code shape or naming, not read directly. "Gate-7" means having any of ADMINISTRATOR, MANAGE_GUILD, BAN_MEMBERS, KICK_MEMBERS, MODERATE_MEMBERS, MANAGE_ROLES, MANAGE_NICKNAMES at guild level (402655 `M`, computed with `checkElevated:false`). **observed**

---

## 1. `@role` mention popout (593284, web)

### When it opens
- The markdown rule for `<@&id>` looks the role up locally. If the role is **unknown**, it emits plain text `@unknown-role` ("YV4F/n"). That text has no colour and no popout. **observed**
  `if(null==s)return{type:"text",content:\`@${b.intl.string(b.t["YV4F/n"])}\`};`
- Known role → `{type:"mention", roleId, roleName:"@name", colorString, roleColors…}`. The mention renderer sends roleId / `@everyone` / `@here` to 593284 `A`. **observed**
- 593284 `A` wraps the mention in a popout only when it is not an inline preview, both channelId and guildId are set, and (roleId ≠ null or roleName === "@everyone"). **observed**
  `return c||null==n||null==s||null==t&&"@everyone"!==a?(...P()):(... _.Y,{... position:"right"`
  - **@here** therefore gets the styled pill with no popout. **observed**
  - **@everyone** gets a popout. roleId is null, so the list uses the everyone role (id = guild id, via 260509 `af` → `castGuildIdAsEveryoneGuildRoleId`). **observed**
- Popout position: `right` of the mention. Opening it is not gated by any permission in the client. **observed**

### Preload (runs before the popout renders)
- `preload: async()=>{null!=t&&await (0,S.a)(s,t)}`: only for a real roleId (never for @everyone). **observed**
- 545868 `a(guildId, roleId)`: `GET /guilds/{g}/roles/{r}/member-ids`. The result is cached per `g-r` in an LRU with `maxAge:1e4` (10 s), so a second open within 10 s doesn't refetch. The returned id array is then passed to `requestMembersById(guildId, ids, false)` → dispatch `GUILD_MEMBERS_REQUEST {guildIds, userIds, presences:false}`. That is gateway op 8 (the gateway mapping is **inferred**). **observed**
  `a.Bo.get({url:d.Rsh.GUILD_ROLE_MEMBER_IDS(e,t)…}).then(t=>(l.A.requestMembersById(e,t.body,!1),t.body.length))`
- **100-member cap**: no `100` appears anywhere in the client path. The cap, if it exists, is enforced by the server's response size. The client renders whatever members are already cached. **inferred / not verified**

### Layout (`F`)
- The role is resolved from local state. **Null role → the popout renders nothing.** **observed**
- Rows = `GuildMemberStore.getMembers(guild)` filtered by `roles.includes(roleId)` (unfiltered for @everyone), dropping members whose user isn't cached. **observed**
- Sorting: lodash `sortBy(m => m.nick ?? displayName(user))`. Plain JS string order, so it is case-sensitive and there is no locale collation. **observed**
  `.sortBy(e=>e.nick??U.Ay.getName(M.default.getUser(e.userId))).value()`
- Wrapped in a scroller (`c.Ip`):
  1. **Header** (282006): role icon (16 px, if any; right-click opens the image menu) + role name + `\xa0— {count}` (locale-formatted). Screen readers hear "{title}, {count} members" ("Uaqbke"). With no count (e.g. @everyone) it shows the title only. **observed**
  2. **Member rows** (`k`): the standard member-list item (110574) with the member's colour, owner crown (`isOwner`), boost (`premiumSince`), and `colorRoleName: role.name`. Click opens the user popout (342296, `roleId` passed, spacing 14). Right-click opens the guild user context menu 107632 with `showMediaItems:true`. **observed**
  3. **Footer**: when the count is known and larger than the number of rendered rows, it shows "+{count} more" ("9oMmZC") with `count = total − rows`. It is plain text, with no "View all" link. **observed**
- Count source: 386784 → 642133 `GuildRoleMemberCountStore`, filled by `GET /guilds/{g}/roles/member-counts` (a map of roleId→count for the whole guild, refetched only after 120 s: `Date.now()-t>12e4`). Gateway `GUILD_ROLE_MEMBER_ADD/REMOVE/BULK_ADD` adjust it in place. **observed**

---

## 2. Mod View (sidebar `GUILD_MEMBER_MOD_VIEW`)

### Gate
- `985925 r(guildId)` = Gate-7 for the current user. The panel closes itself when the gate turns false (`useEffect(()=>{R||l()})`). **observed**
- Kick, Ban and Timeout buttons each have their own gate (below). **observed**

### Entry points (all call 474196 `z(guildId, userId, baseChannelId, {modViewPanel, sourceLocation})`)
| Surface | Label / id | Module |
|---|---|---|
| User context menu, moderation group | "Open in Mod View", id `mod-view`. Shown only with Gate-7. For a thread, the sidebar opens on the parent channel and closes the thread sidebar first | 121127 (chunk 7667940e…) **observed** |
| User profile popout toolbar | icon button, tooltip "Open in Mod View", action `PRESS_MOD_VIEW`. Hidden for self and without Gate-7 | 395377 (chunk 094662da…) **observed** |
| Members page row | action icon, tooltip "Open Mod View". Without Gate-7 the same slot reads "View Profile" and opens the profile modal | 920360 **observed** |
| Moderator-report channel header | "Open Moderator View" ("Beo/7v"), targets `reported_user_id` | 903137 **observed** |
| Inside Mod View | the "Invited by" user chip opens Mod View for the inviter | 956972 **observed** |

- With no current channel in the guild and a base channel that isn't the Members page, `z` first navigates to the guild's default channel. It sends analytics `guild_mod_view_opened` and opens the sidebar with `modViewPanel` defaulting to `INFO`. **observed**
- Panels (486974): `INFO=1, MESSAGE_HISTORY=2, PERMISSIONS=3, AUDIT_LOG=4`. **observed**

### On open (956972 `nD`)
- Loads three things in parallel: `requestMembersById(g,[u])` (op 8), `POST /guilds/{g}/members/supplemental {user_ids:[u]}` (join source), and the user profile fetch (917445; **inferred** to be `GET /users/{u}/profile?guild_id=`). If the user is still unknown afterwards, the panel closes. While loading it shows a spinner. **observed**
- Rendered inside the user's profile theme (`displayProfile`, `forceShowPremium:true`). **observed**

### Header + action row (`nN`, `nO`)
- Avatar 48 px with status, display name, server tag badge, `username` tag, and an X close button. **observed**
- Five icon buttons with tooltips. Each is disabled (aria-disabled, tabIndex −1) when its gate fails. **observed**
  | Label | Gate | Action |
  |---|---|---|
  | Message | disabled for self | open/create DM |
  | Kick (member only) | `canManageUser(KICK_MEMBERS)`, not a non-user bot, not provisional | Kick modal (547166) |
  | Ban | `canManageUser(BAN_MEMBERS)`, not a bot / non-user bot / provisional | Ban modal (333179) |
  | Timeout / **Remove** (member only) | 838111: not owner, target not ADMINISTRATOR, `canManageUser(MODERATE_MEMBERS)` | timed out → remove timeout; else the timeout modal |
  | Copy ID | clipboard available | copy user id |

### INFO panel: sections in order (`tF`)
Title bar: "Mod View" + Refresh (spins for at least 1 s, refetches everything) + `esc` keycap.
1. **Signals** ("7V3759"). Shown only if `unusualDMActivityUntil != null` or the member is timed out. **observed**
   - "Unusual DM Activity": value is a relative time ("x days ago" style, 157347 `JOINED_AT` formatter) of `unusual_dm_activity_until`. There is no 2-day cutoff here, unlike the Members page.
   - "Timed Out": live countdown to `communication_disabled_until`.
2. **Server Activity** ("vCn2Tn"): Messages / Links / Media counts and the Audit Log entry count. Each row has a chevron and navigates when its count > 0 (−1 = spinner). The Audit Log row shows "This is Awkward" in red on error. **observed**
3. **Mod Permissions** ("ZCq2nC") + "All ({count})" → PERMISSIONS panel. Pills list the mod-relevant permissions the user holds, in guild permission-spec order. Elevated ones get a shield icon. None held → "No Mod Permissions" (green). **observed**
   - Mod set = community-moderator perms (124759 `dR`) ∪ elevated ∪ {ADMIN, KICK, BAN, MANAGE_GUILD/CHANNELS/ROLES/MESSAGES/THREADS, MANAGE/CREATE_GUILD_EXPRESSIONS, MANAGE/CREATE_EVENTS, MODERATE_MEMBERS, MENTION_EVERYONE, MANAGE_WEBHOOKS}. **observed**
   - Elevated (818348 `nf`) = KICK, BAN, ADMINISTRATOR, MANAGE_CHANNELS, MANAGE_GUILD, MANAGE_ROLES, MANAGE_MESSAGES, MANAGE_THREADS, MODERATE_MEMBERS. This is the same list that gets stripped when the guild requires 2FA. **observed**
4. **Roles** ("LPJmL/"): highest role first (tooltip "Highest Role"), then the other roles in guild order, then a "+" button "Add Role" (MANAGE_ROLES only) that opens the role-toggle context menu (841549 `Mg`). No roles and no MANAGE_ROLES → "No Roles". **observed**
5. **Account** ("ldCE/p"): **observed**
   - "Passed Verification Level" ✓/✗: user verified, phone-verified, staff, has `joinedAt`, or member flag BYPASSES_VERIFICATION. Shown for members only.
   - "Agreed to Rules" ✓/✗: only when the guild has a rules gate. True if BYPASSES_VERIFICATION, COMPLETED_ONBOARDING, or `pending === false`.
   - "Discord Join Date": snowflake time, `{month:"short",day:"numeric",year:"numeric"}`.
   - "Server Join Date": `joinedAt`, same format.
   - "Join Method": see the join-source table in §3. Footer "Invited by @user" / "Added by Bot" / "Joined through linked channel #x". When streamer mode hides invites: "Hidden", tooltip "Streamer mode is active, invite codes are hidden". No data: "Unknown", tooltip "Join method not available".
- There is **no nickname-reset item** in Mod View. Nickname changes go through the user context menu. **observed** (the bounded search covered module 956972)

### Sub-panels
- **MESSAGE_HISTORY**: Back + `esc`. Runs `GET /guilds/{g}/messages/search` with `{author_id:[u], include_nsfw:true}`, plus `has:["link"]` for Links or `attachment_extension:[png,jpg,webp,mov,mp4,qt,gif,mp3,wav,flac,ogg,opus]` for Media. Pages are 25 (`offset:25*page`). Uses the standard search-results renderer, with blocked/ignored counts. Scroll offset is saved in the sidebar state (throttled to 300 ms). Dispatches `MOD_VIEW_SEARCH_MESSAGES_SUCCESS`. **observed**
- **PERMISSIONS** "All Granted Permissions": search box "Search permission names, category, or description...", debounced 300 ms, case-insensitive substring match on title, description, category, and the label "Elevated"/"Moderator". Each permission card shows title, category, an "Elevated" tag (tooltip "Elevated permissions grant higher levels of access to server settings, and should only be granted to highly trusted mods.") or "Moderator", "{n} Roles", the description, and the granting roles. Clicking a role (MANAGE_ROLES) opens Server Settings › Roles at that role. The owner shows as "Server Owner". If the user has left: "User is no longer a member of this server." **observed**
- **AUDIT_LOG** "Moderator Action Log": `GET /guilds/{g}/audit-logs?limit=50&target_id={u}`. Entries are merged when they share action, target and actor within 30 min (the cap is 50 merges; deletes, pins, moves and prune never merge). Each row shows the title, date (`numeric` format), "@actor" (tooltip "Member who initiated this action."), and the reason or change details. **observed**

### Keyboard / motion
- `esc` is bound globally while the panel is mounted: on INFO it **closes** Mod View, on any other panel it goes **back to INFO**. **observed**
  `action:()=>o===F.g.INFO?l():g(F.g.INFO)`
- Panel change: horizontal slide (forwards when leaving INFO, backwards when returning), or a cross-fade under reduced motion. **observed**

---

## 3. Members page safety flags ("Members" channel-list page / Server Settings › Members)

### Access
- Page gate `canAccessMemberSafetyPage` = Gate-7. Guilds with COMMUNITY, MEMBER_VERIFICATION_MANUAL_APPROVAL or ENABLED_MODERATION_EXPERIENCE_FOR_NON_COMMUNITY open the channel-list route `MEMBER_SAFETY`. Other guilds open Server Settings › Members. **observed**
- Bulk actions (134413 `lN`): bulk ban needs MANAGE_GUILD + BAN. Bulk role, kick and timeout need Gate-7 plus an experiment, plus MANAGE_ROLES, KICK_MEMBERS or MODERATE_MEMBERS respectively. **observed**

### Toolbar (920360 / chunk a667d8c0…)
- Title is "Recent Members" when the search state is default, otherwise "Search Results". Search box "Search by username or id", debounced 300 ms, trimmed. Then the "Sort" button, "Select by List" (experiment + bulk access), and "Prune" (critical-secondary). **observed**
- Query parsing (859126): split on `,`, trim each token, snowflake-shaped tokens → `user_id.or_query`, others → `usernames.or_query`. Ignored unless `query.length > 1`. **observed**

### Table columns
Checkbox (bulk), **Name**, **Member Since**, **Joined Discord**, **Join Method**, **Roles**, **Signals**, actions. **observed**
- The Member Since, Joined Discord and Join Method filter menus need MANAGE_GUILD. Without it, Member Since and Joined Discord are plain headers and the Join Method column is hidden. **observed**
- Date filter presets: All, Past 1 hr, Past 24 hrs, Past 7 days, Past 2 weeks, Past 4 weeks, Past 3 months, Custom Range (After Date / Before Date, min 2015-05-15). **observed**
- Dates show relative values ("<1 day ago", "3 months ago"). Holding **Shift + Cmd/Ctrl** switches every row to long dates (`MMM d, yyyy, h:mm`). **observed**
- Roles cell: highest role pill, then "+N" (tooltip "View All Roles"), then "+" (tooltip "Add Roles", MANAGE_ROLES). **observed**
- Row click opens the profile modal. Right-click or "More Options" opens the moderation context menu (841549 `B8`, with transfer ownership). The action icon opens Mod View. **observed**
- Above the rows, a "{count} new members since {h:mm A}" banner with a "View New Members" button resets the search. **observed**
- Page sizes `[12,25,50,100]` (default 12). Footer: "Showing" + select + "members of **{count}**", or "Showing {count} members" when there is only one page size. Pager shows 7 visible pages. **observed**

### Safety flags (Signals column `e7` + filter menu `eu`, header tooltip "Filter based on safety info, such as timed out or engaging in unusual DM activity.")
| Filter label (menu id) | Row icon + tooltip | Source / threshold | Wire filter |
|---|---|---|---|
| Unusual DM Activity (`toggle-require-unusual-dm-activity`) | ChatWarning, muted: "Sent excessive DMs to non-friend server members in last 24 hrs" | member `unusual_dm_activity_until` ≥ now − **2 days** (70738 `tX = 2*DAY`; the client compares against the module-load time) | `unusual_dm_activity_until:{range:{gte:now-2d}}` |
| Timed Out (`toggle-require-communication-disabled`) | ClockWarning, red: "Timed out for" + countdown | `communication_disabled_until` > now | `communication_disabled_until:{range:{gte:now}}` |
| Unusual Account Activity (`toggle-require-unusual-account-activity`) | custom person-warning glyph, red: "Engaged in suspected spam activity" | **user** public flag `SPAMMER` (1<<20) (694318 `cx`) | `unusual_account_activity:true` |
| Quarantined (`toggle-require-username-quarantined`) | ChatX, muted: "User can't talk in server until they change their member name" | member flags `AUTOMOD_QUARANTINED_USERNAME_OR_GUILD_NICKNAME` 128, `_BIO` 256, `_SERVER_TAG` 1024 (229527 `TR`) | `automod_quarantined_username:true` |
- There is **no "new account" flag**. Account age is only the "Joined Discord" column and its filter. **observed** (bounded to 920360 and 811977)
- Each icon is shown independently, in the order of the table above. **observed**

### Sort (779662, menu `members-table-sort-menu`)
"Member Since (Newest first)" = `ORDER_BY_GUILD_JOINED_AT_DESC` 1 (default, checked when unset) · "Member Since (Oldest first)" 2 · "Joined Discord (Newest first)" `ORDER_BY_USER_ID_DESC` 3 · "Joined Discord (Oldest first)" 4. **observed**

### Join method labels (11541 `CI` / 589935)
`join_source_type`: 0 Unknown · 1 "Bot Invite" (robot) · 2 "Integration" (or "Twitch"/"YouTube" via `integration_type`) · 3 "Server Discovery" · 4 "Student Hub" · 5 invite code (link icon) · 6 "{vanity} (Vanity)" / "Vanity URL" · 7 "Manual Verification ({code})" · 8 "Linked Channel" (game icon). Clicking a value filters the table by that code or type. **observed**

---

## Endpoints & wire

| Use | Method + path | Body / query | Response used | Tag |
|---|---|---|---|---|
| Role popout preload | `GET /guilds/{g}/roles/{r}/member-ids` | none | `string[]` user ids → op 8 `{user_ids, presences:false}` | observed (op 8 inferred) |
| Role counts | `GET /guilds/{g}/roles/member-counts` | none | `{[roleId]: number}`; cached 120 s | observed |
| Members search | `POST /guilds/{g}/members-search` | `{or_query:{safety_signals:{…}}, and_query:{usernames, user_id:{or_query, range}, role_ids:{and_query}, guild_joined_at:{range}, source_invite_code, join_source_type}, sort, limit, after?/before?:{guild_joined_at, user_id}}` | `{guild_id, members:[{member, source_invite_code, join_source_type, join_source_application_id, join_source_channel_id, inviter_id}], page_result_count, total_result_count}`. **202** `{retry_after}` (s) → "Before searching, we need to index this server. Give us a bit.", up to 3 retries | observed |
| Search chunk size | (in body) | `limit = max(5*pageSize, 250)` | | observed |
| Join source | `POST /guilds/{g}/members/supplemental` | `{user_ids:[…]}` | `[{user_id, source_invite_code, join_source_type, inviter_id, integration_type}]` | observed |
| Mod View counts | `POST /guilds/{g}/messages/search/tabs` | `{tabs:{messages:{author_id,limit:1,cursor:null}, links:{…,has:["link"]}, media:{…,attachment_extension:[…]}}, track_exact_total_hits:true, include_nsfw:true}` | `tabs.{messages,links,media}.total_results` | observed |
| Mod View history | `GET /guilds/{g}/messages/search` | `author_id`, `include_nsfw=true`, `has`/`attachment_extension`, `offset` (25 per page) | standard search body | observed |
| Mod View audit | `GET /guilds/{g}/audit-logs` | `limit=50&target_id={u}` | standard audit-log body | observed |
| Member by id | gateway op 8 | `{guild_id, user_ids, presences:false}` | `GUILD_MEMBERS_CHUNK` | inferred |

Member wire fields used: `communication_disabled_until`, `unusual_dm_activity_until`, `flags`, `joined_at`, `pending`, `roles`, `nick`. **observed**

## Open questions
- Does `member-ids` really cap at 100, and which permission does the server require? Neither appears in the client. Check with a live request against a role that has more than 100 members.
- `unusual_account_activity` is sent as `true`. The client only ever sends `true`, so whether the server accepts `false` is unknown.
- The DM-activity tooltip says "last 24 hrs", but the client threshold is 2 days. Which one the server uses to set `unusual_dm_activity_until` is unknown.
- Kick, Ban and Timeout modal contents (547166, 333179, 991982) were not read. Their endpoints (DELETE member, PUT ban, PATCH `communication_disabled_until`) are **inferred**.
- No REA Evidence IDs: this session's REA server exposed only `open_binary`, so the JS graph tools were unavailable, and the bundle was read with text tools instead.
