# Discord Group DMs: reverse-engineered behaviour

Source: public web client bundles fetched unauthenticated from `discord.com/app` on 2026-11-22 (build `web.355a76e660bee7a5.js`). The desktop cache under `~/Library/Application Support/discord/` was not readable (macOS privacy block), so every observation comes from the web bundles, which the desktop client also loads. UI strings are hashed intl keys. They were resolved through the en-US message chunk `c30b23bab1166ada.js`.

Labels: **observed** means seen in a bundle (file + search term). **inferred** means reasoned from code or not seen directly.

## Limits

- Member cap is `10`, including you. It is `25` when the user is staff, or has Nitro tier 2 and the `2026-06-nitro-gdm-cap-increase` experiment is on. **observed** `web.355a76e660bee7a5.js`, search `getGroupDMRecipientLimit`:
  ```js
  return t?.isStaff()||e&&(0,r.ki)(t,s.PremiumTypes.TIER_2)&&(0,a.J)("getGroupDMRecipientLimit").enabled?25:10
  ```
- The server error for too many recipients is `TOO_MANY_RECIPIENTS = 30004`. **observed** web.js, search `TOO_MANY_RECIPIENTS=`.
- Group name maximum is 100 characters (Edit Group modal input `maxLength:100`). **observed** `bfe5aeeee32bba7a.js`, search `maxLength:100`.
- Icon upload maximum is 8 MiB (`maxFileSizeBytes:8388608`), and the upload modal title is "Change\nIcon". **observed** `bfe5aeeee32bba7a.js`, search `8388608`.
- Group invite links default to `max_age` 86400 s. The `2026-02-longer-group-dm-invites` experiment raises this to 2419200 s (28 d). **observed** web.js, search `inviteMaxAgeSeconds`.

## Recipient picker (one component, three entry points)

The class is `PrivateChannelRecipientsInvite`. Its analytics entry points are `{ADD_FRIENDS_TO_DM:"Add Friends to DM", NEW_GROUP_DM:"New Group DM", MEMBER_LIST:"Member List"}`. **observed** web.js, search `NEW_GROUP_DM:"New Group DM"`.

Entry points:
- **Friends page**: a toolbar icon with tooltip "New Group DM" (`ChatPlusIcon` when there is no channel). It is hidden when the window is 940px or narrower (`@media (max-width:940px){.inviteToolbar_zrUwGA{display:none}}`). **observed** web.js `tooltip:...HfOgA/`, plus `css/599618.8b099a3b548d3c11.css`.
- **DM or group DM header**: a `GroupPlusIcon` button that opens the same modal. **observed** web.js `function tu(`. The tooltip copy in the main header was not confirmed. The overlay header uses "Add Members". **observed** `f377a8a1dbe4c9c1.js`.
- **Group member list footer**: "Invite to Group DM" button, shown when members < limit. **observed** `9b3cd218fe829ac6.js`.
- Global hotkey event `TOGGLE_DM_CREATE` opens the new-DM flavour. **observed** web.js.

Title, from `getTitle()`:
- Not friends with a 1:1 DM peer: "Add Friends to DM"
- Group is full: "The party is full!"
- No channel: "New Message"
- Group DM: "Invite to Group DM"
- 1:1 DM: "Add to DM"

Subtitle, from `getSubtitle()`:
- No friends: "Add friends to start messaging.", or "You need to be friends with **{username}** to start a group DM."
- New: "Group DMs can have up to {number} members."
- Existing group: "You can add 1 more person." or "You can add {n} more people."
- At the limit: "This group has a {number} member limit."

Other picker copy:
- Search field: placeholder "Search" (empty when tags exist), leading tag label "To:". **observed** web.js `renderSearchBar`.
- Helper text: "Add friends or server members to group DMs." It switches to "Non-friends in mutual servers will need to accept your invite before joining." once a non-friend is selected.
- Results split into collapsible sections "Friends" and "Server Members" (header 32px, row 48px). Rows show name, username, and "{n} Mutual Server(s)" (max 3 icons).
- Empty results: "No results found." plus "Can't find them? Try sending an invite link." (group) or "Search your friends or people from your servers."
- A row is disabled when it is already a recipient, or when `getRemaining()<=0`. `getRemaining = limit - selected - (recipients+1)`.
- Nitro treatment: a counter shows "{numMembers} / {maxMemberLimit} member limit" in critical colour at the limit. Selecting past the limit shakes the banner "You've reached your limit". When full, the modal shows "You've reached the {number} member limit".

Footer actions:
- New DM: "Cancel" and either "Create Message" (1 selected) or "Create Group Message" (more than 1). With more than 1 selected, an inline "Group Name (optional)" input and an icon picker appear (placeholder is the auto name; the icon cannot be removed here).
- 1:1 DM: "Create Group Message".
- Group DM: a header button "Add" next to the search field. The footer is "Or, send an invite link" with Copy, Copied (1 s), or Create, and helper "Your invite link expires in {n} hours/days". "Hidden; Use copy button" is shown when streamer mode hides invites.
- Not friends: "Send Friend Request" or "Friend Request Sent" (disabled).

Duplicate group guard: if a group with the same recipient set exists, a "Confirm New Group" modal appears: "You already have a group with these people! Are you sure you want to create a new one?" with buttons "Cancel" and "Create Group", followed by an "Existing Groups" list. **observed** web.js `function tr(`.

## Create / add requests

