# Discord post-join gates: reverse-engineering notes

Source: public web client, fetched 2026-11-22 from `https://discord.com/app` (no auth). Main bundle `web.355a76e660bee7a5.js` (12.8 MB); 6,600 lazy chunks resolved through the webpack chunk map (`p.u`) and fetched as `https://discord.com/assets/<hash>.js`. Chunk names below are those asset filenames. User-facing strings come from the en-US message chunks (`JSON.parse('{"<key>":[...]}')`) with each `intl.t.<key>` resolved to its text. Static reading only: nothing was executed.

Tags: **[observed]** means seen in code (file + excerpt). **[inferred]** means a reading of observed code or server behaviour the client only implies.

---

## 0. Shared enums and constants

- **Guild member `flags`** [observed] `web.355a76e660bee7a5.js` module 340837:
  `DID_REJOIN=1, COMPLETED_ONBOARDING=2, BYPASSES_VERIFICATION=4, STARTED_ONBOARDING=8, IS_GUEST=16, STARTED_HOME_ACTIONS=32, COMPLETED_HOME_ACTIONS=64, AUTOMOD_QUARANTINED_USERNAME_OR_GUILD_NICKNAME=128, AUTOMOD_QUARANTINED_BIO=256, DM_SETTINGS_UPSELL_ACKNOWLEDGED=512, AUTOMOD_QUARANTINED_SERVER_TAG=1024`
- **user_guild_settings.flags (guild level)** [observed] web: `n3:{OPT_IN_CHANNELS_ON:16384,OPT_IN_CHANNELS_OFF:8192,UNREADS_ONLY_MENTIONS:4096,UNREADS_ALL_MESSAGES:2048}`
- **channel_overrides[].flags** [observed] web: `vv:{NEW_FORUM_THREADS_ON:16384,NEW_FORUM_THREADS_OFF:8192,OPT_IN_ENABLED:4096,FAVORITED:2048,UNREADS_ALL_MESSAGES:1024,...}`
- **Channel flag** [observed] web module 746080: `IS_GUILD_RESOURCE_CHANNEL:128` (also `PINNED:2`, `REQUIRE_TAG:16`, ...).
- **Static (fake) channel ids** [observed] web module 746080:
  `GUILD_HOME="@home"`, `CHANNEL_BROWSER="channel-browser"`, `GUILD_ONBOARDING="onboarding"`, `CUSTOMIZE_COMMUNITY="customize-community"`, `MEMBER_APPLICATIONS="member-applications"`, `MEMBER_SAFETY="member-safety"`. There is no `guild-home` id; the Home id is `@home`.
  Deep-link section names: `{GUILD_HOME:"home", SERVER_GUIDE:"guide", CHANNEL_BROWSER:"browse", CUSTOMIZE_COMMUNITY:"customize", LINKED_ROLES:"linked-roles"}`.
- **Guild features involved** [observed] web: `COMMUNITY`, `MEMBER_VERIFICATION_GATE_ENABLED`, `MEMBER_VERIFICATION_MANUAL_APPROVAL`, `MEMBER_VERIFICATION_ROLLOUT_TEST`, `GUILD_ONBOARDING`, `GUILD_ONBOARDING_HAS_PROMPTS`, `GUILD_ONBOARDING_EVER_ENABLED`, `GUILD_ONBOARDING_UPSELL`, `GUILD_SERVER_GUIDE`, `WELCOME_SCREEN_ENABLED`, `CLAN`.
- **Join endpoint** [observed] web: `GUILD_JOIN:e=>`/guilds/${e}/members/@me`` is called with `PUT` and body `{}`. Query: `lurker`, `session_id` (lurker only), `recommendation_load_id`, `location`, `from_directory`. Response fields the client reads: `join_request`, `show_verification_form`, `welcome_screen`, `approximate_presence_count`, `id`.

---

## 1. Rules Screening / Membership verification gate

