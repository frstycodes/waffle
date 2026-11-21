# Discord app commands, replies, components and modals: reverse-engineered spec vs Waffle

Source: Discord web client build fetched 2026-11-21 from `https://discord.com/app` (no auth, no cookies). The desktop
cache under `~/Library/Application Support/discord/` was unreadable (macOS TCC: `Operation not permitted`), so the
public web bundles were used instead; desktop loads the same bundles. Extracted tree:
`/private/tmp/claude-501/-Users-frsty-dev-dango/b118311f-89b2-43c0-99d2-b9b84d64a911/scratchpad/discord-js/`
(`assets/` raw chunks, `mods/` main bundle split per webpack module, `lazy/` lazy chunks split per module,
`css/`, and `../en/messages.json` with the en-US intl table used to resolve hashed string keys).

References: `web#NNNN` = module NNNN of `assets/web.355a76e660bee7a5.js` (also `mods/NNNN.js`);
`chunk:<file>#NNNN` = module NNNN in a lazy chunk. Strings are quoted from the en-US table with their key.
**Observed** = read in code. **Inferred** = deduced from code shape, not seen executing.

---

## 1. Composer and slash commands

**Opening the picker**
- Observed: `/` only triggers at the start of the message (`isAtStart:!0` for the COMMANDS provider), and only while no
  command is active. `chunk:223862#496533` provider `B`: `sentinel:"/"`, `matches:…null==M.A.getActiveCommand(e.id)`.
- Observed: an empty query (`/` alone) returns no inline results (`if(0===n.length…)return Y`); the bare `/` shows the
  command browser instead (`web#989161`, `placeholderCount:7,limit:10,includeFrecency:!0`). Inferred: browser for `/`,
  inline list for `/q…`.
- Browser (observed, `web#989161`): a category rail (one icon per section with tooltip, an `<hr>` after the frecency
  section); clicking a rail item filters the list to that section, clicking frecency/the active one clears the filter.
  Sections page 10 commands at a time and load more on scroll. First row is preselected (`useState(0)`), up/down clamp
  at the ends (no wrap), Tab/Enter pick.
- Inline list (observed, `B.queryResults`): `limit:20`, `scoreMethod:COMMAND_OR_APPLICATION`, header "Commands matching
  **{prefix}**" (`HFRoZR`) / "Commands" (`0hKkS+`). With a trailing space (`/music `) only commands named exactly that or
  starting with `that ` remain.
- Frecency (observed, `web#264322` `XC`): top 5 commands by Discord's server-synced frecency score form a first
  section (`Ik.FRECENCY`), and also stay in their app section: `commands:e.concat(s)`, `.splice(0,5)`.
- Data scope (observed, `web#264322`, `web#577700`): the store merges the context index (guild or channel) **and** the
  user index (`/users/@me/application-command-index`, user-installed apps); when an app is in both, the user index's
  commands win and the guild's extra ones are appended.
- Section order (observed, `er`): app frecency score, then name (`localeCompare`, numeric). Built-ins last.
- Command order (observed, `er`/`ea`): global sort by match score (lower is better), then frecency, then display name.
  Score tiers: 0 name prefix · 1 first word prefix and rest-of-words prefix (`/mu q` → `music queue`) · 2 name contains ·
  3 option-name prefix · 4 option-name contains · 5 app name prefix · 6 app name contains · 7 description contains ·
  8 app description contains · 9 fuzzy name · 10 fuzzy option name · 11 fuzzy app name. Case-insensitive.
  `if(r.startsWith(t)||s.startsWith(t))return 0;…if(r.includes(t)||s?.includes(t))return 2;`
- Subcommands are flattened into separate entries (`untranslatedName:[n.name,...l].join(" ")`, `web#168186`), like Waffle.

**Keys (observed, `web#84046` key handler)**
- Up/Down and Ctrl-P/Ctrl-N move the autocomplete selection; Esc hides the autocomplete.
- Tab: the autocomplete takes it first (pick); else, with an active command, Tab selects the next option pill and
  Shift-Tab the previous one. `e.shiftKey?X.b.selectPreviousCommandOption(t):X.b.selectNextCommandOption(t)`
- Past the last pill, Tab puts the caret at the end of the editor (`resetSelectionToEditorEnd`, `web#35277`).
- Enter: autocomplete pick first, else submit.

**What picking inserts (observed, `web#870748`)**
- An `applicationCommand` block: text `/displayName`, then one `applicationCommandOption` pill for **every required
  option** (plus any with initial values), in definition order. The caret goes into the first empty pill.
  `for(let i of a.options)if(!e.has(i.name)&&(i.required||null!=r[i.name]))…_.push(a)`
- Shortcut: if the command has exactly one option, it is optional and not an attachment, text typed after `/name ` is
  turned into that option's pill (function `b`).
- Typing `/name ` by hand (trailing space, exact name match) activates the command the same way.
- Typed `name:` text becomes a pill: regex `/([\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}_-]+):/gu`, must follow whitespace,
  first occurrence per option only (`web#317681` `pY`).

