# Discord user settings: Appearance, Accessibility, Chat, Keyboard shortcuts

Reverse-engineered from Discord's shipped web client JS. Static reading only; nothing was executed.

## Source and method

- **Electron cache: not read.** `~/Library/Application Support/discord/` returned `Operation not permitted` (macOS privacy protection), so the fallback below was used.
- **Web client, stable channel, build 678976** (`buildId e85e837c…`), fetched 2026-11-22 with no cookies or auth: `curl https://discord.com/app`, the 366 initial `/assets/*.js` scripts, and then all 6,277 lazy chunks listed in the webpack chunk map. Lazy chunks are served as `/assets/<hash>.js`, with no chunk-id prefix.
- **Bundles cited below:**

| Short name | File | sha256 (prefix) | Contents |
|---|---|---|---|
| `web` | `web.355a76e660bee7a5.js` | `bf62e0e123c45de0` | stores, proto schema, keybind map, shortcut list, theme tiles |
| `settings` | `52b6e35ba37f71f0.js` | `aa3cb596ec22aa5c` | the new settings registry (every panel, category and row) |
| `en` | `c30b23bab1166ada.js` | `378649cb38b71b65` | en-US intl messages (hashed keys such as `Q6lKkg` → "Space Between Message Groups") |

- **How the code is built:** labels are hashed intl keys, resolved through the `en` chunk. Settings are declared as `(0,h.<kind>)(f.X.<SETTING_ID>,{useTitle,useSubtitle,useOptions,useValue,setValue,…})`. Kinds: `t_` panel, `zZ` category, `FW` fieldset, `bd` accordion, `zD` toggle, `Qx` dropdown/select, `Hn` radio, `sN` slider, `E2` custom component, `AK` navigator link, `gN` related-settings block, `i4` sidebar item, `WI` sidebar section. The kind names come from usage; they are not documented anywhere.
- **Storage kinds:**
  - **proto**: synced `PreloadedUserSettings` field, read through the accessor module `253932` in `web` (`(0,_.c$)("textAndImages","…",default)`).
  - **proto+device**: a proto field wrapped in `_.Mt(…,"text",key)`. When the user turns off sync for the `text` group, the value lives in the local `SelectivelySyncedUserSettingsStore` instead.
  - **local:A11y**: `AccessibilityStore` (`extends DeviceSettingsStore`, `persistKey="AccessibilityStore"`), per device.
  - **local:Theme**: `ThemeStore` (`PersistedStore`).
  - **local:TTS**: `TTSStore` (`DeviceSettingsStore`).
  - Not verified: which browser storage backend (localStorage vs IndexedDB) the persisted stores use.
- **The `observed`/`inferred` column:** `observed` means the label, values, default and storage all come straight from code. `inferred` means at least one link was reasoned from names or structure.
- **REA:** this session exposed only `mcp__rea__open_binary` (it confirmed the `settings` chunk sha256 `aa3cb596…`). `analyze_javascript_application` was not advertised, so the analysis used grep plus small Python extractors.

## Page structure (sidebar "Experience" section, in order)

Observed in `settings`: `APP_SECTION` → `useTitle "Experience"`, `buildLayout:[oh,nQ,tt,iW,sG,n1]`.

1. **Voice & Video**
2. **Appearance.** Categories in order: **Theme**, **App Icon**, **Messages** (search term "Chat"), **Chat Box** (search term "Chat Input"), **Search**, **Streamer Mode**, **Advanced**.
3. **Accessibility.** Categories in order: **Text Readability**, **Visual Density**, **Color & Contrast**, **Reduced Motion**, **Audio & Screen Reader**. A sticky live **Preview** panel shows example messages, avatars and a button.
4. **Party Mode**
5. **System.** Categories in order: **General**, **Custom Keybinds**, **Default Keybinds**, **System Helper**, **Game Mode**, **Advanced**.
6. **Language & Time**

There is **no separate "Text & Images"/"Chat" panel or "Keybinds" panel** in this build. The full panel enum (`*_PANEL="…"` in `web`) has no chat, text or keybinds panel. Chat rows live in Appearance → Messages and Chat Box. Keybinds live in System. The `/tts` toggles live in Notifications → advanced accordion.

```
settings: nZ=(0,h.t_)(f.X.APPEARANCE_PANEL,{…useTitle:()=>…"Appearance",buildLayout:()=>[nK,t8,nh,tF,nT,nM,tg]}
settings: te=(0,h.t_)(f.X.ACCESSIBILITY_PANEL,{useTitle:()=>…"Accessibility",buildLayout:()=>[eQ,e9,ej,eH,ea],…
settings: sL=(0,h.t_)(f.X.SYSTEM_PANEL,{useTitle:()=>…"System",buildLayout:()=>[sj,sO,sD,sP,sR,sv]}
```

---

## 1. Appearance

### Theme category