### Endpoints [observed] web, route table
```
GUILD_MEMBER_VERIFICATION:e=>`/guilds/${e}/member-verification`,
GUILD_MEMBER_REQUEST_TO_JOIN:e=>`/guilds/${e}/requests/@me`,
USER_JOIN_REQUEST_GUILDS:"/users/@me/join-request-guilds",
```
- **Fetch form** [observed] web module 408213 `fetchVerificationForm`: `GET /guilds/{id}/member-verification?with_guild=<!isMember>&invite_code=<code?>`. The client keeps `version`, `description`, `form_fields`, `guild`, and `profile`. On failure it dispatches `MEMBER_VERIFICATION_FORM_FETCH_FAIL`, and the store falls back to `{version:"",description:"",formFields:[]}`.
- **Form also arrives in the invite accept response** [observed] chunk `22afaa448b05653d.js` (MemberVerificationFormStore): `INVITE_ACCEPT_SUCCESS` reads `invite.member_verification_form` (`version`, `description`, `form_fields`) and `invite.join_request`.
- **Submit** [observed] web 408213 `submitVerificationForm`:
  ```
  i.Bo.put({url:I.Rsh.GUILD_MEMBER_REQUEST_TO_JOIN(e),body:{version:t.version,form_fields:t.formFields},...})
  ```
  The body is `{version, form_fields}`, where `form_fields` is the fetched array with a `response` key added to each field. The response is a join request. The client dispatches `USER_GUILD_JOIN_REQUEST_UPDATE`. If any non-TERMS field exists, it shows a coachmark after 200 ms (`USER_GUILD_JOIN_REQUEST_COACHMARK_SHOW`).
- **Submit errors** [observed] web 408213:
  - 429: modal titled "WOAH THERE. WAY TOO SPICY", body "You've been sending too many Guild applications.", button "Enter the chill zone".
  - 403: "You don't have permission to join this server".
  - Any other error: the API error message, else "Oops, something went wrong...".
  - Modal-level error when `errors.version` or `errors.form_fields` is present: "This server's requirements have changed. Please close this and try again." (chunk `8192e61e17dc8592.js`).
- **Other join-request endpoints** [observed] web 624458:
  - `DELETE /guilds/{id}/requests/@me` withdraws an application.
  - `POST /guilds/{id}/requests/@me` resets the request.
  - `POST /guilds/{id}/requests/{reqId}/ack` acknowledges an approved request.
  - `GET /users/@me/join-request-guilds`.
  - Admin side: `GET /guilds/{id}/requests?status=&limit=25&before=&after=` and `PATCH /guilds/{id}/requests/{reqId}` with body `{action, rejection_reason}`.
- **Admin form edit** [observed] web 408213: `PATCH /guilds/{id}/member-verification` with body `{form_fields, enabled, bulk_action}`, `{description}`, or `{enabled}`.

### Form field shape [observed] web module 513461 and chunk `02017b99037ec61c.js`
- `field_type` enum (strings): `TERMS`, `TEXT_INPUT`, `PARAGRAPH`, `MULTIPLE_CHOICE`, `VERIFICATION`.
- Keys used: `field_type`, `label`, `required`, `values` (TERMS: array of rule strings), `choices` (MULTIPLE_CHOICE: array of strings), `response`.
- Response types [observed] web 709977 `Ge`:
  ```
  case r.rX.TERMS:case r.rX.VERIFICATION:return!!n;case r.rX.TEXT_INPUT:case r.rX.PARAGRAPH:return"string"==typeof n&&""!==n.trim();
  case r.rX.MULTIPLE_CHOICE:return"number"==typeof n;
  ```
  TERMS and VERIFICATION take `true`. TEXT_INPUT and PARAGRAPH take a non-blank string. MULTIPLE_CHOICE takes the index into `choices`. A non-required field always passes.
- Rule rendering [observed] `02017b99037ec61c.js`: each rule is drawn as `"${i+1}."` plus markdown via `parseGuildVerificationFormRule(rule, true, {channelId: rulesChannelId})`.
- "Apply to Join" means any field other than TERMS (`j5`: `e.some(e=>!d(e))`). Fields that count as application questions: `MULTIPLE_CHOICE`, `TEXT_INPUT`, `PARAGRAPH`.

### Join request shape and statuses [observed] web 212455 and 513461
- Fields: `join_request_id, guild_id, user_id, user, created_at, form_responses, rejection_reason, application_status, actioned_at, actioned_by_user, last_seen, interview_channel_id`.
- `application_status`: `STARTED`, `SUBMITTED`, `REJECTED`, `APPROVED`.
- `READY` (`CONNECTION_OPEN`) carries `guildJoinRequests` (read as `guild_id` per item).
- Gateway events [observed] web dispatcher: `GUILD_JOIN_REQUEST_CREATE`, `GUILD_JOIN_REQUEST_UPDATE` (payload `{request, status, guild_id}`), `GUILD_JOIN_REQUEST_DELETE`.