**The option pill (observed, `web#84046` `em` + CSS `css/358931.*.css`)**
- `<span optionPill>` = `<span optionPillKey contentEditable=false onClick=select>{displayName}</span>` +
  `<span optionPillValue>{editable children}</span>`. So the name segment sits inside the pill and the value is
  edited inside it.
- CSS: pill `border:1px solid transparent; border-radius:4px; max-width:calc(100% - 30px)`; key segment
  `padding:1px 8px`, darker background (`--background-surface-high` dark / `--background-base-lowest` light), left
  corners rounded; value `padding:1px 8px; white-space:pre-wrap`. Pill background `--background-mod-normal` (dark).
- Selected pill: `border-color:Highlight`; errored: `border-color:var(--red-400)`. Errored shows only when not selected.
- Attachment options are void pills whose value reads "Please attach a file" (`GRdFni`) until a file is staged, then the
  filename in brand colour.
- Trailing placeholder after the pills: "+{count} optional" (`0mI72g`) or "+{count} more" (`BP8N0K`).

**Deleting (observed, `web#68059` `v`, `web#870748` `D`)**
- Backspace/Delete in an empty or void pill removes the whole pill. Walking over an empty pill also removes it.
- If the text stops starting with `/displayName` (you delete into the name), or the editor becomes empty
  (select-all + delete), the command is dropped and the content is flattened back to plain text.
- Enter inside a STRING pill can insert a newline (`R(e)?r("\n")`, used by insertBreak; inferred: Shift-Enter). Pasting
  multi-line text into a non-string pill joins lines with spaces.

**Option-name list (observed, provider `K`)**
- Shown when a command is active and the caret is outside any pill (end of editor). Lists options whose displayName
  `startsWith` the typed text (case-sensitive) and that have no value yet; required first under "Required options"
  (`7II2G3`), then "Other options" (`TpDXm4`) or "Options" (`+1H47t`). Picking inserts `name:` which becomes a pill.

**Choices and autocomplete (observed, provider `F`, `web#166862`, `web#721768`)**
- Shown for BOOLEAN (fixed `True`/`False`), options with `choices`, and `autocomplete` options. Filtered by
  `queryChoiceResults` (no limit). Picking inserts the choice's displayName into the pill, validates it, then selects
  the next pill.
- Autocomplete request: `debounce(…,500,{leading:!0,trailing:!0})`. Results are cached per (option, query) and dropped
  when the other options' values change (`contextKey` = `name=value\0…` of non-focused options). No request is sent
  when the cache has the query.
- While loading: 5 skeleton rows. On INTERACTION_FAILURE for that nonce: "Loading options failed" (`rTAbPn`). Empty:
  "No options match your search" (`41014u`). Choice labels use `name_localized ?? name`.

**Validation (observed, `web#300541`, `web#73510`)**
- Runs live on the active pill and on all pills at submit. Messages:
  - missing/empty required: "This option is required. Specify a value." (`JZJQL2`)
  - choices: "Not a valid choice." · boolean: "Pick either true or false." · integer: "Input a valid integer." ·
    number: "Input a valid number." · user: "Not a valid user." · channel: "Not a valid channel." ·
    role: "Not a valid role." · mentionable: "Not a valid user or role." · else "Not a valid value."
  - string length: "Enter {value} characters." / "Enter between {minimum} and {maximum} characters." /
    "Enter {minimum} or more characters." / "Enter {maximum} or fewer characters." (JS `.length`, UTF-16)
  - number range: "Enter a number between {minimum} and {maximum}." / "…greater than or equal to {minimum}." /
    "…less than or equal to {maximum}."
- Integers must be `Number.isSafeInteger`; numbers are parsed locale-aware (`P.ib(locale, text)`).
- USER/CHANNEL/ROLE/MENTIONABLE accept a mention node, a bare snowflake, or plain text resolved to a mention
  (`(0,o.f)(text, guild, channel)`); MENTIONABLE/ROLE accept `@everyone` (sent as the guild id).

**Submit with errors (observed, `web#84046` `eX`)**
- Validates every pill; inserts a pill for each missing required option; selects the first bad one; marks the pills
  errored (red border); shakes the **whole app**: `dispatch(SHAKE_APP,{duration:200,intensity:2})`.
- The header bar above the composer (`web#927659` `k`) shows app icon (32px), `/command`, option names (required, set
  optional, "+N optional" with a tooltip listing the rest) and, when the active option failed validation, the error in
  red **in place of the option description**. Bot nickname/section name on the right.

**Limits seen**: inline list 20, browser 10 per page, frecency 5, autocomplete skeleton 5, select `maxVisibleItems:5`,
channel option search `limit:10`.

## 2. `POST /interactions` bodies and the command index

All JSON (`Bo.post({url:Rsh.INTERACTIONS, body:…})`). Attachments are cloud-uploaded first and referenced by
id, so there is no multipart. Nonce = snowflake from `Date.now()` (`fromTimestamp(Date.now())`).

