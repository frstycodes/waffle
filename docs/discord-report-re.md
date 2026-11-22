# Discord "Report" flow (In-App Reporting, IAR) — reverse-engineered

Source: static read of the public web client, fetched unauthenticated on 2026-11-22 (`curl https://discord.com/app`, its `<script>` assets, then every lazy chunk from the rspack chunk map in `web.355a76e660bee7a5.js`, for 6,600 chunks in total). The desktop cache under `~/Library/Application Support/discord/` returned `Operation not permitted` (macOS privacy protection), so it was not read. Nothing was sent to `/reporting/*` and no authenticated call was made. The REA server only offered `open_binary` in this session, so the analysis used grep and Python slicing of the minified bundles, not `analyze_javascript_application`.

Key chunks (all under `/assets/`):

| Chunk | Contents |
|---|---|
| `web.355a76e660bee7a5.js` | `Endpoints` constants, `MessageTypes` sets, message-report gate (module 628691), toasts |
| `d65c4148a71425e4.js` | report types (17372, 2110), report openers (928658), menu fetch/submit API (369053) |
| `39d568e4337920df.js` | modal launcher `T()` (882276): fetches the menu, then lazy-loads the modal |
| `0113de44bd0f9760.js` | the modal itself (422495): node renderer, elements, buttons, navigation |
| `ebaf6c2acd1720c2.js`, `ceb73656db6e6af4.js`, `09ef3fcb822a8e89.js`, `dbee876872a682ca.js`, `e3024a9a937f92a7.js`, `fc9b5c3443825c91.js` | menu entry points |
| `c30b23bab1166ada.js` | en-US intl strings (hashed keys); `24149e8e29110292.js` holds the moderator ("Flag to Mods") strings |

Tags: **observed** = read in the bundle; **inferred** = my reading of it.

---

## 1. Menu fetch endpoints

**observed**, the `Endpoints` table in `web.js`:
```js
GET_REPORT_MENU:e=>`/reporting/menu/${e}`,GET_UNAUTHENTICATED_REPORT_MENU:e=>`/reporting/unauthenticated/menu/${e}`,
SUBMIT_REPORT_MENU:e=>`/reporting/${e}`,SUBMIT_UNAUTHENTICATED_REPORT_MENU:e=>`/reporting/unauthenticated/${e}`,
SEND_UNAUTHENTICATED_REPORT_PINCODE:e=>`/reporting/unauthenticated/${e}/code`, VERIFY_UNAUTHENTICATED_REPORT:e=>`/reporting/unauthenticated/${e}/verify`,
```
Other reporting endpoints (**observed**): `SUBMIT_MODERATOR_MESSAGE_REPORT:(c,m)=>/channels/${c}/${m}/mod-report`, `MODERATOR_REPORT_CLOSE /mod-report/{id}/close`, `REPORT_TO_MOD_REOPEN /mod-report/{id}/reopen`, `DSA_EXPERIMENT_UNAUTHENTICATED /reporting/unauthenticated/experiment`, `DSA_CAPABILITIES /reporting/unauthenticated/capabilities`, `SUBMIT_REPORT_SECOND_LOOK /reporting/review` (body `{token}`).

Fetch (**observed**, `369053.m9`):
```js
let n=g(e),i=await o.Bo.get({url:T.Rsh.GET_REPORT_MENU(n),query:t?.variant!=null?{variant:t.variant}:void 0,rejectWithError:!1});
return i.body??JSON.parse(i.text)
```
- Method `GET`, path `/api/v9/reporting/menu/{report_type}`. The `/api/vN` prefix is added by the HTTP client (**inferred**). The only query parameter is `variant`, and only when the caller passes one (**observed**). No language parameter is sent, so the language presumably comes from the request locale header (**inferred**).
- `g()` asserts that the name is in `REPORT_TYPES` (`Invalid report type …`). The unauthenticated flow (`IM`) uses `/reporting/unauthenticated/menu/{type}` with the URF types (**observed**).

