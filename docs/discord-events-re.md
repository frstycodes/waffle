# Discord guild scheduled events: reverse-engineered

Source: Discord web build `web.355a76e660bee7a5.js` (fetched 2026-11-22) plus lazy chunks fetched from `https://discord.com/assets/<hash>.js`. Lazy chunks are served by **hash only**: the `p.u` map gives `chunkId → hash`, and the URL is `/assets/<hash>.js`. The exception is ids handled by the `if(N===e)return""+e+".<hash>.js"` chain at the top of `p.u`, which use `<id>.<hash>.js`. Copy comes from the en-US message chunk `c30b23bab1166ada.js` (chunk 301911, module 177354) and from `06d126e51cd73410.js`. Message keys are 6-character hashes such as `t["0bK0B1"]`.

Tags: **[O]** means read in shipped code. **[I]** means inferred from the code without running it. Nothing here was observed at runtime.

| Module / chunk | What |
|---|---|
| 496092 (web) | REST action creators |
| 988794 (web) | enums: status, entity type, privacy, RSVP response, notice types |
| 698441 (web) | `GuildScheduledEventStore` |
| 794782, 974930, 604701 (web) | recurrence ↔ API shape, recurrence options, date strings |
| 931991 (web) | permission hooks |
| 568036 / chunk 18181 | `GuildEventsModal` (the list) |
| 688218 / chunk 664743 | `ScheduleEventModal` (create/edit wizard) |
| 383216 / chunk 508425 | date/time + frequency fields |
| 237519 / chunk 605732 | event details modal |
| 924037 / chunk 628673 | event context menu |
| 163765 / chunk 612502 | Start Event modal |
| 482857 / chunk 798354 | card action handlers |
| 428978, 625142 / chunk 402076 | card buttons, share link |
| 567550 / chunk 908170 | card layout |
| 103355, 492260 / chunk 147626 | status/date line |
| 441477 | channel list (`rM` = Events row) |
| 499817 | guild header popout |
| 359047 / chunk 204309 | "Create Event" menu item |

## States

- **Status [O]** (`988794`): `SCHEDULED=1, ACTIVE=2, COMPLETED=3, CANCELED=4`. "Upcoming" means status not in `{3,4}`.
- **Entity type [O]**: `NONE=0, STAGE_INSTANCE=1, VOICE=2, EXTERNAL=3, PRIME_TIME=4`. Channel-backed types are 1 and 2. Type 3 carries `entity_metadata.location`.
- **Privacy [O]**: `PUBLIC=1, GUILD_ONLY=2`. The form always defaults to 2: `privacyLevel:e?.privacy_level??s.dD.GUILD_ONLY`.
- **RSVP response [O]**: `UNINTERESTED=0, INTERESTED=1`. `UNINTERESTED` exists to opt one occurrence out of a series RSVP.
- **Store ordering [O]**: active events first, then by start time ascending, then by id.
  ```js
  i=L(e)?"\0":"\x01";return`${i}-${new Date(n).getTime()}-${t}`
  ```
- **Status line on the card [O]** (`103355`) has six states:
  - 0: date
  - 1: within 1 h of start → "Starting in {minutes}m", or "Starting soon" at ≤0 min
  - 2: past start but not started → "Starting soon"
  - 3: active → "Live Now" for channel events, "Happening Now" for EXTERNAL. With an end time it reads "{start} — Ends {end}".
  - 4: ended
  - 5: canceled → "Canceled"
- **Date line [O]** (`974930.CC`): moment `calendar()` `sameDay`/`nextDay` when the start is less than 1 day away ("Today at 5:00 PM", "Tomorrow at …"). Otherwise `"ddd MMM Do · LT"`, with `, YYYY` added when the year differs. An end on the same day shows `LT` only, joined as "{start} — {end}".
- **Time windows [O]**:
  - `withinStartWindow` = start ≤ now+15 min. This gates the card's Start button.
  - `upcomingEvent` = start ≤ now+1 h.
  - Notice types: `EVENT_STARTING_SOON` within 12 h of start; `NEW_EVENT` for 2 days after creation.
  - The channel-list upcoming notice (`508654.WG`) only fires for guilds without COMMUNITY that have `INTERNAL_EMPLOYEE_ONLY`, so it is staff-gated today.