Rows in order: theme fieldset (`APPEARANCE_THEME_FIELDSET`: Same as Device Theme → dual Light/Dark selector → Default Themes tiles → Dark sidebar → custom-theme upsell → limited-time theme → Nitro client themes), then **Sync theme across my devices**, then **Apply theme to other users' profiles**, then a link to Accessibility ("Change saturation of colors, enable high-contrast mode, and more"). The header button is "Preview Theme".

**Theme values and mapping (observed).** The client theme enum is `{ASH:"dark", LIGHT:"light", ONYX:"midnight", DARK:"darker"}`.

- The label **"Ash" is the old classic dark** (client value `"dark"`, proto `DARK=1`).
- The label **"Dark" is the newer, darker value** `"darker"` (proto `DARKER=3`).
- **"Onyx"** is `"midnight"` (proto `MIDNIGHT=4`). The legacy `"amoled"` value is migrated to Onyx.

Proto enum: `Theme {UNSET=0, DARK=1, LIGHT=2, DARKER=3, MIDNIGHT=4}`. Proto → client map: `UNSET→"darker"(Dark), DARK→Ash, DARKER→Dark, LIGHT→Light, MIDNIGHT→Onyx`. **The default with no setting is "Dark" (`darker`).** The per-OS-theme default preferences are `{dark:"darker", light:"light", unknown:"darker"}`.

Tile order: Light, Ash, Dark, Onyx, then **Sync with computer** last (first only when `systemSelectorFirst`).

| Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|
| Theme tiles (Default Themes) | heading "Default Themes" | Light / Ash / Dark / Onyx / Sync with computer | Dark (`darker`) | proto `appearance.theme` (field 1, enum `Theme`); sync tile = local `useSystemTheme` (`Q_.ON`) | `web`: `case g.NJ8.ASH:return …"Ash";case g.NJ8.DARK:return …"Dark";case g.NJ8.ONYX:…"Onyx";case"system":…"Sync with computer"` / `let c={[a.Sx.UNSET]:s.NJ8.DARK,[a.Sx.DARK]:s.NJ8.ASH,[a.Sx.DARKER]:s.NJ8.DARK,…[a.Sx.MIDNIGHT]:s.NJ8.ONYX}` | observed |
| Same as Device Theme | "Match your device's light or dark mode." | toggle | off (Not verified: initial value) | local ThemeStore (`isSameAsDeviceThemeEnabled`) | `settings`: `APPEARANCE_SAME_AS_DEVICE_THEME,{useTitle:…"Same as Device Theme"` | observed (default inferred) |
| Light Mode / Dark Mode selector | (none) | one theme each for system light and system dark | light→Light, dark→Dark | local ThemeStore `preferences` | `settings`: `APPEARANCE_DUAL_THEME_SELECTOR,{useSearchTerms:…"Light Mode",…"Dark Mode"` | inferred (shown when Same as Device is on) |
| Dark sidebar | (none) | toggle; shown only for the Light theme without a gradient or custom theme | false | proto `appearance.dark_sidebar` (field 15) | `web`: `(0,_.c$)("appearance","darkSidebar",e=>e??!1` | observed |
| Sync theme across my devices | (none); disabled while Same as Device is on | toggle | true | local `SelectivelySyncedUserSettingsStore.shouldSync("appearance")` | `settings`: `!1!==nB.A.shouldSync("appearance")` | observed |
| Apply theme to other users' profiles | (none) | toggle | false | local:A11y `syncProfileThemeWithUserTheme` | `web`: `syncProfileThemeWithUserTheme:!1` | observed |

### Messages category (Appearance → Messages; this is the old "Text & Images" content)

Order: `[no, nu, nc, nA, nE, ng, ns, t9]`.

| # | Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|---|
| 1 | Fieldset **"Show images, videos, and lolcats..."** → **When posted as links to chat** | (none) | toggle | true | proto+device `text_and_images.inline_embed_media` (field 10) | `settings`: `CHAT_INLINE_MEDIA_LINKS,{useTitle:…"When posted as links to chat"),useValue:U.hD.useSetting` / `web`: `"textAndImages","inlineEmbedMedia",e=>e?.value??!0` | observed |
| 1b | → **When uploaded directly to Discord** | (none) | toggle | true | proto+device `inline_attachment_media` (field 9) | `web`: `"textAndImages","inlineAttachmentMedia",e=>e?.value??!0` | observed |
| 2 | Show embeds and link previews | (none) | toggle | true | proto+device `render_embeds` (12) | `web`: `"renderEmbeds",e=>e?.value??!0` | observed |
| 3 | Show emoji reactions | (none) | toggle | true | proto+device `render_reactions` (13) | `web`: `"renderReactions",e=>e?.value??!0` | observed |
| 4 | Show spoiler content (radio) | (none) | On click (`ON_CLICK`) / Always (`ALWAYS`) / On servers I moderate (`IF_MODERATOR`) | ON_CLICK | proto `render_spoilers` (4, StringValue) | `settings`: `{value:_.P6Q.ON_CLICK,…"On click"},{value:_.P6Q.ALWAYS,…"Always"},{value:_.P6Q.IF_MODERATOR,…"On servers I moderate"}` | observed |
| 5 | Open threads in split view | (none) | toggle | true | proto `use_thread_sidebar` (3) | `web`: `"useThreadSidebar",e=>e?.value??!0` | observed |
| 6 | **Show user avatars** (the old "Show avatars in compact mode") | when Cozy: "To hide avatars, go to **Accessibility** and select **Compact** under **Chat Message Display**." Disabled unless Compact. | toggle; shows true when not compact | false | proto `display_compact_avatars` (field 41) | `settings`: `useDisabled:()=>!U.hH.useSetting(),useValue:()=>{let e=U.aM.useSetting();return!U.hH.useSetting()\|\|e}` | observed |
| 7 | Favorites toggle (`CHAT_FAVORITES_TOGGLE`, badge "beta") | label/description in a separate message module (`ni.default.OT1NK5`) | toggle | Not verified | Not verified | `settings`: `ns=(0,h.E2)(f.X.CHAT_FAVORITES_TOGGLE,{…badge:"beta"` | inferred |
| 8 | Related link → Accessibility | "Adjust the visual density of chat messages and UI" | link | n/a | n/a | `settings`: `APPEARANCE_CHAT_ACCESSIBLITY_NAVIGATOR` | observed |

Also observed: **Show image descriptions** — caption "Image descriptions are used to describe images for screen readers.", default false, proto `view_image_descriptions` (7). It sits under Accessibility → Audio & Screen Reader (see §2).

### Chat Box category (Appearance → Chat Box)

Order: `[tV, tM, tk, tU, tN, tL]`.

| # | Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|---|
| 1 | Preview emoji, mentions, and markdown syntax as you type | (none) | toggle | true | proto `use_rich_chat_input` (2) | `web`: `"useRichChatInput",e=>e?.value??!0` | observed |
| 2 | Automatically convert emoticons in your messages to emoji | "For example, when you type :) Discord will convert it to 🙂" | toggle | true | proto `convert_emoticons` (21) | `web`: `"convertEmoticons",e=>e?.value??!0` | observed |
| 3 | Show stickers in autocomplete results | (none) | toggle | **false** | proto `include_stickers_in_autocomplete` (28). **Not** `expression_suggestions_enabled` (22), which has no UI in this build and only appears in a migration from `ExpressionSuggestionsPersistedStore`. | `web`: `"includeStickersInAutocomplete",e=>e?.value??!1` | observed |
| 4 | Show games in autocomplete results | (none) | toggle | true | proto `include_game_mentions_in_autocomplete` (39) | `web`: `"includeGameMentionsInAutocomplete",e=>e?.value??!0` | observed |
| 5 | Show send message button (shown only when NOT in experiment `EnableSendButtonOutsideExperiment`) | (none) | toggle | false | local:A11y `submitButtonEnabled` | `settings`: `tC={useTitle:…"Show send message button",useValue:…O.Ay.isSubmitButtonEnabled` / `web`: `submitButtonEnabled:!1` | observed |
| 6 | Accordion "Show/Hide Advanced Chat Bar Settings" (experiment `ChatBarAdvancedAccordion`) contains: Show send message button; **Show apps button** (default true); fieldset "Expression picker" | (none) | (see below) | (see below) | local:A11y | `settings`: `CHAT_BAR_ADVANCED_ACCORDION,{…buildLayout:()=>[ty,tb,tR]}` | observed |
| 6a | Expression picker format (select) | option descriptions: Flexible "Show the emoji, GIF, and sticker buttons separately. Each one can be toggled individually."; Condensed "Combine the emoji, GIF, and sticker buttons into a single button in the chat input."; Hidden "Hide the emoji, GIF, and sticker buttons from the chat input entirely." | `flexible` / `condensed` / `hidden` | flexible | local:A11y `expressionPickerFormat` | `web`: `expressionPickerFormat:"flexible",condensePickerWhenNarrow:!0,emojiButtonEnabled:!0,gifButtonEnabled:!0,stickerButtonEnabled:!0` | observed |
| 6b | Condense expression picker when narrow; Show emoji button; Show GIF button; Show sticker button (Flexible format only) | condense: "Automatically combine the emoji, GIF, and sticker buttons when the chat input is too narrow to fit them individually." | toggles | all true | local:A11y | (same line as 6a) | observed |

### Advanced, Search and related rows (Appearance)

| Label | Values | Default | Storage | Status |
|---|---|---|---|---|
| Show Game Library (Advanced; shown only if a library app exists) | toggle | true (stores `!disable_games_tab`) | proto `game_library.disable_games_tab` | observed |
| Search → "By default, searching in DMs..." | Searches only in the selected DM (0) / Searches across all my DMs (1) | 0 | proto `text_and_images.is_cross_dm_search_enabled` (36) | observed |