**Type 2, command (observed, `web#870801` `handleCommand`, `web#545152`)**
```js
{type:2, application_id, guild_id, channel_id, session_id, data, nonce, analytics_location, section_name, source}
data = {version, id: rootCommand.id, guild_id: command.guild_id, name: rootCommand.untranslatedName, type,
        options, application_command: rootCommand, target_id?, attachments: [ {id:"0", filename, uploaded_filename, description?, is_spoiler?} ]}
```
- `analytics_location` is `"slash_ui"` from the chat composer (`default:return w.Oh.SLASH_UI`); `section_name`/`source`
  are usually undefined, so they are dropped from the JSON.
- `data.name` is the **untranslated** root name; the index normalizer maps `name_default→name`, keeping the localized
  one as `name_localized` (`web#264322` `eo`). `application_command` is that normalized root command object.
- `data.guild_id` is the command's own `guild_id` (guild-scoped commands only).
- Options are emitted in **command definition order**, not text order: `for(let e of t.options)…g.push({type,name,value,focused})`.
  Subcommand nesting wraps innermost-out: `g=[{type:i,name:n,options:g}]`.
- Values: STRING → trimmed text, a choice's value via displayName, or a cached autocomplete choice's value via its
  displayName; INTEGER/NUMBER → `Number(…)`; BOOLEAN → bool; USER/ROLE/CHANNEL/MENTIONABLE → id string;
  ATTACHMENT → numeric index into `data.attachments`. `focused` is `true` or absent.
- `data.attachments` is always present on type 2 (`[]` when none).

**Type 4, autocomplete (observed, `web#721768` `WL`)**
```js
{type:4, application_id, guild_id, channel_id, session_id, data /* same as type 2, no attachments */, nonce}
```
- Focused option's value = the raw query string for every type; other options with empty strings are skipped;
  attachment options are skipped. HTTP `timeout:3e3`; on HTTP error dispatches INTERACTION_FAILURE for the nonce.

**Type 3, component (observed, `web#964363` `m`)**
```js
{type:3, nonce, guild_id, channel_id, message_flags, message_id, application_id, session_id,
 data:{component_type, custom_id, …localState}}
```
- Button: `data = {component_type:2, custom_id}`.
- String select: `data = {component_type:3, custom_id, type:3, values:[…]}`. User/role/mentionable/channel select:
  `{component_type, custom_id, type, values:[ids]}`, so the select's type appears twice.
- `application_id = message.application_id ?? message.author.id`. HTTP `timeout:3e3`.
- One in-flight interaction per message: `canQueueInteraction(messageId, nonce)` refuses a second until the first fails.

**Type 5, modal submit (observed, `chunk:945413#969508`)**
```js
{type:5, application_id, channel_id, guild_id, data:{id: modal.id, custom_id, components, attachments?}, session_id, nonce}
```
- `components` mirror the modal's structure: ACTION_ROW → `{type:1, components:[…]}`; LABEL →
  `{type:18, component:{…}}`; TEXT_INPUT → `{type:4, custom_id, value}` (`null` if the field was never given a value);
  selects → `{type, custom_id, values}`; TEXT_DISPLAY → `{type:10}`; FILE_UPLOAD → `{type:19, custom_id, values:[upload indexes]}`;
  RADIO_GROUP/CHECKBOX → `value`; CHECKBOX_GROUP → `values`.
- HTTP 429 is retried after `retry_after`; other errors → INTERACTION_FAILURE.

**Error handling of the POST (observed, `web#964363` `g`)**: 4xx `INVALID_FORM_BODY` whose first error is
`INTERACTION_APPLICATION_COMMAND_INVALID_VERSION`/`…_INVALID` or code `UNKNOWN_INTEGRATION` marks the index stale
(APPLICATION_COMMAND_EXECUTE_BAD_VERSION) and fails the interaction with the server's message.

**Command index (observed, `web#652215`, `web#577700`, `web#264322`)**
- `GET /guilds/{id}/application-command-index` (guild channels), `GET /channels/{id}/application-command-index`
  (DMs/GDMs), `GET /users/@me/application-command-index` (user-installed apps, always), and
  `GET /applications/{id}/application-command-index` (single-app contexts).
- `202` → retry after 5 s; `429` → retry after `retry_after`; max 3 tries, then FAILURE with `retryAfter = now+5s`.
- Staleness: every index is marked stale on CONNECTION_OPEN (each gateway (re)connect) and on a bad-version error;
  refetched on next use when `result.version !== serverVersion`. In-flight fetches are aborted when superseded.
- `GET /channels/{c}/application-commands/search` exists (`APPLICATION_COMMANDS_SEARCH`) but was not seen on the composer path.

## 3. Gateway handling and the command lifecycle

**Dispatch parsing (observed, `web#225123`)**
- `INTERACTION_CREATE {id, nonce}`, `INTERACTION_SUCCESS {id, nonce}`, `INTERACTION_FAILURE {id, nonce, reason_code}`,
  `APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE {choices, nonce}`,
  `INTERACTION_MODAL_CREATE {id, channel_id, custom_id, application, title, components, nonce, resolved}`,
  `INTERACTION_IFRAME_MODAL_CREATE` (activity modals).