Report type names (**observed**, module 17372 / 2110):

| Group | Names |
|---|---|
| `REPORT_TYPES` (authenticated) | `guild`, `guild_discovery`, `guild_directory_entry`, `message`, `stage_channel`, `guild_scheduled_event`, `first_dm`, `user`, `application`, `widget` |
| `REPORT_TO_MOD` | `report_to_mod_message` (menu from `/reporting/menu/report_to_mod_message`, submit to `/channels/{c}/{m}/mod-report`) |
| `UNAUTHENTICATED` ("URF") | `message_urf`, `user_urf`, `guild_urf`, `media_takedown` |
| `CONSOLE` | `playstation_console_voice`, `xbox_console_voice` |

Variants passed by the openers (**observed**): none by default; `"staff"` for the staff test menus; `"safety_alerts_v1"` (from the block-user flow, opener `jJ`); `"safety_alerts_headless_v1"` (`LF`) and `"_first_dm_ham_v1"` (`tJ`) are headless, meaning they fetch the menu and auto-submit without UI (see §3).

Report sub-types (`report_type` on nodes; **observed**, enum `TS`): `sub_spam`, `sub_unsolicited_porn`, `sub_sexually_degrading_content`, `sub_ncp`, `sub_loli`, `sub_csam`, `sub_general_harassment`, `sub_vulgar_language`, `sub_racist_or_discriminatory_language_or_imagery`, `sub_gore`, `sub_glorifying_violence`.

## 2. Menu response schema

The schema is reconstructed from the code that reads it. There is no type definition in the bundle.

Root (**observed**, modal `li`):
```js
let{nodes:A,root_node_id:f,success_node_id:N,fail_node_id:S}=l
...let{version:a,variant:_,language:r}=e   // in body builder h()
```