### Display and density settings (owned by Accessibility → Visual Density, also linked from Appearance)

| Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|
| UI Density (search term "List Spacing") | "Adjust the space between server, channel, and member lists." | Compact (`COMPACT=1`) / Default (`DEFAULT=4`) / Spacious (`COZY=2`) | Default (UNSET maps to DEFAULT) | proto `appearance.ui_density` (12, enum `UIDensity {UNSET_UI_DENSITY=0, COMPACT=1, COZY=2, RESPONSIVE=3, DEFAULT=4}`) | `web`: `"appearance","uiDensity",e=>e===s.NS.UNSET_UI_DENSITY?s.NS.DEFAULT:e??s.NS.DEFAULT` | observed |
| Chat Message Display (search term "Message Display") | "Change the appearance of chat messages." | Default (cozy) / Compact | Default (cozy) | proto `text_and_images.message_display_compact` (17), bool | `settings`: `useOptions:()=>[{name:…"Default",value:e0},{name:…"Compact",value:e1}],useValue:()=>U.hH.useSetting()?e1:e0` | observed |
| Space Between Message Groups | "Adjust the spacing between message groups." | 0, 4, 8, 16, 24 px (snaps to these) | **16 in Cozy, 0 in Compact** (stored `null` = auto) | local:A11y `messageGroupSpacing` | `web`: `qh:[0,4,8,16,24]` / `get messageGroupSpacing(){return null!=m.messageGroupSpacing?m.messageGroupSpacing:16*!u.hH.getSetting()}` | observed |
| Zoom level (desktop app only: `usePredicate:()=>isPlatformEmbedded`) | "Adjust the size of the interface. You can also change this with {cmd\|ctrl} +/-." | 50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200 % | 100 | local:A11y `zoom` | `web`: `ZOOM_DEFAULT:100,ZOOM_MIN:50,ZOOM_MAX:200,ZOOM_SCALES:[50,67,75,80,90,100,110,125,150,175,200]` | observed |

---

## 2. Accessibility

Section and row order is exactly as listed.

### Text Readability

| Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|
| **Text size in chat** (the old "Chat font scaling") | "Adjust the size of the chat font." | 12, 14, 15, 16, 18, 20, 24 px | 16 | local:A11y `fontSize` (CSS class `font-size-N`) | `web`: `FONT_SIZE_DEFAULT:16,FONT_SIZE_MIN:12,FONT_SIZE_MAX:24,FONT_SIZES:[12,14,15,16,18,20,24]` | observed |
| Always underline links | "Make links stand out more." | toggle | false | local:A11y `alwaysShowLinkDecorations` | `web`: `alwaysShowLinkDecorations:!1` | observed |
| Display Name Styles | (formatted text with a modal link; message `aEax6P` not decoded) | toggle | true | local:A11y `displayNameStylesEnabled` | `web`: `displayNameStylesEnabled:!0` | observed (caption Not verified) |

### Visual Density

Rows in order: UI Density, Chat Message Display, Space Between Message Groups, Zoom level. These are documented in §1.

### Color & Contrast

Order: `[eI, ed, eh, eg, ev, eS, ex, eT, eA, eu]`.

| Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|
| Saturation | "Reduce the saturation of colors within the app, for those with color sensitivities. This does not affect the saturation of images, videos, role colors or other user content." | 0–1, step 0.05 (21 marks; only even percentages labelled: 0%, 10% … 100%) | 1 (100%) | local:A11y `saturation` | `web`: `SATURATION_INCREMENTS:[0,.05,.1,…,.95,1]` / `saturation:1` | observed |
| **Apply saturation setting to custom colors** (the role-colours variant) | "Apply the above setting to custom color choices, like role colors." | toggle | false | local:A11y `desaturateUserColors` | `web`: `desaturateUserColors:!1` | observed |
| Enable High Contrast Mode (NEW badge) | "Enhance visibility with bold colors and sharp contrast." | toggle; sets `contrastMode` to `"high"` or `"default"` | off (`"default"`) | local:A11y `contrastMode` | `web`: `contrastMode:"default"` | observed |
| Enable custom cursor in shop (experiment `EnableCustomCursorSetting`) | "Use a custom cursor design when browsing the shop. Can be turned off if the cursor feels distracting or interferes with navigation." | toggle | true | local:A11y `enableCustomCursor` | `web`: `enableCustomCursor:!0` | observed |
| Sync contrast settings (only when the OS supports forced colors) | "This allows Discord to use your computer's contrast theme. Learn more about contrast settings." | toggle | true | local:A11y `syncForcedColors` | `web`: `syncForcedColors:!0` | observed |
| High Dynamic Range (BETA, experiment `HDRDynamicRangeSetting`) | "Control how HDR images are displayed on HDR-capable monitors." | Full Dynamic Range (`no-limit`) / Standard Range (`standard`) | Not verified | another store (`ep.Ay.hdrDynamicRange`) | `settings`: `value:"no-limit"},{…"Standard Range"…,value:"standard"}` | observed (default Not verified) |
| Role Colors (radio) | "Choose how you would like role colors to be displayed." | In names (`username`) / Next to names (`dot`) / Don't show role colors (`hidden`) | `username` | local:A11y `roleStyle` | `settings`: `{id:"username",label:…"In names"},{id:"dot",label:…"Next to names"},{id:"hidden",…}` / `web`: `roleStyle:"username"` | observed |
| Official Messages (radio) | "Choose how official developer messages look on verified servers that support it." | Default / No text color / No background color / No styling (`default`, `no_text_color`, `no_gradient`, `hidden`) | `default` | local:A11y `officialMessageStyle` | `web`: `officialMessageStyle:"default"` | observed |
| Show on/off indicators | "On forms, menus and settings, on/off toggles will have icons." | toggle | false | local:A11y `switchIconsEnabled` | `web`: `switchIconsEnabled:!1` | observed |
| Related link → Appearance | "Change your app theme" | link | n/a | n/a | `settings`: `ACCESSIBILITY_TO_DISPLAY_NAVIGATOR` | observed |