**Interaction store (observed, `web#436283`)**: QUEUED → CREATED (INTERACTION_CREATE) → done on INTERACTION_SUCCESS,
INTERACTION_MODAL_CREATE, or a MESSAGE_CREATE whose `nonce` equals the interaction nonce. On INTERACTION_FAILURE a
command is dropped; a component stays in FAILED (so its error keeps showing) until CHANNEL_SELECT clears failed entries.

**Local placeholder (observed, `web#545152` `H`)**: a local message is inserted immediately, with type
`CHAT_INPUT_COMMAND` (20), author = the app's bot user, empty content,
`interaction:{id, name, name_localized, type:2, user: me}` and `interaction_data`. Its id **is** the nonce
(`get nonce(){return o.id}`), so the app's MESSAGE_CREATE carrying that nonce replaces it through normal optimistic
send. INTERACTION_CREATE writes the real interaction id into `interaction.id`. On failure: MESSAGE_SEND_FAILED with
the reason. When a modal answers the command, INTERACTION_MODAL_CREATE with the command's nonce deletes the placeholder
locally (`web#750128`).

**Status line (observed, `web#594808`, `web#964363` `fK`)**
- SENDING (queued, before INTERACTION_CREATE): "Sending command..." (`RiLfBY`) with animated dots.
- CREATED, or a server message with the LOADING flag (deferred): "{applicationName} is thinking..." (`7ePV4t`),
  applicationName = `author.username`.
- TIMED_OUT: "The application did not respond" (`h8hzPd`), red (`text-feedback-critical`) with CircleErrorIcon, when
  (a) a LOADING message is older than **15 min** (`extractTimestamp(id)+9e5`), or (b) an interaction message is
  SEND_FAILED more than 3 s after it was created.
- FAILED (failed under 3 s): `message.interactionError` or "This interaction failed" (`VCsUJu`).
- No client-side timer for commands: Discord waits for the gateway (INTERACTION_FAILURE arrives when the app doesn't
  answer in 3 s). Inferred from the absence of any timeout in `handleCommand` (no `timeout:` option) and in the store.
- Retry: a failed command message can be resent from its stored `interactionData` (`web#545152` export `x`).

## 4. Ephemeral messages

- Observed (`chunk:599719#376767`): footer = EyeIcon + "Only you can see this • [Dismiss message]" (`vgUjKR`,
  `uX3ecL` with count 1). With `interaction_metadata.ephemerality_reason` set: "Only you can see this message.
  {reason} • Dismiss" (`wx1tLQ`), reasons from an 18-value enum (slowmode, rate limit, missing permissions, automod,
  cannot use external apps, …). No dismiss handler → plain "Only you can see this" (`Nb1EQx`).
- Dismiss calls `onDeleteMessage(message, true)`. `deleteMessage(channel, id, local=true)` dispatches only a local
  MESSAGE_DELETE, with no API call (`web#138399`). Inferred that `true` maps to `local`.

## 5. Components

**Parsing (observed, `web#257249` `D`)**: every component gets a stable id hashed from its index path. A SECTION
with no text children or no accessory is dropped. Fields kept: button `sku_id`; selects `required`, `default_values`,
`channel_types`; thumbnail `spoiler`; file `size`, `spoiler`; separator `divider ?? true`, `spacing ?? SMALL`.
Types: 1 row, 2 button, 3/5/6/7/8 selects, 4 text input, 9 section, 10 text, 11 thumbnail, 12 gallery, 13 file,
14 separator, 16 content inventory, 17 container, 18 label, 19 file upload, 20 checkpoint card, 21 radio group,
22 checkbox group, 23 checkbox.

**Buttons (observed, `chunk:30441#736157` `V`)**
- Colours: PRIMARY and PREMIUM → brand; SUCCESS → green; DESTRUCTIVE → red; SECONDARY and LINK → grey. Size small.
- LINK opens through the link-safety confirm (`shouldConfirm:!0`) and shows WindowLaunchIcon; it has a context menu.
- PREMIUM (`sku_id`): ShopIcon, label "{skuName} - {price}" (`c6Q+B3`), opens the subscription/item modal;
  "Product Unavailable" (`CHa0vN`) disabled when the SKU can't be bought.
- Loading: the label is hidden (`aria-hidden`) and animated dots are drawn over it.

**In-flight state (observed, `chunk:945413#911408` `visualState`)**: while a component interaction on a message is
pending, the clicked component is LOADING and **every other interactive component on that message is DISABLED**.
Components are also disabled where you can't chat (lurking, pending member, locked/archived thread without rights).

**Failure (observed, `web#257249` `m`, `web#594808`)**: under the ACTION_ROW or SECTION holding the failed component,
a red line with CircleErrorIcon: `message.interactionError` ?? "Slow down! You're clicking too fast." (`fitPBS`, HTTP 429)
?? the `reason_code` text ?? "This interaction failed". No retry button; it stays until you switch channel.