| Root field | Use |
|---|---|
| `nodes` | map `node_id → Node` (iterated with `for (l in A)`; the key equals `node.id`) — **observed** |
| `root_node_id` | first slide — **observed** |
| `success_node_id` | navigated to after a successful submit — **observed** |
| `fail_node_id` | rendered as a slide but never navigated to by client code — **observed**/**inferred** |
| `version`, `variant`, `language` | echoed back in the submit body (`language ?? "en"`) — **observed** |
| `postback_url` | **not present** in the bundle (searched, 0 hits) |

Node fields (**observed** unless noted):

| Field | Type / meaning |
|---|---|
| `id` | node id; also the `nodeRef` used in breadcrumbs. The numeric-vs-string type is not determined |
| `key` | string id such as `…_SUBMIT`; `key.endsWith("_SUBMIT")` orders slides; `onNavigate(key)` and jump-to-node use it |
| `header` | title (`heading-lg/semibold`) |
| `subheader` | markdown (links allowed), shown under the header |
| `info` | markdown, shown as an `info` notice |
| `button` | `{type: "next"\|"submit"\|"done"\|"cancel", target?}`; `target` is used with `next` |
| `elements` | `[{type, name, data, should_submit_data}]` |
| `children` | `[[label, node_id], …]` rendered as chevron rows; clicking one navigates |
| `report_type` | sub-type of the node (e.g. `sub_spam`); sent to analytics and used for settings upsells |
| `is_auto_submit` | when true, the node submits on mount (`o(ee(["",t.id]))`) |

```js
let ex=e=>{let{node:{children:t},onSelectChild:l,nodeMap:a}=e;... t.map(e=>{let[t,i]=e;return(0,n.jsx)(eh,{child:e,...
```

Element `type`s rendered by the modal (**observed**, `lt(node,"…")`/`ll(node,"…")`):

| type | Rendering / `data` fields read | Gated to report types |
|---|---|---|
| `checkbox` (multi-select) | `data: [[value, label, subtitle, defaultChecked], …]`; `name`; `should_submit_data` (required); `require_all_options` | any |
| `free_text` | `data.{title, subtitle, description, placeholder, rows, character_limit, pattern}` (regex check); textarea if `rows≠1` | URF only |
| `dropdown` | `data.{title, options:[{id?,value,label}]}` | URF only |
| `country_select` | `data.{title, options}`; labels localised via region names | any |
| `radio_group` | `data.{title, options:[{label,value}]}` | any |
| `content_url_input` | `data.{title, subtitle, placeholder, message_link_title, message_link_placeholder}`; a CDN URL requires a message link `…/channels/…/…/…` | URF only |
| `inline_notice` | `data.{body, notice_type, visible_when:{element_name?|element_type?, value}}` | any |
| `text` | `data.{header, body, is_localized}` | any |
| `text_line_resource` | `data.{title, body, sms, is_localized}` + Copy button | any |
| `external_link` | `data.{url, link_text, link_description, is_localized, is_header_hidden}`; "Learn More" header | any |
| `breadcrumbs` | renders the "Report Category" list from history | any |
| `success` | success illustration; switches the node to the success layout | any |
| `skip` | node auto-forwards to `button.target` when the button is `next` | any |
| `message_preview` | "Selected Message" | message, first_dm, report_to_mod_message |
| `user_preview` | "Selected User" | user |
| `guild_preview` | "Selected Server" | guild |
| `channel_preview` | "Selected Stage" | stage_channel |
| `guild_scheduled_event_preview` | "Selected Event" | guild_scheduled_event |
| `guild_directory_entry_preview` / `guild_discovery_preview` | "Selected Server" | resp. type |
| `app_preview` | "Selected App" | application |
| `widget_preview` | "Selected Widget" | widget |
| `ignore_users` | "Ignore {username}" / "Get space without letting them know" | message, first_dm, user |
| `block_users` | "Block {username}" / "Stop seeing their messages" | message, first_dm, user, report_to_mod_message, application with bot |
| `mute_users` | "Mute {username}"; shown only when `ignore_users` is absent | message, first_dm, user, report_to_mod_message |
| `delete_message` | "Delete message"; requires MANAGE_MESSAGES in a guild channel | message, report_to_mod_message |
| `leave_guild` | "Leave the server" with a confirm dialog | guild |
| `deauthorize_app` | "Deauthorize Application" + "Remove Application from Server" (MANAGE_GUILD) | application |
| `share_with_parents` | "Share report with {parents}"; only for teen accounts linked to Family Center | any |
| `settings_upsells` | "Recommended Settings": DM spam filter / graphic media / sexual media filter, chosen by `report_type` | message, report_to_mod_message |

Any of the action elements (`ignore_users`, `block_users`, `leave_guild`, `delete_message`, `deauthorize_app`, `share_with_parents`) makes the "More you can do" section appear (**observed**: `t7=["ignore_users","block_users","leave_guild","delete_message","deauthorize_app","share_with_parents"]`).

## 3. Submit endpoint

`POST /reporting/{report_type}`. For `report_to_mod_message` it is `POST /channels/{channel_id}/{message_id}/mod-report`. Unauthenticated reports go to `POST /reporting/unauthenticated/{type}` (**observed**).

Body builder `h(menu, report, history, emailToken)` (**observed**):
```js
l={version:a,variant:_,language:r??"en",breadcrumbs:n.map(e=>e.nodeRef),
 elements:n.reduce((e,t)=>{let{multiSelect:n,textInput:i}=t;return{...e,...null!=n&&{[n.name]:Object.keys(n.state)},
 ...Object.fromEntries(Object.entries(i??{}).map(e=>{let[t,{value:n}]=e;return[t,n]}))}},{})};
```
Every body carries the full set of id keys (unused ones are `undefined`, so JSON drops them): `channel_id, message_id, stage_instance_id, guild_id, guild_scheduled_event_id, user_id, email_token, application_id, entrypoint, widget_id`, plus `name` (**observed**).

| `name` | Ids filled |
|---|---|
| `message`, `first_dm` | `channel_id`, `message_id` |
| `guild`, `guild_discovery` | `guild_id` |
| `guild_directory_entry` | `channel_id` (directory channel), `guild_id` |
| `stage_channel` | `channel_id`, `guild_id`, `stage_instance_id` |
| `guild_scheduled_event` | `guild_id`, `guild_scheduled_event_id` |
| `user` | `user_id`, `guild_id` (contextual guild, omitted for DMs/`@me`) |
| `application` | `application_id`, `guild_id`, `channel_id`, `entrypoint` |
| `widget` | `user_id`, `widget_id` |
| `user_urf`/`message_urf`/`guild_urf`/`media_takedown` | the matching id + `email_token` |
| `report_to_mod_message` | `channel_id`, `message_id` (only `version, variant, language, breadcrumbs, elements` plus these) |

- `elements` is a flat object. A checkbox contributes `{[element.name]: [selected values…]}`; free-text, dropdown, radio, country and URL elements contribute `{[element.name]: "value"}`; a URL input also adds `{name}_message_link` (**observed**).
- Example shape for a message report (**inferred** from the builder; ids illustrative):
  ```json
  {"version":"…","variant":"…","language":"en","breadcrumbs":[3,28,30],"elements":{},
   "name":"message","channel_id":"…","message_id":"…"}
  ```
- Response: the modal reads `n?.body?.report_id` and stores it for analytics and the feedback survey (**observed**). Requests use `rejectWithError:!1` for authenticated reports, so a non-2xx response resolves. How the modal still shows an error is not traced (see Open questions). Error codes are mapped to copy: `UNKNOWN_TIDA_CONTENT`, `INVALID_SIGNATURE_MISMATCH`, `INVALID_FORM_BODY`, otherwise a generic message (**observed**).
- Headless submit (`gP`, used for `safety_alerts_headless_v1` and `_first_dm_ham_v1`): fetch the menu, then post with `breadcrumbs:[root_node_id]` and destination `success_node_id`, without showing UI (**observed**).
- The dev setting `iar_skip_api_report_submit` short-circuits submit to `Promise.resolve()` (**observed**).
- `report_to_mod_message` success shows the toast `REPORT_TO_MOD_SUCCESS` and dispatches `REPORT_TO_MOD_REPORT_MESSAGE_SUCCESS` (**observed**).

## 4. Navigation

History entry (**observed**): `{nodeRef: currentNode.id, destination: [label, nextNodeId], textInput?, multiSelect?: {name, state}}`.
```js
ee=e=>({nodeRef:t.id,destination:e,textInput:...?q:void 0,multiSelect:null!=y?{name:y.name,state:K}:void 0})
```
- **Child click**: pushes `{nodeRef: current, destination: [childLabel, childId]}` and moves to `childId`. If the target node has a `skip` element and a `next` button, the client follows `button.target` recursively. A navigation to a missing node only increments the metric `in_app_report_navigate_to_nonexistent_node` and does nothing else (**observed**).
- **`breadcrumbs`** in the body = `history.map(nodeRef)`, plus the submit node's own entry (`[...M, e]`). So it lists every node id visited in order, ending with the node that held the submit button (**observed**/**inferred**).
- **Back**: shown when `history.length>0` and the button type is not `done`. It pops the last entry, restores that entry's checkbox and text state, returns to its `nodeRef`, and calls `onNavigate("..")` (**observed**).
- Button handling (**observed**):
  ```js
  switch(e.type){case"done":case"cancel":u();break;case"next":er(["",e.target]);break;case"submit":H(!0),eu()}
  ```
  Labels: `next` = "Next"; `submit` = "Submit Report" (critical-primary; "Submit Flag" for report-to-mod); `cancel` = "Cancel" (secondary); `done` / default = "Done". A `cancel` button never renders as a primary action (`x=null!=l&&"cancel"!==l.type`, so the primary button is hidden for cancel). This looks like a minifier artifact; it could mean cancel renders only Back (**inferred**, low confidence). The primary button is disabled while the required-field validator `ks()` fails. Enter triggers the node button.