### Pending flag and composer bar
- `member.pending` maps to `isPending` [observed] web, GUILD_MEMBER_* gateway handler:
  ```
  eH(["GUILD_BAN_ADD",...,"GUILD_MEMBER_ADD","GUILD_MEMBER_UPDATE",...],(e,t)=>{eY({...roles:e.roles,...isPending:e.pending,joinedAt:e.joined_at,...flags:e.flags
  ```
  The same mapping applies in `GUILD_CREATE` (`isPending:n.pending`) and in member chunks.
- Messages are disabled while pending [observed] web: `messagesDisabled:n||!t&&!s||o` where `n = member.isPending`.
- Composer blocker [observed] chunk `ceb73656db6e6af4.js` module 678655:
  - The modal path is used when `showMemberVerificationModal = member.isPending && guild.features.has(MEMBER_VERIFICATION_GATE_ENABLED)`. The bar text depends on the join request's `applicationStatus`:
    - `SUBMITTED`: "Your application to join the server is awaiting review." Subtitle: "You'll get notified when there's an update." Button: "Withdraw Application". Confirm dialog: "Are you sure?" / "If you want to join in the future, you’ll have to apply again." / "Confirm".
    - `REJECTED`: "Your application to join the server has been rejected." Button: "Learn more".
    - Default (no request or `STARTED`): **"You must complete a few more steps before you can talk."**, button **"Complete"** (variant `primary`), which calls `openMemberVerificationModal(guildId)`.
  - The full-page variant string "You must complete a few more steps before you can start talking in this server." (key `G5YKXP`) is referenced in web only.
  - If the account is unclaimed, the bar shows "You must claim and verify your account before you can send messages in this server." with button "Claim Account". This button also opens the verification modal when the gate applies.
  - On `APPROVED` with no `last_seen`, a success modal appears: title "Welcome to {guildName}!", subtitle "You're all set. Jump in and say hi.". Closing it POSTs the ack.
  - Lurker popout after joining: "You're now a member of {guild}!" / "Hooray! It's time to talk and hang out with your fellow members." / "Let's go!".
  - There is no "Accept rules" button text in the composer. "Accept rules" exists only in admin copy [observed i18n].

### Verification modal [observed] chunk `8192e61e17dc8592.js` module 93895 (modal key `in-app-member-verification`)
- Steps enum: `CLAIM_ACCOUNT=0, EMAIL_CONFIRMATION=1, VERIFICATION_FORM=2`. The modal starts at step 2 unless the account is unclaimed.
- Header text:
  - With application questions: "Answer a few questions to join {guildName}" and "The server admin will get back to you shortly after you submit."
  - Rules only: **"Before you can talk here..."** and **"You’ll need to complete the required steps below."**
- Verification-level rows, shown above the fields when the account does not meet them:
  - Guild `verification_level` VERY_HIGH: "Verify your phone number." / "Phone Verified".
  - Guild `verification_level` LOW, MEDIUM, or HIGH: "Verify your email." / "Email Verified".
  - Both rows: buttons "Verify" / "Resend Email" / "Email sent!", footnote "Your email and phone number will not be shared with the community staff.".
  - These rows are skipped when the form has application questions (`disableVerification = hasManualFormFields`).
- TERMS section [observed] `02017b99037ec61c.js`: title **"Agree to rules"**, numbered rules, and a checkbox labelled **"I have read and agree to the rules"**. The read-only review label is "Agreed to Rules".
- Submit button: **"Submit"** (variant `active`). It is disabled while any required field is invalid, or while the verification level is unmet (rules-only forms; staff bypass this check).
  - Tooltip when disabled and the form has application questions: "All required fields must be filled, including agreeing to the rules." or "All required fields must be filled.".
  - Application footers: "Make sure you have not entered your password or any other sensitive information." and "By applying, you give the server admins permission to message you regarding your application.".
- If the member is no longer `isPending` when Submit is pressed, the modal just closes.
- Closing with unsaved non-TERMS answers shows "Are you sure? Your progress will be lost." with buttons "Close Anyway" / "Cancel".
- Claim-account step: "Claim your account to talk in this server." (or "...to apply to this server."), "Email", "Password", "Next", "We will send you a verification email once you press Next. You must verify your email to continue.".