| Action | Request | Notes |
|---|---|---|
| Create DM / group | `POST /users/@me/channels` body `{recipients:[ids]}` | **observed** web.js `openPrivateChannel`. Response `type===GROUP_DM (3)` means a group |
| Create group when some picks are non-friends | `POST /users/@me/channels/group-dm-shell` body `{recipient_id}` | Used when `useGroupDmShell && recipientIds.length===1`. Non-friends get an invite link DM'd instead. **observed** `createGroupDmShell` |
| Then name/icon | `PATCH /channels/{id}` `{name?, icon?}` (data URI) | **observed** `updateGDM` → `updateChannel` |
| Add to existing group | `PUT /channels/{id}/recipients/{user}` (no body), one call per user, sequential after the first | **observed** `addRecipient`. Announces "user added to the group" or "failed to add user to the group" |
| Add to a 1:1 DM | same PUT on the DM id | `201` means a new group channel was created and is opened. Otherwise the id stays. **observed** `201===t.status` |
| Invite non-friends | `POST /channels/{id}/invites` `{max_age}` then DM the code to each user | Toasts "Invite sent to {username1}…" and "Couldn't send invites, please try again." **observed** `async function eO` |

Only friends (or staff↔staff) go into `recipients`. Others become invite-link-only. **observed** `function e9(`.

## Edit group (rename / icon)

Opened from the context menu "Edit Group", the system-message link "Edit Group", or the empty-group header button "Edit Group". There is no inline header rename in this build (the "Edit Group Name" string is unused, **inferred**).

Modal copy: title "Edit Group", icon button aria "Edit Group Icon", "Remove Icon" link, name input placeholder = auto name. Buttons "Cancel" and "Save"; Save is disabled when nothing changed. **observed** `bfe5aeeee32bba7a.js`.

Request: `PATCH /channels/{id}` with only the changed keys, `{name}` and/or `{icon: dataURI|null}`. On `retry_after` a dialog shows "You are being rate limited." / "Try again in {duration}". **observed** same file (`setIcon`, `updateChannel`).

## Members, owner, leave

User context menu inside a group (`1dea26153f4ef4d4.js`):
- **Remove From Group** (danger). Shown only if you are owner, target is a recipient and not owner, and the group is not managed. Calls `DELETE /channels/{id}/recipients/{user}` with no confirm. **observed** search `removeRecipient`.
- **Make Group Owner** (danger). Shown to the owner for others. Opens a confirm modal "Transfer Group Ownership", body "Transfer ownership of this group to {user}?" (two 80px avatars with an arrow), buttons "Confirm" and "Cancel". Then `PATCH /channels/{id}` `{owner:userId}`. **observed** `setDMOwner`.

Channel context menu (`fd04ebd43cae1b56.js`, navId `gdm-context`, hidden for managed groups):
- Groups in order: mark-read, favorite or mute group items, "Invites" (owner only, opens the invites modal), "Edit Group", ..., the leave item, and "Copy Channel ID".

Leave flow:
- Menu item "Leave Group" (danger). **observed** `4415f1818db27a8f.js`.
- Confirm modal (`82c01c102e388cbd.js`): title "Leave '{name}'", body "Are you sure you want to leave **{name}**? You won't be able to rejoin this group unless you are re-invited." Checkbox "Leave without notifying other members" (default off). Buttons "Leave Group" and "Cancel".
- Managed groups use different copy: title "Leave {name} Group", body "...unless\n**{name}** adds you again."
- Request: `DELETE /channels/{id}?silent={bool}`. The client dispatches `CHANNEL_DELETE` optimistically first. Announcements: "You have left the group" or "Failed to leave the group". **observed** web.js `closePrivateChannel`.
- Slash command `/leave silent:<bool>` in group DMs shows the same modal. **observed** web.js `untranslatedName:"leave"`.
- Bulk cleanup: "Clean Up Inactive Group DMs" sends `POST /users/@me/channels/bulk-leave` `{channel_ids}`. Copy: "Leave up to 1,000 Group DMs at once without notifying anyone. Only Group DMs inactive for ~6 months are shown." **observed** `bulkLeaveGroupDMs`.

## Gateway events

- `CHANNEL_RECIPIENT_ADD` / `CHANNEL_RECIPIENT_REMOVE` carry `{channel_id, user, nick}`. The store calls `addRecipient(user.id, nick)` or `removeRecipient(user.id)`. **observed** web.js `CHANNEL_RECIPIENT_ADD:function(e)`.
- `CHANNEL_UPDATE` (batched client-side to `CHANNEL_UPDATES`) carries name, icon and owner changes. **observed** event name. Per-field use is **inferred**.
- `CHANNEL_CREATE` / `CHANNEL_DELETE` for new or left groups. **observed** dispatches in `openPrivateChannel` / `closePrivateChannel`.

## System messages (message `type`)

Enum: `RECIPIENT_ADD=1, RECIPIENT_REMOVE=2, CALL=3, CHANNEL_NAME_CHANGE=4, CHANNEL_ICON_CHANGE=5`. **observed** web.js `RECIPIENT_ADD=1`.

Renderer copy (`2b1d2576a5862cd7.js`):
- 1: "{username} added {otherUsername} to the group."
- 2: "{username} removed {otherUsername} from the group." when the mentioned user differs from the author. Otherwise "{username} left the group."
- 4: "{username} changed the group name: **{channelName}**. Edit Group". When `content===""` it reads "{username} removed the custom group name. Edit Group".
- 5: "{username} changed the group icon. Edit Group" (the link opens the Edit Group modal).

## Group invite links

- Created with `POST /channels/{id}/invites` `{max_age}`. The source tag is `GROUP_DM`. The picker auto-creates one on open when the group is not full. **observed** `createInvite`.
- Owner-only "Invites" menu item opens an invites modal (module `454963`). Its contents were not inspected.
- Message copy: "Hey there! {username} invited you to join their group DM. Invite expires in 24 hours. {link}". The invite embed reads "You've been invited to join a group DM". **observed** strings `+zWvOQ`, `3p3/BK`. Where these render is **inferred**.
