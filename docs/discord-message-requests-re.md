# Discord Message Requests + Spam: reverse-engineered behaviour

Source: public web client bundles from `discord.com/app` (2026-11-22, `web.355a76e660bee7a5.js`) and the lazy chunks listed per claim, with en-US strings from `c30b23bab1166ada.js`. The desktop cache was unreadable (macOS privacy block). Labels: **observed** = seen in a bundle (file + search term); **inferred** = not seen directly.

## Channel fields and client state

- DM channel JSON maps to the record as: `is_message_request` → `isMessageRequest`, `is_message_request_timestamp` → `isMessageRequestTimestamp`, `is_spam` → `isSpam` (default `false`). **observed** web.js, search `isMessageRequest:e.is_message_request`.
- `MessageRequestStore` (module `380335`): the set of private channels where `isMessageRequest && !isSpam`. **observed** web.js `displayName="MessageRequestStore"`.
- `SpamMessageRequestStore` (`157550`): the set where `isSpam`. **observed**.
- Both stores rebuild on `CONNECTION_OPEN(_SUPPLEMENTAL)`. They update on `CHANNEL_CREATE`, `CHANNEL_UPDATES` (the gateway `CHANNEL_UPDATE`, batched) and `CHANNEL_DELETE`, and are snapshot-cached.
- `MESSAGE_REQUEST_ACCEPT_OPTIMISTIC {channelId}` is a local dispatch after accepting. **observed**.
- No dedicated gateway event exists for requests. State flows through the channel events above. **inferred** from the store handlers (no other `MESSAGE_REQUEST*` dispatch types besides `MESSAGE_REQUEST_NOTIFICATION_SENT`).
- The normal DM list hides both sets: `uX()` filters out `isMessageRequest(id) || isSpam(id)`. **observed** module `309199`.

## DM home nav row

- Row "Message Requests": `EnvelopeIcon`, route `/message-requests` (`BVt.MESSAGE_REQUESTS`), trailing count badge = `getMessageRequestsCount()`. The count covers requests only, not spam. **observed** web.js `function pu(`, `function pc(`.
- Visible only when `requests > 0 || spam > 0`. **observed** `309199` `SM`.
- The row is one of the fixed home rows, alongside Family Center, Quests, Shop and Library. **observed** `let pa=[...MESSAGE_REQUESTS...]`.

## Message Requests page (`1b94b2e83f586720.js`, module `793706` "MessageRequestPage")

### Header

- `EnvelopeIcon`, title "Message Requests", divider, then top-pill tabs: "Requests" and "Spam". The Spam tab reads "Spam ({count})" when the spam count is above 0.
- First-visit coach mark, title "New Message Requests":
  - Body: "Messages from people you aren't friends with will show up here. Toggle this under Settings > Privacy & Safety."
  - Buttons: "Check it out" (opens the setting) and "Got it".
  - Variant without the toggle: body "Messages from people you aren't friends with will show up here." with only "Got it".

### Requests tab

- Section header: "Pending Requests — {count}", or "Pending Requests" when the count is 0.
- When the user may clear: " • Clear All" (text button).
- List settings: `rowHeight:73`, `sectionHeight:26`, padding 24, chunkSize 30. Sorted by `lastMessageId` descending.

### Spam tab

- Header: "Spam - {count}" and " • Clear All". Same list metrics.

### Empty states

- Wumpus image (415×200) with note "There are no pending message requests. Here's Wumpus for now."
- Spam variant: "There are no spam requests. Here's Wumpus for now."

### Request row (both tabs)

- 40px avatar.
- Display name with nickname and account identifier.
- Relative time of the last message (`extractTimestamp`).
- Mutual servers: up to 3 icons plus "{n} Mutual Server(s)", or "No Mutual Servers".
- On hover, the mutual line swaps to a one-line message preview. The preview shows one of:
  - "Message is from a blocked user."
  - "Message is from an ignored user."
  - "The last message was deleted."
  - "Error loading message preview."
  - "Click to preview" when restricted.
- Right-click opens the user context menu.

Row buttons:
- **Requests tab**: "Ignore" (secondary, `sm`) and "Accept DM" (primary). "Ignore" becomes "Decline" when the account is under `MESSAGE_REQUEST_RESTRICTIONS`.
- **Spam tab**: "Accept DM" (secondary) and "Report Spam" (`critical-primary`, disabled until the message loads).

### Row click and sidebar

- Clicking a row opens the DM in a right sidebar (`openPrivateChannelAsSidebar`) and focuses the composer. The sidebar has max width `pageWidth-650` and a close X.
- For spam channels the sidebar adds a banner "Was this message spam? Let us know and help improve Discord’s spam filtering." with a "Mark as Not Spam" button.
- If the user sends a message from the sidebar, the client navigates to the DM and closes the sidebar. **inferred** from the `findNewest(author===me)` effect.

### Error toasts

- Requests: "Uh oh! Something went wrong while updating this message request. Please wait a moment and try again."
- Spam: same text with "spam message".

## Actions and endpoints

REST module `966107` (**observed** `7ffd2b3eacb24530.js`). Enum `consent_status`: `UNSPECIFIED=0, PENDING=1, ACCEPTED=2, REJECTED=3` (**observed** `5f8bd9b504d3d0c3.js`, module `961523`).