### Flow and gating
- [observed] web `joinGuild`: if the response has `show_verification_form` and the user has not joined yet (`joinedAt==null`), the client navigates to route `/member-verification/{guildId}` (`GUILD_MEMBER_VERIFICATION` route) and stops. Otherwise it applies `welcome_screen` and runs the onboarding entry (section 2).
- [observed] web 357566: `q:function(e){(0,i.pX)(r.BVt.GUILD_MEMBER_VERIFICATION(e))}`.
- Auto-approve vs apply-to-join [observed] web 961973 `L1` and the invite-card helper:
  ```
  n.has(MEMBER_VERIFICATION_MANUAL_APPROVAL)&&n.has(MEMBER_VERIFICATION_GATE_ENABLED)&&(i="APPLY_TO_JOIN")
  ```
  - With the gate alone, the TERMS-only submission clears `pending` [inferred: server sets `pending=false` and pushes GUILD_MEMBER_UPDATE].
  - With `MANUAL_APPROVAL`, the request goes to `SUBMITTED` and the member stays pending until an admin approves (`GUILD_JOIN_REQUEST_UPDATE` → `APPROVED`) [inferred: server then flips `pending`].
- CLAN / application interviews (noted, not explored): `POST /join-requests/{id}/interview` opens a DM-like channel. Channel flag `IS_JOIN_REQUEST_INTERVIEW_CHANNEL:65536`.

---

## 2. Onboarding

### Endpoints [observed] web route table
```
GUILD_ONBOARDING:e=>`/guilds/${e}/onboarding`,
GUILD_ONBOARDING_PROMPT:(e,t)=>`/guilds/${e}/onboarding-prompts/${t}`,
GUILD_ONBOARDING_RESPONSES:e=>`/guilds/${e}/onboarding-responses`,
```
- **GET /guilds/{id}/onboarding** response, as parsed by web 539916 `Uh`:
  ```
  prompts:e.prompts.map(I),defaultChannelIds:e.default_channel_ids,responses:e.responses??[],mode:e.mode,enabled:e.enabled,
  onboardingPromptsSeen:e.onboarding_prompts_seen??{},onboardingResponsesSeen:e.onboarding_responses_seen??{},belowRequirements:e.below_requirements,connections:e.connections??[],additionalConnections:e.additional_connections??[]
  ```
  - Prompt fields: `id, title, options[], single_select, disabled, required, in_onboarding, type`.
  - Option fields: `id, channel_ids, role_ids, emoji{id,name,animated}, title, description`. The write side also sends `emoji_id, emoji_name, emoji_animated`.
  - Connection fields: `connection_type, application_id, provider_id, description` (description must be 100 characters or less).
- Enums [observed] web 539916:
  - Prompt `type`: `MULTIPLE_CHOICE=0`, `DROPDOWN=1`.
  - `mode`: `ONBOARDING_DEFAULT=0`, `ONBOARDING_ADVANCED=1`.
  - Connection type: `APPLICATION=0`, `PROVIDER_CONNECTED_ACCOUNT=1`.
  - Channels & Roles tab: `CUSTOMIZE=0`, `BROWSE=1`.
- Client filtering [observed] web 591552:
  - `onboardingPrompts = prompts.filter(p=>p.inOnboarding)` drives the full-screen flow.
  - `defaultChannelIds` keeps only channels the user can see.
  - Each prompt gets `isNew` when its id is missing from `onboarding_prompts_seen`. Each option gets `isUnseen` when its id is missing from `onboarding_responses_seen`.
  - The re-fetch TTL is 1 hour by default (8 hours on channel select).
- **Write answers** [observed] web 669953 `L()`. Both methods send the same body:
  ```
  body:{onboarding_responses:r.map(e=>e.id),onboarding_prompts_seen:a,onboarding_responses_seen:o}
  ```
  - `onboarding_responses` is the selected option ids across the given prompts.
  - The `*_seen` fields are maps of `{promptId|optionId: Date.now()}` for every prompt and option shown.
  - **POST** is used when completing the full-screen flow (`completeOnboarding`, prompts = `in_onboarding` ones).
  - **PUT** is used from Channels & Roles. It is debounced by 1000 ms, covers all prompts, and also fires on page unmount.
  - The response returns `onboarding_responses, onboarding_prompts_seen, onboarding_responses_seen`.
  - The client never sends `update_roles_and_channels` (string absent from the bundle).