**Selects (observed, `na`, `ep`)**
- Placeholder default "Make a selection" (`Otr6W2`). 5 visible rows.
- Single select sends on pick and closes. Multi (`max_values>1`): stays open, unpicked options are disabled at
  `max_values`, and the interaction is sent **when the menu closes** if the picks changed. There is no Done button.
- String defaults: `options[].default`. User/role/channel/mentionable: `default_values` resolved to labels
  (user: guild nick ?? global name ?? username; channels filtered by `channel_types`); searchable list.
- In a message, a selection under `min_values` blocks the send; the count errors ("Select at least {count}",
  `Jmwzdx`) only render in modals. Inferred.

**Components V2 rendering (observed, `736157` + `css/243794.*.css`)**
- Container: `background:--background-surface-high; border:1px solid --border-subtle; border-radius:--radius-sm;
  padding:--space-16; gap:--space-8`. Accent: a **4px left stripe** (`:before{width:4px;background:var(--__accent-color)}`).
  Text inside is "reduced" size. Spoiler: the whole container is blurred behind a click-to-reveal obscure layer.
- Section: text column, accessory (button or thumbnail) on the right; failure line under it.
- Thumbnail: keeps aspect, min 85 px, max 170 px; spoiler supported; non-images use an 85 px placeholder.
- Media gallery: the regular attachment mosaic, using each item's real `width`/`height`; invalid items filtered out;
  per-item spoiler; the viewer pages through the gallery.
- Text display: markdown with headings, lists, links, game mentions.
- Separator: `divider` adds a line class, `spacing===LARGE` adds a large-spacing class. **Unknown**: exact px. The CSS
  module `210716` isn't in any downloaded chunk (`grep '210716(' assets/*.js` → none).
- Unknown component types → "Please update Discord to view this message" (`zksHZO`).

**Interaction header (observed, `chunk:975933#267508`)**: "{user} used {command}" (`rg7U+C`). The command is a chip
with AppsIcon + name that opens a popout of the options used (`GET …/messages/{id}/interaction-data`,
`web#706727` `S7`). A user-targeted command appends "› {target}" (`mqKdCM`). The command name comes from
`interaction.displayName`.

## 6. Modals

Observed (`chunk:594055#121338`, `chunk:945413#969508`, `#911408`, `web#750128`):
- Header: app icon 24 px + title (`heading-xl/semibold`). A warning banner is always shown: "This form will be
  submitted to **{applicationName}**. Do not share passwords or other sensitive information." (`dSTy/w`). The
  applicationName is the bot's guild nickname, then the bot username, then the app name.
- Body renders any component tree: LABEL (title, required mark, description, child), TEXT_INPUT (short or autosizing
  paragraph, native `minLength`/`maxLength` attributes), selects, file upload, radio/checkbox groups, text display.
  The first field autofocuses.
- Footer: full-width Cancel (secondary) + Submit (primary, `loading` while IN_FLIGHT). Enter submits (it is a `<form>`).
- Validation on submit, per field, shown under the field: "This field is required." (`eJEUvD`; empty means
  `value.length===0`, no trimming), "Must be between {min} and {max} in length." (`ONSqYd`); selects "Select at least
  {count}" / "Select at most {count}".
- Client `required` default is `false` when the field omits it (`required:t.required??!1`). Unknown: whether the
  gateway always sends it.
- Submit lifecycle: one modal submit at a time. The client fails it after **10 s** with no answer (2 min while
  uploads are staged, then 10 s). INTERACTION_SUCCESS → close and clear state; INTERACTION_FAILURE → stays open with a
  red alert at the top: "Something went wrong. Try again." (`uJgdEu`).
- Closing: Cancel, the modal's own close/Escape (standard modal `onClose`). Unknown: whether clicking outside closes it
  (generic modal layer, not checked).

---

## Waffle gaps

Waffle paths are relative to `/Users/frsty/dev/dango/.claude/worktrees/slash-commands/`.

### (A) Wire and protocol

1. **No `analytics_location` on commands.** Discord sends `analytics_location:"slash_ui"` on type 2. Waffle's
   `envelope` (`src/app_commands.rs:203-216`) has no such field. Change: add it to the type-2 body in `command_body`.
2. **`data.name` and option names may be localized.** Discord sends `name_default`-derived untranslated names. Waffle
   takes `c["name"]` (`src/app_commands.rs:126`, `:160`) and echoes the raw object as `application_command` (`:134`).
   Change: parse `name_default ?? name` for commands, subcommands, options and choices; keep `name_localized` for
   display; match typed text against both.
3. **`data.guild_id` missing.** Discord adds the command's own `guild_id` for guild-scoped commands. Waffle never sets
   it (`src/app_commands.rs:220-228`). Change: carry `guild_id` from the index and add it when present.
4. **Option order.** Discord emits options in definition order; Waffle emits them in typed order
   (`src/command_line.rs:126-134`, `:189-199`). Change: sort `values` by the option's index before building `data.options`.
5. **Type 4 carries `attachments: []`.** Discord's autocomplete `data` has no `attachments`; Waffle always adds it
   (`src/app_commands.rs:227`). Change: add it only for type 2. Probably harmless.
6. **Select component `data` lacks `type`.** Discord sends `{component_type, custom_id, type, values}`; Waffle sends no
   `type` (`src/app_commands.rs:239-242`). Change: add `"type": kind` for selects.
7. **Modal submit flattens to action rows.** Discord mirrors the modal tree (LABEL → `{type:18, component:{…}}`,
   TEXT_DISPLAY → `{type:10}`, `value:null` if never set). Waffle always wraps each input in `{type:1}` and sends `""`
   (`src/app_commands.rs:250-258`). Waffle also drops every non-text-input field when parsing (`:480-495`): selects,
   radio, checkbox, file upload, text display in labels. Change: keep the parsed tree and serialize it back in the same
   shape.
8. **No user-installed apps.** Discord also fetches `GET /users/@me/application-command-index` and merges it. Waffle
   fetches only the guild/channel index (`src/discord.rs:1208-1214`, `src/command_picker.rs:240-257`). Change: fetch
   and merge the user index (user wins on overlap).
9. **Index never refreshes; 202 not handled.** Discord retries 202 after 5 s and 429 after `retry_after`, up to 3 tries,
   marks every index stale on each gateway CONNECTION_OPEN, and refetches after an `…_INVALID_VERSION` error. Waffle
   caches per scope forever (`src/command_picker.rs:233-238`). Not verified: how `run` handles a 202 with no body;
   at best it parses to an empty index. Change:
   handle 202/429, clear `indexes` on READY/RESUME, and refetch on a bad-version error.
10. **HTTP errors from `POST /interactions` lose their message.** Discord shows the server's error message on the
    placeholder and refetches the index on version errors. Waffle only flags `failed` (`src/interactions.rs:49-52`).
    Change: return the error body from `interact` and show it.
11. **`reason_code` ignored.** Discord maps `INTERACTION_FAILURE.reason_code` to a message and shows a 429 as "Slow
    down!". Waffle parses only `nonce` (`src/discord.rs:2096`). Change: parse `reason_code`, map it, special-case 429.
12. **A client timer fails commands and components.** Discord has none for commands or components (it waits for
    INTERACTION_FAILURE, and component POSTs have a 3 s HTTP timeout); only modal submits time out client-side after
    10 s. Waffle's `NO_ANSWER` = 10 s fails any interaction without INTERACTION_CREATE (`src/interactions.rs:16`,
    `:54-60`). Change: keep the 10 s only for modals. For commands, rely on INTERACTION_FAILURE plus the 15-min LOADING
    rule (gap 14).