### Reduced Motion

Order: `[ew, ek, eB, eM, eG, eV]`.

| Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|
| Enable Reduced Motion | "Reduce the amount and intensity of animations, hover effects, and other moving effects. Learn more." | toggle; sets `"reduce"` or `"no-preference"` | follows OS (`"auto"`) | local:A11y `prefersReducedMotion` | `web`: `prefersReducedMotion:"auto"` / `settings`: `setValue:e=>(0,ec.qz)(e?"reduce":"no-preference")` | observed |
| Sync with computer setting | (none) | toggle; on = `"auto"`, off = copies the current OS value | on | local:A11y (same key) | `settings`: `useValue:…"auto"===O.Ay.rawPrefersReducedMotion` | observed |
| Minimum notification duration | "Keep pop-up notifications on screen for at least this long. Notifications that already stay longer are unaffected." | 3–20 s, step 1 | 3 s | local:A11y `minToastDurationMs` | `web`: `TOAST_DURATION_DEFAULT_MS:3e3,TOAST_DURATION_MIN_SECONDS:3,TOAST_DURATION_MAX_SECONDS:20` | observed |
| **Play GIFs when Discord is focused** (GIF autoplay) | dynamic: "This setting has been automatically disabled because of your **Reduced Motion** setting." when overridden | toggle | true | proto+device `gif_auto_play` (11), with a motion override | `web`: `(0,_.Tg)((0,_.Mt)((0,_.c$)("textAndImages","gifAutoPlay",e=>e?.value??!0` | observed |
| Play animated emoji | same override caption | toggle | true | proto+device `animate_emoji` (14) | `web`: `"textAndImages","animateEmoji",e=>e?.value??!0` | observed |
| Play sticker animations (select) | override caption: "This controls when stickers animate. This is currently overridden by your reduced motion preferences." | Always animate (0) / Animate on interaction (1, description "On the desktop client, stickers will animate on hover or focus. On mobile clients, stickers will animate on long-press.") / Never animate (2) | Always animate (0) | proto+device `animate_stickers` (15, UInt32Value) | `web`: `"animateStickers",e=>e?.value??I.BJ.ALWAYS_ANIMATE` / `ALWAYS_ANIMATE=0,…ANIMATE_ON_INTERACTION=1,…NEVER_ANIMATE=2` | observed |

### Audio & Screen Reader

Order: `[er, z, X]`.

| Label | Caption | Values | Default | Storage | Evidence | Status |
|---|---|---|---|---|---|---|
| Text-to-Speech rate (only when TTS is supported) | "Control the speed at which text is read aloud when using Speak Message or other text-to-speech features." Has a **Preview** button (Play/Pause) that speaks "This is what text-to-speech sounds like at the current speed." | 0–10, step 0.25 (marks labelled "Slower" at 0, "x1.0" at 1, "Faster" at 10); shown as `x1.00` | 1 | local:TTS `speechRate` | `web`: `P:[0,.25,.5,…,9.75,10]` / `let a={speechRate:1,currentMessage:null}` | observed |
| Show image descriptions | "Image descriptions are used to describe images for screen readers." | toggle | false | proto `view_image_descriptions` (7) | `web`: `"viewImageDescriptions",e=>e?.value??!1` | observed |
| Use the legacy chat input | "Disables most chat features but may work better with screen readers." | toggle | false | proto `use_legacy_chat_input` (24) | `web`: `"useLegacyChatInput",e=>e?.value??!1` | observed |