- Roles are applied server-side [inferred]. The client only updates roles locally (`GUILD_MEMBER_UPDATE_LOCAL` with `addedRoleIds/removedRoleIds`), and no role PATCH follows a selection.
- Channels from options and default channels are opted in client-side. From Channels & Roles, each selection dispatches `USER_GUILD_SETTINGS_CHANNEL_UPDATE_BULK` with `OPT_IN_ENABLED` set or cleared per channel (chunk `01149b1c4730007b.js`). Persisting goes through the 1 s debounced bulk PATCH (section 3). Single-select swaps remove the previous option's roles and channels.

### Entry, status, and flags
- On join [observed] web 197463: if the guild has `COMMUNITY` and `GUILD_ONBOARDING`, the client fetches onboarding (`h9`).
  - If every prompt has `in_onboarding=false`, the client skips the flow and records step -2.
  - Otherwise, unless the member already has `COMPLETED_ONBOARDING`, it dispatches `GUILD_ONBOARDING_START`. The status becomes `started`, then `ready` if `enabled`, else `not_applicable`.
  - It then navigates to `/channels/{guild}/onboarding` and waits until the self member's `flags` include `COMPLETED_ONBOARDING` (`addConditionalChangeListener`).
- Status enum [observed] web 713125: `started`, `ready`, `completed`, `not_applicable`. Steps: `"cover"`, `"connections"`, `0..n-1` (prompt index), `"rules"`.
- Resume after closing halfway [observed] web 280598:
  - On `POST_CONNECTION_OPEN` and `CHANNEL_SELECT`, if the self member has `STARTED_ONBOARDING (8)` and not `COMPLETED_ONBOARDING (2)`, the client re-runs the onboarding entry and sends the user back to `/onboarding`.
  - Members who joined before 2023-04-26 (`16824888e5` ms) are exempt (web 961973 `$Y`).
- Who sets the flags:
  - `STARTED_ONBOARDING` is never set by the client [observed: no setter in the bundle]. The server sets it on join [inferred].
  - `COMPLETED_ONBOARDING` is set by the server after the POST [inferred]. The client only clears it in admin "reset onboarding" (`PATCH` member `flags`).

### Full-screen flow [observed] chunk `1ae836e3d82d5964.js` (module 215764)
1. **Cover** (step `-1`) shows "Welcome to {guildName}", then "Let's customize your experience" (or "We hope you enjoy your stay!" when not applicable). It auto-advances after **3000 ms**. Footer: "Your responses will be treated in accordance with Discord's Privacy Policy".
   - On advance: if `connections.length>0` go to `"connections"`, else prompt `0`.
   - If there are no prompts and no connections, it calls `completeOnboarding` directly.
2. **Connections** (step `-3`, optional) shows "Connect your account" and "Optionally link your external account with Discord for additional features and access in this server.". The button reads **"Skip"** until a connection is linked, then **"Next"**, or **"Finish 🎉"** on the last step.
3. **Prompts** (step = index):
   - Caption: "Question {currentQuestion} of {questionCount}", plus "· Required" when the prompt is required.
   - Title: `prompt.title`.
   - Body: MULTIPLE_CHOICE shows option cards. DROPDOWN shows a multi-select dropdown (option meta "+{memberCount} people in common").
   - Help text: "Choose all that apply." and "You will receive the {count} ..." / "You will be added to {count} ...".
   - Buttons:
     - Left: **"Back"**, shown when `step>0` or there are connections.
     - Right: **"Skip"** (secondary) when nothing is selected, **"Next"** when something is selected, or **"Finish 🎉"** on the last step.
   - A required prompt with no selection disables the button, with tooltip **"This is a required question."**.
4. **Rules** (only when `L1`: the gate is on and **not** `MANUAL_APPROVAL`, and a TERMS field exists):
   - Caption **"One last step!"**, heading **"Read & Agree to Server Rules"**, numbered rules.
   - Finish is disabled until the last rule scrolls into view (IntersectionObserver). The hint reads "You must finish reading the rules to join." until then, and "By clicking "Finish", you have read and agreed to the Server Rules." after.
   - The button reads **"Finish 🎉"**. "Back" returns to the last prompt, or to connections.
   - Finish calls `submitVerificationForm(guild, {...form, formFields:[{...rulesField, response:true}]})`, which is the `PUT /requests/@me` from section 1, then `completeOnboarding`.
   - The verification form is pre-fetched when the flow mounts if the gate is on without manual approval.