- **After submit**: the client navigates to `["", success_node_id]`. That node typically carries `success`, `breadcrumbs` and action elements (block, mute, ignore, delete, leave, deauthorize, settings upsells) with a `done` button (**inferred** from rendering rules; the node content comes from the server). On a submit node without `success`, the modal shows the legal line "By submitting this report you confirm that it is truthful and made in good faith…" (**observed**).
- Slide order: root, then intermediate nodes (a `next` target is moved after its source), then `_SUBMIT` and submit nodes, then success, then fail. All slides are mounted in a 440px slide container keyed by node id (**observed**).
- On close: analytics `IAR_MODAL_CLOSE {navigation_history, report_id…}`, `onClose`, then `IN_APP_REPORTS_SHOW_FEEDBACK`. That feedback survey has a 48h cooldown (`1728e5` ms) and a 50% chance (**observed**).

## 5. Entry points

| Surface | Menu item id / copy | Opener | Gating |
|---|---|---|---|
| Message context menu (`navId:"message"`) and hover bar "More" (`navId:"message-actions"`) | `report`: "Report Message", or "Report to Discord" with Clyde icon when `Qo(guildId)` (the report-to-mod guild feature) is on | `V2(msg,"web_message_context_menu")` | `ul(msg)`: type not in `NON_REPORTABLE` `{67 = FRIEND_REQUEST_ACCEPTED}`, author not the current user, author not `system`. Context menu also needs `ta` (`canReport`) — **observed** |
| Same menus | `report-to-mod`: "Flag to Mods" / "Flagged to Mods" (disabled once flagged), "new" badge | `dy(msg)` | `ne.KB(msg)`, the guild's moderator-reporting feature (**observed**; condition internals not traced) |
| Delete-message alert | checkbox "report" in delete confirm | `V2(msg,"message_delete_alert")` | moderator deleting a message (**observed**) |
| User context menu / profile | `report-user`: "Report User Profile" | `NW(user, guildId≠@me)` | experiment `2023-09_iar_user_reporting` enabled, not self, not `isNonUserBot()` (**observed**) |
| Server (guild) context menu | `report-guild`: "Report Server" | `b1(guild, location)` | experiment `2026-11-iar-guild-reporting-desktop` (default **off**) and current user not owner (`bM(guild,user)`) — **observed**/**inferred** for owner |
| Server directory listing | `report-server-listing`: "Report Server" | `sJ(entry)` | hidden for some condition `y` (not traced) |
| Stage channel menu | `report-stage`: "Report Stage" | `pb(channel)` | `isGuildStageVoice() && isLive` (**observed**) |
| Event context menu | `report-event`: "Report Event" | `uA(event)` | event exists (**observed**) |
| Apps (launcher, activity popout/call, app directory, authorized apps, bot profile) | `report-app`: "Report App" / "Report" | `r3({application, entrypoint})` | entrypoints `app_launcher`, `activity_ui_popout`, `activity_ui_channel_call`, `app_directory`, `authorized_apps_settings`, `user_profile_widget` (**observed**) |
| First-DM / message-request spam bar | "Report Spam" button | `b8(msg)` then close DM | **observed** |
| Block confirm ("Block & Report") / safety tools | "Report" | `jJ(msg)` variant `safety_alerts_v1` | **observed** |
| Staff only | `staff-test-message-report` "[STAFF] Test Message Report", `staff-test-guild-report`, `staff-test-report-user` | variant `staff` | `isStaff() && devSetting iar_testing` (**observed**) |

## 6. Local copy, flags, experiments

Client-side copy (en-US, **observed**): "Submit Report", "Next", "Back", "Cancel", "Done", "Report Category" (breadcrumbs header), "Selected Message/User/Server/Stage/Event/App/Widget", "More you can do", "Recommended Settings", "Go to Privacy & Safety Settings", "Learn More", "Invalid Input", and the errors "There was an error trying to submit this report. Please try again!", "We couldn't locate the content you are trying to report. Please review the details of your submission and re-submit.", "The specified content could not be found. Please check the URL and try again.", "Electronic signature must match your full legal name". The good-faith line includes a Community Guidelines link (https://discord.com/guidelines). Action labels: Block/Blocked, Mute/Muted, Ignore/Ignored, Delete/Deleted, Leave/Left Server. The modal has **no** client-side title and no "Thanks for reporting" text: headers, subheaders and the success text come from the server (**observed**: `header` comes from the node).

Flags (**observed**): dev settings `iar_testing`, `iar_skip_api_report_submit`, `iar_show_report_sub_type_labels` (shows `report_type` under each child row); experiments `2023-09_iar_user_reporting`, `2026-11-iar-guild-reporting-desktop`; DSA capabilities via `/reporting/unauthenticated/capabilities`. Analytics: `IAR_MODAL_OPEN`, `IAR_NAVIGATE`, `IAR_MODAL_CLOSE`, `IAR_BLOCK_USER_BUTTON_CLICKED`, etc.

## 7. Sample message menu

The menu is **entirely server-driven**: no node graph, category labels or success copy exist in the bundle (**observed**: searched every chunk). The following is an **inferred** illustration built only from the parser. Labels, ids and the version are placeholders, not Discord's real taxonomy:

```json
{
  "name": "message", "variant": "…", "version": "…", "language": "en",
  "root_node_id": 1, "success_node_id": 90, "fail_node_id": 91,
  "nodes": {
    "1":  {"id":1,"key":"START","header":"<server copy>","subheader":null,"info":null,
           "elements":[{"type":"message_preview","name":"message_preview","data":null,"should_submit_data":false}],
           "children":[["<category A>",2],["<category B>",3]],"button":null,"report_type":null},
    "2":  {"id":2,"key":"CATEGORY_A","header":"…","elements":[],
           "children":[["<sub-reason>",10]],"button":null,"report_type":null},
    "10": {"id":10,"key":"SPAM_SUBMIT","header":"…","report_type":"sub_spam",
           "elements":[{"type":"message_preview",…},{"type":"breadcrumbs",…}],
           "children":[],"button":{"type":"submit"}},
    "90": {"id":90,"key":"SUCCESS","header":"…","elements":[
           {"type":"success",…},{"type":"breadcrumbs",…},{"type":"block_users",…},
           {"type":"settings_upsells",…}],"children":[],"button":{"type":"done"}},
    "91": {"id":91,"key":"FAIL","header":"…","elements":[],"children":[],"button":{"type":"done"}}
  }
}
```
Submitting from node 10 after the path 1, 2, 10 yields `breadcrumbs: [1,2,10]`.

## Open questions / Not verified

- Whether node ids are integers or strings, and the real `version` and `variant` values. Both come only from the live response, which was not fetched.
- The real category tree, labels and success copy for any report type (server-driven).
- `fail_node_id` is rendered, but no client code navigates to it; the server may never point a child at it.
- Error UX: authenticated `zC` uses `rejectWithError:!1` while the modal relies on `.catch()`. It is unclear how a non-2xx response surfaces (the HTTP client may still throw on non-2xx).
- Internals of `ne.KB` (report-to-mod eligibility), `Qo` (guild feature that relabels to "Report to Discord"), directory-listing gate `y`, and the `canReport` (`ta`) prop in the message context menu.
- The desktop app may differ from the web build: the desktop cache was blocked by macOS privacy protection, so this is the web bundle as of 2026-11-22.
- `language` comes from the menu response; whether the server picks it from `Accept-Language` or `X-Discord-Locale` was not checked.