| Action | Request |
|---|---|
| Accept DM | `PUT /channels/{id}/recipients/@me` body `{consent_status:2}`, then dispatch `MESSAGE_REQUEST_ACCEPT_OPTIMISTIC` |
| Ignore / Decline (one) | `DELETE /channels/{id}/recipients/@me` |
| Clear All (requests or spam tab) | `PUT /channels/recipients/@me/batch-reject` body `{channel_ids:[...]}`, chunked by 50, run sequentially |
| Mark as message request (experimental, staff) | `PUT .../recipients/@me` `{consent_status:1}` ("Mark as Message Request (Experimental)") |
| Reset (experimental) | `PUT .../recipients/@me` `{consent_status:0}` ("Reset Message Request (Experimental)") |
| Recipient flags | `PATCH /channels/{id}/recipients/@me` `{flags}` (**observed** `ceb73656db6e6af4.js` `updatePrivateChannelRecipientFlags`) |
| Row preview data | `GET /users/@me/message-requests/supplemental-data?channel_ids=...` (batches of 25). Dispatches `LOAD_MESSAGE_REQUESTS_SUPPLEMENTAL_DATA_SUCCESS/ERROR` (**observed** `ceb73656db6e6af4.js` module `978914`) |
| Report Spam | Opens the report flow for the first DM message (`FIRST_DM` report type). On submit, closes the channel: `DELETE /channels/{id}` (**observed** `1b94b2e83f586720.js` `function to(`, `d65c4148a71425e4.js` `b8:`) |

Hook `190857` (**observed** `ceb73656db6e6af4.js`) wires the UI to these calls.

**Accept** may show a confirm modal first. The modal appears only when the user's country code is `GB` and gate `840387` passes:
- Title: "Are you sure you know this person?"
- Body: "They're not on your friends list. Click 'Confirm' below if you're sure you want to speak to them in a DM."
- Buttons: "Cancel" and "Confirm".
- Before showing it, the hook fetches the user's mutual guilds if they are not loaded.
- **observed** `7385a75a3f559c5b.js` and `467840` (`new Set(["GB"])`). The exact meaning of gate `840387` is **inferred** (likely an experiment).

**Mark as Not Spam**: if the setting `privacy.nonSpamRetrainingOptIn` is unset, a modal appears:
- Title: "Confirm this isn't spam"
- Subtitle: "Are you sure this message isn't spam?"
- Checkbox: "Help improve our spam filter by forwarding this message to Discord" (default on).
- Footer checkbox: "Don't show this again" (default on).
- Buttons: "Cancel" and "Confirm".

On confirm:
1. If "don't show again" is ticked, persist the opt-in choice to `nonSpamRetrainingOptIn`.
2. If forwarding is on, submit a headless `FIRST_DM` report with variant `_first_dm_ham_v1` for the oldest message from the other user.
3. Then run Accept (`consent_status:2`).

**observed** `a6d1a2a939a2da6e.js` and `d65c4148a71425e4.js` `tJ:`. The reporting endpoint path is **inferred** (`/reporting/...` family seen in Endpoints).

On accept success, the sidebar closes. When this was the only request (hook `336590`), the client also navigates into the DM. **observed** `onAcceptSuccess` in `1b94b2e83f586720.js`. The meaning of hook `336590` is **inferred**.

## Settings (`52b6e35ba37f71f0.js`, panel "Messaging Permissions")

The panel layout is `[..., Spam Filters, Direct Message (DM) Permissions, ...]`. **observed** `MESSAGING_PERMISSIONS_PANEL`.

**Spam Filters → "Automatically filter suspected spam messages"**
- Subtitle: "Discord can filter out some messages that contain spam. These messages go to your Spam inbox."
- Options:
  - "Filter all spam" = `FRIENDS_AND_NON_FRIENDS`
  - "Filter messages from non-friends (Recommended)" = `NON_FRIENDS`
  - "Don't filter spam" = `DISABLED`
- Search terms: "DM Spam Filter", "Messages", "Inbox".
- Proto: `textAndImages.dmSpamFilterV2`, enum `DEFAULT_UNSET=0, DISABLED=1, NON_FRIENDS=2, FRIENDS_AND_NON_FRIENDS=3`.
- When unset, the effective value is: `FRIENDS_AND_NON_FRIENDS` if `nsfwAllowed===false` and the SPAM_FILTERS age gate applies; otherwise derived from `explicitContentFilter`, else `NON_FRIENDS`.
- Legacy setting `textAndImages.dmSpamFilter` (v1, default `NON_FRIENDS`) still exists.
- **observed**, search `dmSpamFilterV2`.

**Direct Message (DM) Permissions → "Message requests"**
- Subtitle: "Filter messages from server members you may not know. Learn more". Hub variant: "...hub members...".
- A guild selector scopes the toggle: "0" = default for all servers. The selector adds the line "{n} server(s) has custom DM permissions. Select it to see or change them."
- Proto fields:
  - `privacy.defaultMessageRequestRestricted` (bool; toggle ON means not restricted)
  - `privacy.messageRequestRestrictedGuildIds` (per-guild)
  - `privacy.restrictedGuildIds` (DMs off; disables the toggle)
- Changing the default prompts:
  - Header: "Message Requests"
  - Body: "Do you want to also apply this change to all your existing servers? You can also disable messages requests on individual servers by visiting that servers sett…"
  - Buttons: "No" (default only) and "Yes" (also sets `messageRequestRestrictedGuildIds` to all guilds when restricting, or `[]`).
- Teens and unconfirmed-age users cannot turn it off. Notices:
  - "You’re in the teen age group, so Message Requests can’t be turned off. Learn more"
  - "Message Requests can only be turned off if you’re in the adult age group. Learn more"
- Turning it off may first require age verification.
- **observed**, search `PERMISSIONS_MESSAGE_REQUESTS_SETTING`, `messageRequestRestrictedGuildIds`.

**Other observed copy**
- "This might be spam".
- "Message content hidden just to be safe. You can still reply.\nMessages marked as not spam will help improve our spam filters."
- "Pending (potential spam) — {count}".

These come from in-channel banners and the friends Pending list. Their exact placement was not traced (**inferred**).