5. **Complete**: `POST onboarding-responses`, then ack the onboarding-question read state. The client waits for `COMPLETED_ONBOARDING` before resolving. The onboarding page escapes to the default channel 1 s after `shouldShowOnboarding` turns false.
- The connections step reports its own analytics step id (-3), the cover -1, and the auto-skip case -2.
- The admin-side string "Members will be asked to accept Server Rules after questions in Onboarding." confirms the ordering (i18n only).

### Post-join: Channels & Roles "Customize" tab [observed] chunk `01149b1c4730007b.js`
- Page title: "Channels & Roles" (document title "Channels & Roles - {guildName}"). It falls back to "Browse Channels" when the guild lacks `GUILD_ONBOARDING_HAS_PROMPTS`.
- Tabs: **"Customize"** and **"Browse Channels"** (with a "new" badge when there are new channels). The Customize tab exists only when the guild has prompts.
- Prompts are shown in this order: new ones, then ones with unseen answers (header "{count, New Option(s)}"), then the rest (header "{count, Customization Question(s)}" and "Answer questions to get access to more channels and roles."). Non-onboarding prompts come before `in_onboarding` ones.
- A required prompt cannot be emptied: deselecting its last option marks the `*` red and does not apply the change.
- Connections section, then a "My Profile" card ("This is how you'll look to others!", "Roles") when the window is at least 1344 px wide.
- Empty state: "There are no customization options yet." / "You can add and remove channels from Browse Channels.".

---

## 3. Channels & Roles / Browse Channels and opt-in channels

### Entry rows at the top of the channel list [observed] chunk `6b05ca0762af596a.js`
Row order (`useGuildActionRows`): hub header, then **New-member progress bar** (or premium progress bar), then **Server Guide**, then guild space, then events, then **Channels & Roles / Browse Channels**, then subscriptions, shop, and the rest.
- **Channels & Roles row**: shown if the guild has `COMMUNITY` and is not a `HUB`, and (`GUILD_ONBOARDING_HAS_PROMPTS` or the user has opt-in on).
  - Label: "Channels & Roles" when the guild has prompts, else "Browse Channels". Icon: ChannelListMagnifyingGlass.
  - Click goes to `customize-community` when the guild has prompts, else `channel-browser`.
  - It shows a "new" badge for new channels (more than 2) or new onboarding questions.
- **Server Guide row**: label "Server Guide", SignPost icon, links to `@home`. See section 4 for visibility.
- **Progress bar row**: "Get Started", "{completed} of {total}", and a green bar. Visibility is in section 4.

### Browse Channels page [observed] `01149b1c4730007b.js`
- Header: "Search Channels" input, and a "Create" button when the user has MANAGE_CHANNELS.
  - NUX card: "Choose Your Channels" / "Customize what's on your channel list, so you see just the stuff you care about." / "Tip: Channels with mentions will always appear on your channel list, even if you hide them.".
  - Admin upsell instead: "Hey admin, make this page less overwhelming for members" / "Check it out".
- Each category header has a **"Follow Category"** toggle. This is the select-all for a category: it opts in the category's own override id.
- Channel row: name, active-thread count, topic, and a **"View"** / **"Join"** (voice) / **"Open"** (forum) button, plus a checkbox.
  - When the parent category is followed, child rows are muted. Tooltip: "You must unfollow this category to add or remove specific channels."
- Toggling a channel [observed] web 816662 `zk`:
  - Sets or clears `OPT_IN_ENABLED (4096)` on `channel_overrides[ch].flags`.
  - Unfollowing also clears `FAVORITED (2048)`.
  - If the guild's opt-in is off, it also sets guild `flags |= OPT_IN_CHANNELS_ON (16384)`.
- **"Show All Channels"** toggle (guild header menu, chunk `a8dfb64ebcb89184.js`) [observed]:
  - Shown only when the guild has `COMMUNITY` (or the user is staff).
  - `checked = !optInEnabled`. Toggling sets or clears guild `OPT_IN_CHANNELS_ON` (web 816662 `e4`).
  - For an un-onboarded existing member, the first toggle instead opts in every non-thread channel plus the default channels and turns opt-in on (chunk `258d517bee8bb5ee.js`, `onboardExistingMember`).
  - Related string: "Turn on **Show All Channels** in your server menu. You can reorder channels when this view is enabled.".