13. **Attachments unsupported.** Discord stages uploads, sends `data.attachments:[{id,filename,uploaded_filename,…}]`
    and the option value is the index. Waffle refuses ("Attachments aren't supported yet",
    `src/command_line.rs:156-158`). Change: implement via the cloud-upload path the composer already uses for messages.

### (B) Composer

14. **Status wording and timing.** Discord shows "Sending command..." until INTERACTION_CREATE, then "{app} is
    thinking...". A deferred (LOADING) reply turns into "The application did not respond" after 15 min. Waffle shows
    "is thinking…" immediately (`src/interactions.rs:43-46`, `src/components.rs:342-352`), has no 15-min LOADING timeout,
    and its failure reads "{app} didn't answer in time. It may be offline." + Try again (`src/components.rs:392-395`).
    Change: add the SENDING state keyed on `PendingInteraction.id.is_none()`, use Discord's two failure texts
    ("The application did not respond" / server error / "This interaction failed"), and time out LOADING messages
    15 min after their snowflake.
15. **Picking inserts only the first required key.** Discord inserts pills for **all** required options at once, caret
    in the first. Waffle inserts one (`src/command_picker.rs:296-307`). Change: insert every required option's token,
    then put the caret in the first value.
16. **Tab/Shift-Tab.** Discord's Tab and Shift-Tab move between pills; past the last one Tab goes to the end, where the
    option-name list appears. Waffle's Tab leaves the value and opens the names list, and Shift-Tab does nothing
    (`src/command_picker.rs:341-354`, `:497-513`). Change: Tab → next option value (wrap to end), Shift-Tab → previous.
17. **Picker matching is narrower.** Discord scores 12 tiers (name prefix, subcommand word prefix, name contains,
    option-name prefix/contains, app name prefix/contains, description, app description, fuzzy). Waffle matches name
    prefix/contains and description only (`src/command_picker.rs:1204-1218`). Change: add the option-name, app-name,
    word-wise and fuzzy tiers in the same order.