### Located elsewhere (asked about)

| Label | Where | Values | Default | Storage | Status |
|---|---|---|---|---|---|
| Allow playback and usage of /tts command | Notifications → advanced accordion | toggle | true | proto `enable_tts_command` (16) | observed |
| Speak all messages out loud | Notifications → advanced accordion | For all channels / Only for currently selected channel / Never | Not verified | `getTTSType()` store | observed (default Not verified) |
| Sync profile themes | Appearance → Theme ("Apply theme to other users' profiles") | see §1 | | | observed |
| Show send message button | Appearance → Chat Box | see §1 | | | observed |
| Tap to react | not found in the web bundles (mobile-only) | | | | Not verified |

---

## 3. "Chat" settings

There is no Chat or Text & Images page in this build; every row lives on Appearance (§1) or Accessibility (§2). `CONTENT_FILTERS_APPEARANCE_NAVIGATOR` ("Show/hide media in chat, spoiler content") links from Content & Social to `APPEARANCE_MESSAGES_CATEGORY`.

**Synced fields with no row on these pages:**

- `emoji_picker_collapsed_sections` (5), `sticker_picker_collapsed_sections` (6), `soundboard_picker_collapsed_sections` (25): repeated strings holding picker state.
- `show_command_suggestions` (8): default true.
- `expression_suggestions_enabled` (22): migration only.
- `inline_emoji_suggestions_enabled` (40): default true.
- `include_soundmoji_in_autocomplete` (31): default true.
- `show_mention_suggestions` (34).
- `explicit_content_filter` (19): default `NON_FRIENDS`.
- `explicit_content_settings` (29), `gore_content_settings` (32), `self_harm_content_settings` (35), `keyword_filter_settings` (30), `view_nsfw_guilds` (20), `view_nsfw_commands` (23), `dm_spam_filter`/`_v2` (26/27): these belong to Content & Social / Privacy.
- `default_reaction_emoji` (33), `diversity_surrogate` (1).

```
web: {no:2,name:"use_rich_chat_input",…},{no:3,name:"use_thread_sidebar",…},{no:4,name:"render_spoilers",kind:"message",T:()=>P.hU}
web: {no:9,name:"inline_attachment_media",…},{no:10,name:"inline_embed_media",…},{no:15,name:"animate_stickers",kind:"message",T:()=>P.ZQ}
web: {no:22,name:"expression_suggestions_enabled",…},{no:28,name:"include_stickers_in_autocomplete",…},{no:41,name:"display_compact_avatars",…}
```

---

## 4. Keyboard shortcuts

### Platforms and how combos are written

- `mod` = **cmd** on mac and **ctrl** elsewhere. `alt` is shown as **opt** on mac. `return` is shown as **enter** off mac.
- "mac" means the desktop app on darwin, or any client whose user agent contains `Macintosh`:

  ```
  web: let r=(0,i.cX)()||(0,i.Sn)()?"cmd":"ctrl",a=(0,i.cX)()||(0,i.Sn)()?"opt":"alt",s=…?"return":"enter";n.d(t,{},{Mu:r,Pe:a,wY:s})
  ```

- Combo strings use the mousetrap/combokeys format (`mod+shift+n`, `plus`, `pageup`, `esc`, `f1`).

### The dialog: Keyboard Combos (Ctrl/Cmd+/)

- The modal's aria-label is "Keybinds". The header reads "Keyboard Combos" with a `mod+/` key badge, and the subtitle is "Master these to be the greatest Discord Discord Revolution player".
- The toggle bind is `TOGGLE_HOTKEYS: ["mod+/","mod+shift+/"]`.
- Groups render in this order: **Messages** ("These shortcuts work when focused on a message."), **Navigation**, **Voice and Video**, **Chat**, **Miscellaneous**. The DND group is defined but **not shown in the modal**.
- Each row shows only the **first** bind of each action (`ep()` returns `t.binds[0]`).
- System → **Default Keybinds** shows the same list in the order Messages, Navigation, **Drag and Drop**, Chat, Voice and Video, Miscellaneous. It drops "Raging Demon" and applies predicates. It starts with a row "Show Keyboard Shortcuts List" = `mod+/`.

```
web: let B=[C.Q_.MESSAGE,C.Q_.NAVIGATION,C.Q_.VOICE_AND_VIDEO,C.Q_.CHAT,C.Q_.MISCELLANEOUS];
settings: function l4(e){…return t([lK.Q_.MESSAGE,lK.Q_.NAVIGATION,lK.Q_.DND,lK.Q_.CHAT,lK.Q_.VOICE_AND_VIDEO,lK.Q_.MISCELLANEOUS])}
web: function ep(){…return t.map(e=>{let t=em[e];…return t.binds[0]})}
```

### Shortcut table