### Persistence endpoint [observed] web
```
USER_GUILD_SETTINGS:e=>`/users/@me/guilds/${e}/settings`,
USER_GUILD_SETTINGS_BULK:"/users/@me/guilds/settings",
```
- For a real guild, `saveUserGuildSettings(guildId, s)` and `saveUserGuildSettingsBulk` both send:
  ```
  i.Bo.patch({url:o.Rsh.USER_GUILD_SETTINGS_BULK,body:{guilds:e}})
  ```
  So the request is `PATCH /users/@me/guilds/settings` with `{"guilds":{"<guildId>":{"flags":<guildFlags?>,"channel_overrides":{"<channelId>":{"flags":<n>}}}}}`.
- The per-guild `PATCH /users/@me/guilds/{id}/settings` is only used for the `@me` (DM) pseudo-guild.
- Onboarding option selection pushes the pending overrides through the same bulk call, debounced by 1000 ms.
- Gateway: the client listens for `USER_GUILD_SETTINGS_*` dispatch types. The server echo is `USER_GUILD_SETTINGS_UPDATE` [inferred, not traced].

---

## 4. Server Guide (Home)

### Endpoints [observed] web route table
```
GUILD_HOME_SETTINGS:e=>`/guilds/${e}/new-member-welcome`,
GUILD_MEMBER_ACTIONS:e=>`/guilds/${e}/new-member-actions`,
GUILD_MEMBER_ACTION_UPDATE:(e,t)=>`/guilds/${e}/new-member-action/${t}`,
```
Also present: `RESOURCE_CHANNEL: /guilds/{id}/resource-channels/{ch}`, `NEW_MEMBER_ACTION: /guilds/{id}/new-member-actions/{ch}` (admin), `GUILD_HOME_HEADER: /guilds/{id}/home-headers/{hash}.jpg`, and icon CDN paths `new-member-actions` / `resource-channels`. There is no `/guilds/{id}/guide` endpoint.
- **GET /guilds/{id}/new-member-welcome** is parsed by web 374084 `Xu`:
  ```
  let{welcome_message:t,new_member_actions:n,resource_channels:i,enabled:r}=e,l={authorIds:t.author_ids,message:t.message};
  ```
  - `new_member_actions[]`: `channel_id, action_type, title, description, emoji{id,name,animated}, icon`.
  - `resource_channels[]`: `channel_id, title, description, emoji, icon`.
  - Entries whose channel is unknown are dropped.
- `action_type` [observed]: `VIEW=0`, `CHAT=1`. Content kinds `DEFAULT=0, TODO=1, RESOURCE=2, UPDATES=3` exist in the same module but are not used for members.
- **GET /guilds/{id}/new-member-actions** (member progress) [observed] web 374084 `xr`:
  ```
  for(let n in e.channel_actions)t[n]=e.channel_actions[n].completed;
  ```
  The response is `{channel_actions: {"<channelId>": {completed: bool}}}`. It is only fetched when the member has `STARTED_HOME_ACTIONS (32)`.
- **Mark done** [observed] web 225142 `eC`: the client dispatches `COMPLETE_NEW_MEMBER_ACTION` optimistically, then sends `POST /guilds/{id}/new-member-action/{channelId}` (no body).
- Trigger rules [observed] web 976639:
  - `VIEW` completes on `CHANNEL_SELECT` of that channel.
  - `CHAT` completes on a `MESSAGE_CREATE` authored by self in that channel (or in a forum post whose parent is that channel), or on a self-created thread in that forum.
  - Only while the member is "new": joined less than 7 days ago and without `COMPLETED_HOME_ACTIONS (64)` (web 978165).
- Completion [observed] web 976639: when `GUILD_MEMBER_UPDATE` brings `COMPLETED_HOME_ACTIONS`, the client opens a celebration modal: "You've explored {count} starter channels, great job!". The server sets bit 64 [inferred].

### Visibility [observed] web 707167 and 473529
- Server Guide needs `COMMUNITY` + `GUILD_SERVER_GUIDE` + `GUILD_ONBOARDING`, not overlay or `@me`.
- The Home row also needs (new member within 7 days without COMPLETED_HOME_ACTIONS) **or** any channel with flag `IS_GUILD_RESOURCE_CHANNEL (128)`.
- The progress bar row needs `!COMPLETED_HOME_ACTIONS`, the Home row visible, a new member, and `new_member_actions.length>0`.