18. **Picker grouping and order.** Discord's typed list is one flat list sorted by score, then frecency, then name,
    max 20. The bare `/` browser has sections ordered by app frecency then name, 10 per page, and a frecency section of
    the 5 top commands that also stay in their app section. Waffle always groups by app, orders apps by best match
    (`src/command_picker.rs:1161-1197`), removes recents from their app section, and uses local run counts
    (`src/recent_commands.rs`). Change: flat list when there's a query; duplicate recents in their app sections; for
    frecency, read Discord's `FrecencyUserSettings.application_command_frecency` (proto settings).
19. **Up/Down wrap.** Discord's browser clamps at the ends; Waffle wraps (`src/command_picker.rs:277`, `:491`).
    Inline-list wrap is unknown (not checked).
20. **Missing validation rules.** Discord checks `min_value`/`max_value`, `min_length`/`max_length` (UTF-16
    `.length`), safe-integer range and locale-aware numbers, and resolves typed names for user/role/channel options.
    Waffle parses none of those fields (`src/app_commands.rs:157-171`), accepts only digits or a mention chip for
    entity options (`src/command_line.rs:177-181`), and uses `str::parse` (`:170-171`). Change: parse the
    limits and `channel_types`; validate with Discord's messages (section 1). Resolve `@name`/`#name` text against the
    member, role and channel lists.
21. **Error copy differs.** Discord: "This option is required. Specify a value.", "Input a valid integer.", "Not a valid
    user.", etc. Waffle: "Fill in {name} to run this command", "Enter a whole number", "Pick someone from the list"
    (`src/command_line.rs:118-183`). Change only if parity is the goal.
22. **Error marking on Enter.** Discord marks **every** failing pill red, inserts all missing required pills, and puts
    the error in the header in place of the description. Waffle stops at the first problem and adds one missing key
    (`src/command_line.rs:124-139`, `src/command_picker.rs:696-712`). Waffle also outlines invalid values only when
    non-empty (`src/command_picker.rs:1013`). Change: collect every failure, insert every missing required option, and
    mark all of them. Waffle shakes the composer and Discord shakes the app window (200 ms, intensity 2); keep
    Waffle's choice.
23. **Autocomplete timing and states.** Discord debounces 500 ms with leading **and** trailing calls, caches answers per
    query (reset when other options change), shows 5 skeleton rows while loading, "Loading options failed" on failure,
    and "No options match your search" when empty. Waffle debounces 250 ms trailing only, with no cache, no loading
    rows and no failure or empty state (`src/command_picker.rs:29`, `:621-661`, `:880-886`). Change: match the timing,
    add a `(option, query, other-values) → choices` cache, and add the three states. Track the autocomplete nonce so
    INTERACTION_FAILURE for it sets the failure state.
24. **Single optional option shortcut.** Discord turns text after `/name ` into the only optional option. Waffle has no
    equivalent. Change: in `pick_command`/`on_command_text`, wrap trailing text in that option's key.
25. **Option-name list filter.** Discord filters by displayName `startsWith`, case-sensitive, with headers "Required
    options" / "Other options". Waffle uses case-insensitive contains (`src/command_picker.rs:177-182`) and one header,
    "Options for /x". Minor.
26. **`@everyone` for role/mentionable options.** Discord accepts it and sends the guild id; Waffle excludes it
    (`src/command_picker.rs:611`). Change: offer `@everyone` for ROLE/MENTIONABLE in guilds.
27. **Dropping the command.** Discord drops command mode as soon as the text no longer starts with `/displayName`,
    leaving plain text. Waffle reparses the plain text every keystroke (`src/command_line.rs:36-59`). Waffle already
    behaves the same in effect. No change.

### (C) Replies and components

28. **Other controls stay live during an interaction.** Discord disables every other interactive component on the
    message and allows one interaction per message. Waffle guards only the clicked control
    (`src/components.rs:92-110`, `:462-505`). Change: when any `InteractionKind::Component` for `message_id` is pending
    and not failed, disable the message's other buttons and selects, and refuse new ones.
29. **Component failure line.** Discord shows a red line with an icon: "This interaction failed" / "Slow down! You're
    clicking too fast." / the reason text. There is no retry, and it clears on channel switch. Waffle shows "This
    interaction failed. {app} didn't answer." with Try again (`src/components.rs:451-460`, `:398-419`). Waffle shows
    nothing under a failed section accessory (`:519-529`). Change: use Discord's text, drop or keep Try again (your
    call), add the line under sections, clear failed component interactions on channel switch.
30. **Multi-select send.** Discord has no Done button: unpicked options disable at max, and the send happens when the
    menu closes with changed picks. Waffle uses a Done entry with a min gate (`src/components.rs:212-223`, `:305-313`).
    Change: send on menu close when picks differ from what was last sent and `min_values` is met.
31. **Select defaults and filters.** Discord resolves `default_values` (user/role/channel/mentionable), filters
    channels by `channel_types`, and keeps `required`. Waffle parses none (`src/app_commands.rs:387-414`) and lists
    category-free channels of every type (`src/components.rs:269`). Change: parse them and apply them in `picked_labels`
    and `server_choices`.