- **Recurring [O]**:
  - Occurrences are addressed by a **recurrence id**: a snowflake built from the occurrence's start timestamp (`G3`: `fromTimestamp(floor(start/1000)*1000)`).
  - Per-occurrence overrides are **exceptions**: `{event_exception_id, event_id, guild_id, scheduled_start_time, scheduled_end_time, is_canceled}`.
  - Expansion uses rrule.js and is capped at now+4 years (`er`).

## Entry points

- **Guild header popout [O]** (`499817`): the item `id:"create-event"`, label "Create Event", uses `CalendarPlusIcon`. It sits after "Create Channel" and "Create Category" in the same group. It is shown when `770666.A(guildId)` passes: ADMINISTRATOR, or CREATE_EVENTS on the guild, or the create permission on any voice/stage channel. The same item appears in the guild icon context menu (`916434`).
- **Channel list row [O]** (`441477` `rM`): a row in the guild action section with id `upcoming-events-{guildId}` and a `CalendarIcon`.
  - Text is "{n} Event(s)" (ICU plural, `IBdqSu`), or "Events" when there are none.
  - It shows an unread dot and mention badge from read-state type `GUILD_EVENT`, both suppressed when `isMuteScheduledEventsEnabled`.
  - Visible when `(canCreate || upcoming.length>0) && !HUB` [O].
  - Clicking opens `GuildEventsModal`. Right-click opens a context menu (`831721`, not fetched).
- **Voice channel rows [O]**: `hasActiveEvent` is set from `CHANNEL_EVENT_ACTIVE(channelId)`. The rendering of that flag was not inspected.
- **"Live now"/"Happening now" banner at the top of the channel list**: not found. The only "Live Now"/"Happening Now" strings in fetched code are the card status line. **[I]** The banner may live in an unfetched chunk.
- **Events list modal [O]** (`568036`):
  - Medium modal with no tabs.
  - Header: calendar icon, title "{n} Events" or "Events", then a `|` divider and a primary sm "Create Event" button (if allowed), and a close X.
  - Body: one card per upcoming event, in store order, with an `isNew` flag for ids newer than the ack.
  - Empty state:
    - "There are no upcoming events."
    - "Schedule an event for any planned activity in your server."
    - If the user has MANAGE_ROLES: "You can give other people permission to create events in **Server Settings > Roles**." (the link opens the Roles settings)
  - On open: fetches user counts per event and `GET /users/@me/scheduled-events?guild_ids=`, requests creator members, and acks `GUILD_EVENT`.
- **Event details modal [O]** (`237519`):
  - Size `lg`, title = event name, cover image (2048 px).
  - Tabs: "Event Info" and "{userCount} Interested" (RSVP_LIST). Tab ids come from enum `EVENT_INFO=0, RSVP_LIST=1`.
  - Opening the RSVP tab fetches users.
  - RSVP copy:
    - "No one's interested in this event yet."
    - "{n} more people are interested."
    - Error: "We are having problems looking up who is interested in this event. Please try again later."
  - Event Info copy: "Created by {user}", "{n} people are interested".
  - Footer: the same actions as the card.
  - The route is `/events/{guildId}/{eventId}[/{recurrenceId}]`.