All rows below are observed in `web` (module `426819` holds the list `eg()` and the map `em`; bind objects come from the cited modules). In the "Shown in dialog" column, mac and win/linux use the same combo unless both are given; for mac, read `mod` as cmd and `alt` as opt.

| Group | Label (exact) | Action ID | All binds (combo strings) | Shown in dialog: mac / win+linux |
|---|---|---|---|---|
| Navigation | Navigate between servers | SERVER_PREV / SERVER_NEXT | prev `mod+alt+up`, `mod+shift+[`, `ctrl+shift+tab`; next `mod+alt+down`, `mod+shift+]`, `ctrl+tab` | cmd+opt+↑ / cmd+opt+↓; ctrl+alt+↑ / ctrl+alt+↓ |
| Navigation | Navigate between channels | CHANNEL_PREV / CHANNEL_NEXT | `alt+up` / `alt+down` | opt+↑/↓; alt+↑/↓ |
| Navigation | Navigate forward and backward in page history | NAVIGATE_BACK / NAVIGATE_FORWARD | mac `mod+[` / `mod+]`; else `alt+left` / `alt+right` | cmd+[ / cmd+]; alt+← / alt+→ |
| Navigation | Navigate between unread channels | UNREAD_PREV / UNREAD_NEXT | `alt+shift+up` / `alt+shift+down` | |
| Navigation | Navigate between unread channels with mentions | MENTION_CHANNEL_PREV / NEXT | `mod+shift+alt+up` / `mod+shift+alt+down` | |
| Navigation | Navigate to current call | JUMP_TO_CURRENT_CALL | `mod+shift+alt+v` | |
| Navigation | Toggle between last server and DMs | TOGGLE_PREVIOUS_GUILD | `mod+alt+right` | |
| Navigation | Toggle QuickSwitcher | QUICKSWITCHER_SHOW | `mod+k` | |
| Navigation | Create or join a server | CREATE_GUILD | `mod+shift+n` | |
| Drag and Drop (settings page only) | Start Drag and Drop / Move / Drop item / Cancel | (literal) | `mod+d` / `up`,`down` / `spacebar`,`enter` / `esc` | n/a ("These shortcuts work when focused on a draggable item") |
| Chat | Mark server read | MARK_SERVER_READ | `shift+esc` | |
| Chat | Mark channel as read | MARK_CHANNEL_READ | `esc`, `shift+pagedown` | esc |
| Chat | Create a private group | CREATE_DM_GROUP | `mod+shift+t` | |
| Chat | Create a thread | CREATE_THREAD | `mod+shift+alt+n` | |
| Chat | Toggle pins popout | TOGGLE_CHANNEL_PINS | `mod+p` | |
| Chat | Toggle inbox popout | TOGGLE_INBOX | `mod+i` | |
| Chat | Mark top inbox channel read | MARK_TOP_INBOX_CHANNEL_READ | `mod+shift+e` | |
| Chat | Toggle channel member list or voice text chat | TOGGLE_USERS | `mod+u` | |
| Chat | Toggle emoji picker | SEARCH_EMOJIS | `mod+e` | |
| Chat | Toggle GIF picker | SEARCH_GIFS | `mod+g` | |
| Chat | Toggle sticker picker | SEARCH_STICKERS | `mod+s` | |
| Chat | Scroll chat up or down | SCROLL_UP / SCROLL_DOWN | `pageup` / `pagedown` | |
| Chat | Jump to oldest unread message | JUMP_TO_FIRST_UNREAD | `shift+pageup` | |
| Chat | Focus text area | TEXTAREA_FOCUS | `any-character` (typing anywhere focuses the input) | |
| Chat | Record a voice message (only with the voice-message config enabled) | RECORD_VOICE_MESSAGE | mac `ctrl+shift+d`; else `alt+r` | |
| Chat | Upload a file | UPLOAD_FILE | `mod+shift+u` | |
| Chat | Copy channel link | COPY_CHANNEL_LINK | `mod+shift+l` | |
| Voice and Video | Toggle mute | TOGGLE_MUTE | `mod+shift+m` | |
| Voice and Video | Toggle deafen | TOGGLE_DEAFEN | `mod+shift+d` | |
| Voice and Video | Answer incoming call | CALL_ACCEPT | `mod+return` | cmd+return / ctrl+enter |
| Voice and Video | Decline incoming call | **reuses MARK_CHANNEL_READ** (no separate decline action) | `esc` | esc |
| Voice and Video | Start call in private message or group | CALL_START | `ctrl+'`, `ctrl+shift+'` (literal ctrl on mac too) | ctrl+' |
| Voice and Video | Toggle soundboard | SEARCH_SOUNDBOARD | `mod+shift+b` | |
| Miscellaneous | Get help | TOGGLE_HELP | `mod+shift+h`, `f1` | mod+shift+h |
| Miscellaneous | Search | FOCUS_SEARCH | `mod+f`, `mod+shift+f` (shift = search without prefill) | mod+f |
| Miscellaneous | Open context menu | OPEN_CONTEXT_MENU | `shift+f10` | |
| Miscellaneous | Raging Demon (Easter egg; modal only) | (literal) | `h+h+right+n+k` | |
| Miscellaneous | Listen to Lofi Beats with Wumpus | VIBE_WITH_WUMPUS | `mod+alt+shift+w` | |
| Messages (focused message) | Edit Message / Delete Message / Pin Message / Add Reaction / Reply / Forward Message / Speak Message / Copy Text / Mark Unread / Focus text area | (literal) | `e` / `backspace` / `p` / `plus` / `r` / `f` / `s` / `mod+c` / `alt+enter` / `escape` | |