32. **Premium button.** Discord uses brand colour + ShopIcon + "{sku} - {price}" and opens the SKU modal, or shows
    "Product Unavailable". Waffle renders a plain secondary button and ignores `sku_id` (`src/app_commands.rs:372-386`,
    `src/components.rs:587-594`). Change: parse `sku_id` and render at least brand tone + shop icon, disabled. Opening a
    purchase flow is out of scope.
33. **Link button safety.** Discord routes through the leaving-Discord confirm; Waffle opens the URL directly
    (`src/components.rs:476-478`). Change: reuse Waffle's link-confirm if it has one.
34. **Loading button look.** Discord hides the label and draws dots over the same-size button; Waffle passes
    `loading(Some(label))` (`src/components.rs:474`). Verify that it keeps the button's width.
35. **Container accent.** Discord draws a 4 px left stripe on a surface-high card with a subtle border, radius-sm,
    padding 16 and gap 8. Waffle tints the fill and border "never a stripe" (`src/components.rs:554-567`). That is a
    deliberate design divergence: keep it or align. Waffle's container gap is 10 and its padding 14×16, against
    Discord's 8 and 16.
36. **Thumbnail sizing and spoiler.** Discord keeps the aspect ratio (85–170 px) and honours `spoiler`. Waffle uses a
    fixed 72 px square crop and doesn't parse the thumbnail spoiler (`src/app_commands.rs:417`,
    `src/components.rs:531-533`). Change: parse `spoiler` and `media.width/height`, size between 85 and 170.
37. **Gallery sizes.** Discord uses each item's `media.width/height`; Waffle assumes 400×225 for every item
    (`src/components.rs:33`, `:536-541`). Change: parse the media dimensions.
38. **Section without text or accessory.** Discord drops it; Waffle renders it (`src/app_commands.rs:415`). Change:
    return `None`.
39. **File component.** Discord shows `size` and `spoiler`; Waffle ignores both (`src/app_commands.rs:432`). Change:
    parse them.
40. **Text inside containers.** Discord renders it at "reduced" size; Waffle uses normal size. Minor.
41. **Ephemerality reason.** Discord shows "Only you can see this message. {reason} • Dismiss" when
    `interaction_metadata.ephemerality_reason` is set; Waffle always shows the generic line (`src/components.rs:369-389`).
    Change: parse the reason and map the enum.
42. **"Used" header.** Discord's command is a chip (Apps icon) that opens the options popout, and context-menu
    commands get "› {target}". Waffle shows plain text (`src/components.rs:317-339`), and its
    `interaction_metadata.name` read is unverified (`src/app_commands.rs:275-277`). Change: optional; prefer the legacy
    `interaction.name` (which Discord's header uses) and fall back to metadata.
43. **Separator spacing.** Unknown in Discord: the CSS module wasn't found (search above). Waffle uses 2/8 px
    (`src/components.rs:550-552`).

### (D) Modal

44. **Missing warning banner.** Discord always shows "This form will be submitted to **{app}**. Do not share passwords
    or other sensitive information." Waffle shows "{app} is asking" (`src/app_modal.rs:260-277`). Change: add the
    banner.
45. **Submit error copy and placement.** Discord shows a red alert at the top: "Something went wrong. Try again."
    Waffle shows "{app} didn't accept this. Try again." above the footer (`src/app_modal.rs:196`). Change the copy and
    move it to the top.
46. **Field validation differs.** Discord: "This field is required." (no trimming: whitespace counts as filled) and
    "Must be between {min} and {max} in length.", measured in UTF-16 units, with native `maxLength` stopping typing
    past the max. Waffle trims, counts chars, lets you type past max and shows a counter (`src/app_modal.rs:310-322`,
    `:286-289`). Change: cap input at `max_length`; use Discord's messages; decide on trimming.
47. **`required` default.** Discord's client defaults to `false` when the field omits `required`; Waffle defaults to
    `true` (`src/app_commands.rs:490`). Unknown whether the gateway always sends it. Change: match Discord (`false`) or
    log modals that omit it.
48. **Non-text modal components.** Discord renders labels with descriptions, selects, radio/checkbox groups, file
    upload and text display; Waffle renders text inputs only (`src/app_commands.rs:480-495`). Change: see gap 7. Render
    at least LABEL description, TEXT_DISPLAY and string selects.
49. **Footer layout.** Discord's Cancel and Submit are full-width; Waffle's are right-aligned (`src/app_modal.rs:182-192`).
    Style choice.
50. **Click outside.** Waffle closes on an outside click only when nothing changed (`src/app_modal.rs:153-159`).
    Discord's behaviour is unknown (not checked).

### Searches that came up empty
- Desktop cache: `ls ~/Library/Application Support/discord` → `Operation not permitted` (TCC).
- Separator CSS: `grep -ohE '210716\(' assets/*.js` → no definition in any downloaded chunk.
- English literals ("is thinking", "did not respond", "Only you can see this") are not in code; they are intl hashes,
  resolved through `en/messages.json` (built from the 95 `"en-US":()=>n.e(…)` chunks).