- **Card layout [O]** (`567550`, `147626`), top to bottom:
  - Optional cover image (thumbnail in the list).
  - Header row: status/date line, a 20 px creator avatar (tooltip "Created by {username}"), and the interested count (tooltip "{n} people are interested").
  - Name, then description (markdown, truncatable).
  - Footer, left: location. That is the channel icon + name (tooltip "Missing permission to connect." when the user can't connect), or `LocationIcon` + location text (linkified when external).
  - Footer, right: a "…" button (context menu) plus action buttons.
  - Stage events add "+N Speakers".
  - Recurring events show "Repeats …" (see Copy).
- **Card buttons [O]** (`428978.Zq`), in order:
  1. Copy Link: "Copy Link" → "Copied" for 1 s. Shown for public channels; otherwise it opens the invite modal.
  2. If active and not EXTERNAL: Join (voice) / Listen In (stage) / Go To Channel (already joined) / Channel Locked.
  3. If the user is lurking: Join Server.
  4. If not lurking and not active: "Interested" (bell icon; becomes check + `active` variant when RSVPed).
  5. If not active and allowed: "Start" (primary).
  6. If active and allowed: "End Event" (EXTERNAL only).

## Create / edit flow (`ScheduleEventModal`, 688218)

- **Steps [O]**: enum `ENTITY=0, SETTINGS=1, PREVIEW=2, SUCCESS=3`. The stepper labels are "Location", "Event Info", "Review". Width 440, slides.
  ```js
  {slideId:0,label:…["56QlKS"]},{slideId:1,label:…["w5/ntT"]},{slideId:2,label:…["8aJzT4"]}
  ```
- **Footer [O]**:
  - Left: "Back" (secondary sm, not on step 0).
  - Right: "Cancel" and a primary button. The primary reads "Next" on steps 0–1 and "Create Event" on Review ("Save Event" when editing). It is disabled until all steps up to the current one are valid.
  - The Success step hides the footer and fires confetti.
- **Step 0, Location [O]**:
  - Title "Where is your event?", subtitle "So no one gets lost on where to go."
  - Radio options:
    - **Stage Channel** (COMMUNITY guilds only): "Great for larger community audio events."
    - **Voice Channel**: "Hang out with voice, video, screenshare, and Go Live."
    - **Somewhere Else**: "Text channel, external link, or in-person location."
  - Disabled options show "There are no available channels." or "You do not have the required server or channel permission."
  - Voice/Stage show a channel picker: "Select a channel", with the note "Events in private channels will only be visible to people who can access that channel."
  - External shows "Enter a location" (required, placeholder "Add a location, link, or something.", `maxLength:100`).
  - Once the event has started: "You cannot edit the location type of an event after it starts."
  - Non-stage COMMUNITY guilds without stage channels get an upsell: "Try using a Stage channel!"
- **Step 1, Event Info [O]**:
  - Title "What's your event about?", subtitle "Fill out the details of your event."
  - Fields, in order:
    - "Event Topic": required, placeholder "What's your event?", `maxLength:100`.
    - "Start Date" / "Start Time": required. Min is now, max is now+365 d (+4 years if recurring).
    - "End Date" / "End Time": shown and required only for EXTERNAL. The code also has optional "Add end date and time" / "Remove end date and time" toggles. The default end is start+1 h.
    - "Event Frequency": required select.
    - "Description": placeholder "Tell people a little more about your event. Markdown, new lines, and links are supported.", `maxLength:1e3`.
    - "Cover Image": hint "We recommend an image that's at least 800px wide and 320px tall.", buttons "Upload Cover Image" / "Change Image" / "Remove Image".
  - The default start is the next whole hour at least 1 h away (`jd`).
- **Frequency options [O]** (`974930.Xx`). `{weekday}` is the long weekday name of the start date; `nth = ceil(date/7)`:

  | value | id | label | rrule |
  |---|---|---|---|
  | NONE 0 | none | Does not repeat | null |
  | WEEKLY 1 | weekly | Weekly on {weekday} | WEEKLY |
  | BIWEEKLY 5 | biweekly | Every other {weekday} | WEEKLY interval 2 |
  | MONTHLY 2 | monthly | Monthly on the {first…fifth} {weekday} | MONTHLY byweekday nth |
  | YEARLY 3 | yearly | Annually on {Mon DD} | YEARLY |
  | DAILY 7 | daily | Every day | DAILY |
  | WEEKDAY_ONLY 4 | weekdayOnly | Every weekday (Monday to Friday) | DAILY byweekday Mo–Fr (shifted for the UTC day offset) |
  | WEEKEND_ONLY 6 | weekendOnly | Every weekend (Saturday and Sunday) | DAILY Sa,Su, **staff only**, offered only when the start is on a weekend |

  WEEKDAY_ONLY is offered only when the start is on a weekday.
- **Step 2, Review [O]**:
  - "Here's a preview of your event." with a card preview.
  - "This event will auto start when it's time." or "When it's time, visit **#channel** to start the event!" **[I]** The first is shown for EXTERNAL, the second for channel events.
- **Step 3, Success [O]**: "All set. Now share your event!", "Copy the event link below to invite people to your event. This also acts as a server invite link." Shows an invite link with a copy button.
- **Validation [O]**: the client strings are listed below. The first two and "Topic must be specified." are hard-coded, not localized.
  - "An event type must be specified."
  - "Either a location or channel must be specified."
  - "Events must have a start date and time."
  - "End date and time are required." (EXTERNAL)
  - "The start time must be in the future." (create only)
  - "The end time must be after the start time."
  - "The end time must be in the future."
  - "Topic must be specified."
- **Editing a recurring series [O]**: when the schedule changes, a confirm appears:
  - Title "Edit all Events in this Series", subtitle "Are you sure you want to edit this event?"
  - Body: "Editing this event will also **update all events** in this series."
  - Buttons: "Save Event" / "Never Mind".
  - Editing a single occurrence opens a separate modal (`462166`, not fetched) that writes an exception.

## Actions on an event (context menu `924037`, `navId:"user-context"`)

Groups appear in this order:

1. **Add to Calendar** (hidden while active): submenu with Google, ICS download, Yahoo, Outlook. ICS uses `ical-generator` with an RRULE.
2. **Start Event** (manager, not active, not on an occurrence row).
3. **Edit Event**, with a recurring submenu: "Edit this Event" / "Edit all Events in this Series".
4. Share (`24661`, not inspected).
5. **Cancel Event** (danger). Recurring submenu: "Cancel this Event" / "Cancel all Events in this Series".
   - Confirm title "Cancel Event?", subtitle "Are you sure you want to cancel this event?"
   - Confirm button: "Cancel Event", or "Cancel all Events" plus the body "Canceling this event will also **cancel all events** in this series."
   - Dismiss: "Never Mind".
   - **Confirming sends `DELETE`, not `PATCH status 4` [O].** Canceling one occurrence sends an exception with `is_canceled:true`.
6. **Restore Event** (canceled occurrence): deletes or updates the exception.
7. **Copy Event Link**: copies `${location.origin}/events/{g}/{id}[/{recurrenceId}]`, toast "Link Copied!". Recurring submenu: "Copy Link for Only this Event" / "Copy Link for all Events".
8. On occurrence rows: **Mark as Interested** / **Mark as Not Interested**, submenu "Only this event" / "All events in this series".
9. Separate groups: "Report Event"; "Copy Image Link"; "Copy Event ID", "Copy Recurrence ID" (developer mode).

Other actions:

- **Card Start [O]** opens the Start modal (`163765`).
  - Title = event name; subtitle "You are about to start this {Public|Private} event".
  - Stage events get a "Notify @everyone" checkbox, default on.
  - Button: "Start Event".
  - What starting does:
    - STAGE: create the stage channel if it is missing, then `POST /stage-instances {topic, privacy_level:2, guild_scheduled_event_id, send_start_notification}`.
    - VOICE / EXTERNAL: `PATCH {status:2}`.
    - Afterwards the client joins the voice or stage channel.
  - The card shows Start only when `withinStartWindow` (≤15 min) [O]. The menu item has no time gate [O].
- **End Event [O]**: EXTERNAL and active only. Confirm "End Event" / "Are you sure you want to end the event?" / "Yes, end the event" → `PATCH {status:3}`. **[I]** Voice and stage events end server-side when the channel empties or the stage instance closes.
- **Card delete [O]**: the card's `onDeleteClick` uses the same "Cancel Event?" confirm with the "Delete" button → `DELETE`. Not allowed while active.
- **Share [O]**: public channel → copy `https://discord.com/events/{g}/{id}` (`625142.d`). Otherwise → invite modal (`137811`: "Invite friends to event", "Or, send an event invite link to a friend").
- **Interested [O]**: optimistic dispatch, then `PUT .../users/@me {response}`, rolled back on error. Toggling off sends `DELETE`. Recurring RSVP logic is in `929267.QC`: RSVP the whole series or one occurrence, or open a picker with "All events in this series" / "Only this event".

## Permissions [O]

- `CREATE_EVENTS = 1<<44`, `MANAGE_EVENTS = 1<<33`.
- **Manage check** (`931991._`): `canManageAll || (canCreate && event.creator_id === me)`. Creators can manage their own events; MANAGE_EVENTS covers everyone's.
- **Channel events** need the permission on the channel together with:
  - voice: VIEW_CHANNEL + CONNECT
  - stage: VIEW_CHANNEL + stage moderator (MANAGE_CHANNELS | MUTE_MEMBERS | MOVE_MEMBERS)
  ```js
  e.isGuildStageVoice()?t=d.Ou:e.isGuildVoice()&&(t=d.EN),[r.kg(t,c.xB.CREATE_EVENTS),r.kg(t,c.xB.MANAGE_EVENTS)]
  ```
- **Visibility**: events in channels the user can't VIEW_CHANNEL are filtered out of lists (`508654.Ay`).

## Limits [O]

| Field | Limit |
|---|---|
| topic | 100 |
| location | 100 |
| description | 1000 |
| start | ≤ now+365 d (≤ now+4 years with recurrence) |
| stage channel name (auto-created) | `name.substring(0,100)` |
| cover image | recommended ≥800×320; served as `https://{CDN}/guild-events/{id}/{image}?size=N` |
| embed user-count refetch | 30 min cache, ≤8 exception ids per batch |

## Requests [O] (`496092`)

```
POST   /guilds/{g}/scheduled-events                    create
GET    /guilds/{g}/scheduled-events                    list (no query params sent)
GET    /guilds/{g}/scheduled-events/{id}               fetch one
PATCH  /guilds/{g}/scheduled-events/{id}               save / {status:2} start / {status:3} end / {status:4} cancel*
DELETE /guilds/{g}/scheduled-events/{id}               "Cancel Event" in UI
GET    /guilds/{g}/scheduled-events/{id}/users/counts?guild_scheduled_event_exception_ids=…
         -> {guild_scheduled_event_count, guild_scheduled_event_exception_counts}
GET    /guilds/{g}/scheduled-events/{id}[/{exc}]/users?limit=100&with_member=true
PUT    /guilds/{g}/scheduled-events/{id}[/{exc}]/users/@me   body {response:0|1}
DELETE /guilds/{g}/scheduled-events/{id}[/{exc}]/users/@me
GET    /users/@me/scheduled-events?guild_ids=[g]       my RSVPs
POST   /guilds/{g}/scheduled-events/{id}/exceptions    {original_scheduled_start_time, scheduled_start_time, scheduled_end_time, is_canceled}
PATCH  /guilds/{g}/scheduled-events/{id}/exceptions/{exc}  {scheduled_start_time, scheduled_end_time, is_canceled}
DELETE /guilds/{g}/scheduled-events/{id}/exceptions/{exc}
```

- `*` `cancelGuildEvent` (PATCH status 4) is defined but has no caller in any fetched chunk. Bounded search: web.js plus the about 215 chunks fetched.
- `with_user_count=true` is **not used** by this client. Counts come from `/users/counts` [O].
- **Create/PATCH body [O]**: `{name, description, image (data: URI or omitted), privacy_level, scheduled_start_time, scheduled_end_time, entity_type, channel_id, entity_metadata, recurrence_rule}`. On save, `channel_id` is null unless the type is 1/2, and `entity_metadata` is null unless the type is 3.
- **`recurrence_rule` [O]**: `{start, end, frequency, interval, by_weekday, by_n_weekday:[{n, day}], by_month, by_month_day (null if empty), by_year_day, count}`. `frequency` and day numbers follow rrule.js: `YEARLY=0, MONTHLY=1, WEEKLY=2, DAILY=3`, weekday `MO=0 … SU=6`. **[I]** The numeric values come from rrule.js, not from Discord code.
- **Event fields read by the client [O]**: `id, guild_id, channel_id, creator_id, name, description, image, scheduled_start_time, scheduled_end_time, privacy_level, status, entity_type, entity_id, entity_metadata.location, recurrence_rule, guild_scheduled_event_exceptions[]`.
- Event links route to `/events/{g}/{id}[/{recurrenceId}]`.

## Gateway [O]

- `READY`: each `guilds[].guild_scheduled_events` is loaded (`CONNECTION_OPEN` handler).
- `GUILD_CREATE`: `guild.guild_scheduled_events`.
- `GUILD_SCHEDULED_EVENT_CREATE` / `_UPDATE`: upsert. `_DELETE`: remove.
- `GUILD_SCHEDULED_EVENT_USER_ADD` / `_USER_REMOVE`: `{user_id, guild_id, guild_scheduled_event_id, guild_scheduled_event_exception_id, response}`.
- `GUILD_SCHEDULED_EVENT_EXCEPTION_CREATE` / `_UPDATE` / `_DELETE`: payload is the exception object.

## Observed vs inferred, open items

- Everything tagged [O] was read in shipped minified code and message bundles. None of it was observed running (no live session).
- [I]: when "auto start" vs "visit #channel" copy appears; server-side ending of voice/stage events; rrule numeric constants.
- Not found or not fetched:
  - a dedicated "Live now" banner at the top of the channel list
  - the Events row context menu (`831721`)
  - the single-occurrence edit modal (`462166`)
  - the Share item (`24661`)
  - the events-row visibility source of `GUILD_ACTION` rows beyond `(canCreate || upcoming>0) && !HUB`