Evidence for the table:

```
web: let em={[N.IWg.SERVER_NEXT]:z.yx,[N.IWg.SERVER_PREV]:z.yv,[N.IWg.CHANNEL_NEXT]:_.kF,…[N.IWg.QUICKSWITCHER_SHOW]:F.R,…
web: {description:…"Decline incoming call"),binds:ep(N.IWg.MARK_CHANNEL_READ),group:"VOICE_AND_VIDEO"}
web: L={binds:(0,N.cX)()||(0,N.Sn)()?["mod+["]:["alt+left"],…},y={binds:…?["mod+]"]:["alt+right"],…}
```

**Binds that exist but are not listed in the dialog:**

| Action | Binds | Notes |
|---|---|---|
| JUMP_TO_GUILD | `mod+1` … `mod+9` | |
| TOGGLE_CATEGORY_COLLAPSED | `mod+shift+a` | |
| RETURN_TO_AUDIO_CHANNEL | `mod+alt+left` | |
| ZOOM_IN / ZOOM_OUT / ZOOM_RESET | `mod+plus` / `mod+minus` / `mod+0` | steps through ZOOM_SCALES; reset = 100 |
| OPEN_APP_DIRECTORY | `mod+ctrl+a` | |
| BROWSER_DEVTOOLS | `mod+alt+i` | blocked on discord.com web |
| SUBMIT | `return` | modal submit |
| POP_LAYER | `esc` | |
| Channel tabs: OPEN_CHANNEL_TAB / CLOSE_CHANNEL_TAB / TOGGLE_CHANNEL_TAB_PIN | `mod+t` / `mod+w` / `mod+shift+p` | tabs experiment |

**Checklist from the request:**

| Exists in this build | Not found as a default in-app bind |
|---|---|
| settings: no bind | **Open user settings** (no `mod+,` bind found) |
| shortcuts dialog `mod+/` | **Bold / italic / underline**: no keybind entries; markdown shortcuts in the editor not checked (Not verified) |
| create/join `mod+shift+n` | **Mentions** (separate from inbox) |
| back/forward | |
| search `mod+f` | |
| emoji/GIF/sticker `mod+e`/`mod+g`/`mod+s` | |
| switch servers/channels | |
| unread and mention navigation | |
| pins `mod+p` | |
| inbox `mod+i` | |
| member list `mod+u` | |
| upload `mod+shift+u` | |
| help `mod+shift+h`/`f1` | |
| mute/deafen | |
| answer `mod+return` / decline `esc` | |
| start call `ctrl+'` | |
| jump to call `mod+shift+alt+v` | |
| soundboard `mod+shift+b` | |
| mark server read `shift+esc` | |
| quick switcher `mod+k` | |
| scroll `pageup`/`pagedown` | |
| focus text area (any key) | |

**Custom (global) keybinds:** the System → Custom Keybinds category (desktop only) has an "Add a Keybind" button and the notice "Keybinds are disabled while this page is visible." Its action list was not enumerated (Not verified).

---

## Not verified

- Electron cache bundles: blocked by macOS TCC, so desktop-only differences come only from `isPlatformEmbedded`/`cX()` branches in the web bundle.
- Initial value of "Same as Device Theme" and the default of `useSystemTheme`.
- Defaults for HDR range, "Speak all messages out loud", and the Favorites toggle; its label/description and the Display Name Styles caption live in separate message modules that were not decoded.
- That the `nz.k` theme fieldset export equals `APPEARANCE_THEME_FIELDSET`: inferred from module `714907` exporting `k:()=>ed`, where `ed` is that fieldset.
- That the dual Light/Dark selector appears only when "Same as Device Theme" is on (`usePredicate` body not read).
- Mobile-only rows (tap to react, etc.) and markdown formatting shortcuts in the editor.
- The persistence backend (localStorage vs IndexedDB) for `DeviceSettingsStore`/`PersistedStore`.
- Everything here is static reading of one stable build (678976); experiment gates (`ChatBarAdvancedAccordion`, `EnableSendButtonOutsideExperiment`, `HDRDynamicRangeSetting`, `EnableCustomCursorSetting`) decide which rows a given user sees.