### UI strings [observed] chunks `da04de52aac15bcd.js`, `e40e5ec70a307706.js`, web
- Page header: "{guild name} Server Guide". Banner buttons: "Invite", plus "Edit Image" or "Unlock with Boosting" for admins.
- Welcome message card: author avatar and name plus `message`. It is only valid if `message.length >= 7` and has author ids.
- To-do list: heading **"Get Started"**. Rows show `title`, a channel name, and a check or empty circle. Aria label: "{title}, {status}" with "completed" / "not completed".
  - A final row **"Read the rules"** appears when `rulesChannelId` is set.
- In-channel action banner (web): `title` plus **"Send a message"** for CHAT. After completion it shows **"Next: {step}"** with the next incomplete action.
- Resources section: **"Resources"**. Done state for non-new members with no resources: **"You're all set!"** with button **"Explore the Server"**, which goes to the default channel.
- The page escapes to the default channel when the guide is empty.

---

## 5. Ordering when several gates apply

- [observed] `joinGuild` (web module 778858, `GUILD_JOIN` PUT):
  1. If the server returns `show_verification_form` for a not-yet-member, the **member-verification page** comes first and onboarding does not start.
  2. Otherwise the client handles `welcome_screen` and enters **onboarding** (COMMUNITY + GUILD_ONBOARDING).
- [observed] Inside onboarding, the step order is **cover → connections → prompts → rules → finish**.
  - The rules step appears only for the gate without manual approval (`L1`). It submits the same `PUT /requests/@me` and clears `pending`, so the composer "Complete" bar never shows for those members.
  - With `MANUAL_APPROVAL` (apply-to-join), there is no rules step in onboarding. The member finishes onboarding while still pending, and the composer bar shows "Complete" to open the application modal.
- [observed] Server Guide to-dos only count after onboarding. The member needs `STARTED_HOME_ACTIONS`, set by the server [inferred], and a 7-day window from `joinedAt`.
- [inferred] For guilds with the gate but no `GUILD_ONBOARDING`, members land in the guild pending. The composer shows "You must complete a few more steps before you can talk." / "Complete", which opens the modal with "Before you can talk here..." / "Agree to rules" / "I have read and agree to the rules" / "Submit".

---

## Not found / open
- Numeric admin limits (max rules, max prompts, max options per prompt, max new-member actions, title and description lengths). These look like inline numbers in settings chunks that were not traced. The only limit strings seen: "You can add up to 5 questions." (application form) and the connection description limit of 100 characters.
- The exact route component for `/member-verification/{guildId}`. It was not opened; it most likely wraps the same form [inferred].
- The server-side rules for setting `STARTED_ONBOARDING`, `COMPLETED_ONBOARDING`, `STARTED_HOME_ACTIONS`, `COMPLETED_HOME_ACTIONS`, and `pending=false`. These are not in the client.
- The gateway payload name for user-guild-settings echoes. Not traced.
- The dropdown placeholder text for DROPDOWN prompts. Not resolved.

## 6. Invite accept screen and the server's splash

- [observed] chunk `4db2f563b4494b0c.js`: the accept screen's settings row is an expander titled "Server Settings" with the subtitle "You can customize these at any time". Inside it: "Server Nickname" (shown only when `invite.is_nickname_changeable`, `maxLength:32`, placeholder "What should everyone call you?"), "Allow direct messages", and "Show your activity status". The last one only appears when the activity-status setting is on.
  `title:C.intl.string(C.t["A5wHQ/"]),subtitle:C.intl.string(C.t.UKCSEd),onOpen:m,maxHeight:d?220:130`
- [observed] web module 960736: defaults. `dmsAllowed` starts as `!restrictGuildsByDefault`, and `showActivity` starts on unless activity is set to "on for large servers" and the server has more than 200 members.
- [inferred] These are applied after joining as the member nickname and your privacy settings. The exact calls were not traced (`Not verified:`).
- [observed] chunk `1ae836e3d82d5964.js`: onboarding reads the server's invite splash, `getGuildSplashURL({id, splash})`, and passes it as `splashUrl` to a lazily loaded background. The cover step is skipped (`useState(null!=w?null:"cover")`) when that state is set, so a splash changes how the cover shows [observed shape, rendering inferred].
- [inferred] The image on the right of the accept screen is the same `splash` (CDN `/splashes/{guild_id}/{hash}.jpg`). The invite object carries `guild.splash`.
