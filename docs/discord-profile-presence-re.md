# Discord profile, profile editing, and own-account rich presence: reverse-engineering notes

Source: the public web client, `https://discord.com/app`, stable build 678976, fetched 2026-11-22 with no auth. That is 364 initial chunks plus 6,331 lazy chunks from the webpack `p.u` map and 1,321 CSS chunks. The desktop cache (`~/Library/Application Support/discord/`) could not be read (`Operation not permitted`). This is the same JS the desktop client loads, but the desktop-only native modules (process observer, overlay) are not included.

Method: grep and slicing over the minified chunks. REA's `open_binary` validated a chunk as `javascript`. The REA JS-analysis tools were not exposed in this session.

Chunk names are content hashes and change with every build. `web.*` means `web.355a76e660bee7a5.js`. Intl keys are hashed, for example `t.a6XYD9`. The English values come from `c30b23bab1166ada.js` unless another file is named.

Labels used below:
- **OBSERVED**: read in the bundle; an excerpt or location is given.
- **INFERRED**: reasoned from observed code; the reasoning is stated.

---

## A. Profile popout / profile modal

### Fetch
- **OBSERVED** (`web.*`): `GET /users/{id}/profile` with query `{type, with_mutual_guilds, with_mutual_friends, with_mutual_friends_count, guild_id, connections_role_id, join_request_id}`.
  `url:E.Rsh.USER_PROFILE(e),query:{type:t,with_mutual_guilds:n,with_mutual_friends:r,with_mutual_friends_count:i&&(null==r||!r),guild_id:o,...}`
  On success the client dispatches `USER_UPDATE(body.user)` and `USER_PROFILE_FETCH_SUCCESS`. If `body.guild_member` is present it also dispatches `GUILD_MEMBER_PROFILE_UPDATE`.
- **OBSERVED** (`web.*`, `USER_PROFILE_FETCH_SUCCESS` reducer): top-level fields the client reads:
  - `user`, `user_profile{banner, accent_color, theme_colors, popout_animation_particle_type, bio, pronouns, collectibles[]}`
  - `connected_accounts[]` (filtered to supported platform types), `application_role_connections[]`
  - `premium_since`, `premium_type`, `premium_guild_since`, `legacy_username`
  - `application{...}` (bots), `badges[]`, `guild_badges[]`, `widgets[]`, `wishlist_settings`, `profile_read_state`, `private`
  - `mutual_guilds[{id,nick}]`, `mutual_friends[]`, `mutual_friends_count`
  - `guild_member_profile{guild_id, banner, accent_color, theme_colors, bio, pronouns, collectibles}`, `guild_member`
- **OBSERVED**: `premium_since` is also used to format the Nitro badge tooltip date (`formatToPlainString(R.t["8zbGNR"],{date:a})`).

### `connected_accounts`
- **OBSERVED** (`8c9db54576cb5000.js`): each account has `{type, id, name, verified, metadata}`. The verified check is generic: `G=h.verified?(...VerifiedCheck...)`. **Any type** with `verified: true` gets the check. For `twitter`, `bluesky`, and `mastodon`, `metadata.twitter_verified === "1"` also recolours the check to `PLATFORM_TWITTER` and swaps the tooltip.
- **OBSERVED**: per-type metadata lines exist for reddit, steam, letterboxd, twitter/bluesky/mastodon, paypal, ebay, and tiktok. `metadata.created_at` is formatted as "Member since {date}" (`t["9rfonh"]`).
- **OBSERVED** (`dd9e93b2dc744c42.js`, profile modal V2 `t8`): each row shows:
  - an icon from `platform.icon.lightPNG`
  - the name, linked through `getPlatformUserUrl(account)`, opened with `trusted: type !== "domain"`
  - aria label `"{Platform}, {name}, Open in Browser"`
  - a key of `${type}:${id}`

  Application identities (`/users/{id}/application-identities`) render in the same list, using the app icon.
- **OBSERVED** (`web.*` platform registry, `getPlatformUserUrl`). Every value is passed through `encodeURIComponent`. "en" is the registry's `enabled` flag: an `!1` (disabled) type can still render if the server returns it.

  | type string | name | URL |
  |---|---|---|
  | `twitch` | Twitch | `https://www.twitch.tv/{name}` |
  | `youtube` | YouTube | `https://www.youtube.com/channel/{id}` |
  | `bluesky` | Bluesky | `https://bsky.app/profile/{id}` (`%3A` turned back into `:`) |
  | `steam` | Steam | `https://steamcommunity.com/profiles/{id}` |
  | `reddit` | Reddit | `https://www.reddit.com/u/{name}` |
  | `twitter` | X | `https://x.com/{name}` |
  | `twitter_legacy` | Twitter (en=false) | `https://twitter.com/{name}` |
  | `spotify` | Spotify | `https://open.spotify.com/user/{id}` |
  | `github` | GitHub | `https://github.com/{name}` |
  | `roblox` | Roblox | `https://roblox.com/users/{id}/profile` |
  | `ebay` | eBay | `https://www.ebay.com/usr/{name}` |
  | `tiktok` | TikTok (en=false) | `https://www.tiktok.com/@{name}` |
  | `instagram` | Instagram (en=false) | `https://www.instagram.com/{name}` |
  | `letterboxd` | Letterboxd | `https://letterboxd.com/{name}/` |
  | `mastodon` | Mastodon (en=false) | `{id}` (the id is the URL) |
  | `domain` | Domain | `https://{id}/` (untrusted-link warning) |
  | `skype` | Skype (en=false) | `skype:{id}?userinfo` |

  No URL (plain text name): `battlenet`, `bungie`, `leagueoflegends`, `facebook`, `xbox`, `samsung`, `playstation`, `playstation-stg`, `epicgames`, `riotgames`, `paypal`, `crunchyroll`, `amazon-music`.

  Enum also contains `contacts`, `soundcloud`, and `meta_quest_or_horizon`.
- **OBSERVED**: icons are bundled assets per platform (`icon:{lightPNG,darkPNG,whitePNG,lightSVG,darkSVG,whiteSVG}`), not CDN URLs.

### Profile effects
- **OBSERVED** (`web.*`, fn `C`): there is no `profile_effect` field. Effects arrive in `user_profile.collectibles[]` / `guild_member_profile.collectibles[]` as `{sku_id, type, expires_at}`. The client picks the entry where `type === PROFILE_EFFECT` (and also `PROFILE_FRAME`). It sets a timer and drops the entry when `expires_at` passes.
- **OBSERVED** (`c89ca66084074a55.js` mod 942426): the config is resolved by `skuId` from the collectibles product store (`getProduct(sku).items[0]`) or the purchases store. If neither has it, the client fetches `GET /collectibles-products/{sku_id}?locale=…`.
- **OBSERVED** (`web.*`): the config shape is:
  - `{id, type, title, description, accessibilityLabel, animationType, staticFrameSrc, thumbnailPreviewSrc, reducedMotionSrc, effects[]}`
  - each `effects[]` layer is `{src, loop, height, width, duration, start, loopDelay, position, zIndex, randomizedSources[{src}]}`
- **OBSERVED**: `src` values are absolute URLs from the server. If a layer carries an `id`, its URL is `https://cdn.discordapp.com/media/v1/collectibles-shop/{skuId}/{assetId}/{static|animated|video}`:
  `return null==t?null:\`${a}/media/v1/collectibles-shop/${t}/${i?\`${i}/\`:""}${n}\``
- **OBSERVED**: layers are animated on a requestAnimationFrame clock (min frame interval `1000/24` ms).
  - Reduced motion, a non-autoplay non-hover state, or `ANIMATION_TYPE_PERSISTENT` renders a single `<img>` (`reducedMotionSrc`, or `thumbnailPreviewSrc` when not hovered).
  - `ANIMATION_TYPE_INTERMITTENT` effects render nothing outside the shop preview.
  - One random index per mount is applied to every layer's `randomizedSources`.

### `display_name_styles`
- **OBSERVED**: the user proto has `display_name_styles` (field 13). The REST save fields are `display_name_font_id`, `display_name_effect_id`, `display_name_colors` (ints).
- **OBSERVED** (`web.*` mod 508425): effect ids and names (labels from intl):
  `SOLID=1 "Solid"`, `GRADIENT=2 "Gradient"`, `NEON=3 "Neon"`, `TOON=4 "Toon"`, `POP=5 "Pop"`, `GLOW=6` (no label in the picker map), `PRISM=7 "Prism"`, `GUMMY=8 "Gummy"`, `TEST_1..4=1001..1004`.
- **OBSERVED** (mod 559949): fonts. `DEFAULT=11`, then:
  - `BANGERS=1`, `BIO_RHYME=2`, `CHERRY_BOMB=3`, `CHICLE=4`, `COMPAGNON=5`, `MUSEO_MODERNO=6`, `NEO_CASTEL=7`
  - `PIXELIFY=8`, `RIBES=9`, `SINISTRE=10`, `ZILLA_SLAB=12`, `PLAYPEN_SANS=13`, `ORBITRON=14`, `NEW_ROCKER=15`, `KALAM=16`, `HEXAGON=17`
- **OBSERVED** (`web.*`): colour count per effect: GRADIENT 2, GUMMY 4, PRISM 5, others 1. Minimum contrast ratio is 2.5 for GRADIENT, GLOW, and PRISM, and 3 for the rest.
- **OBSERVED** (`abd83513bd56377e.js`): CSS vars per effect:
  - GRADIENT and GLOW: `--custom-display-name-styles-gradient-stops` = `c0 10%, …, cN 90%`, evenly spaced across 10–90%.
  - PRISM: `--…-prism-stops` = N+1 stops at `100*i/N%` (wrapping to the first colour), and `--…-prism-cycle: max({nameLength}ch, 38px)`.
  - SOLID, NEON, TOON, POP, GUMMY: derived from colour[0] in HSL. light1 = l×1.2, light2 = l×1.6, dark1 = l×0.6, dark2 = l×0.2, plus toon and neon stroke colours.
- **OBSERVED**: GUMMY splits the name into per-letter `<span style="color:colors[i%n]; --custom-letter-index:i">`. Emoji are skipped, and a hidden plain-text copy is kept.
- **OBSERVED** (`css/163466.71f5984e9faac7fc.css`): animations. They play once with `forwards`, except `.loop` makes gummy infinite.
  - Gradient: `linear-gradient(to bottom right, stops)` with `background-clip:text`.
  - Prism: `linear-gradient(90deg, prism-stops)`, `background-size: prism-cycle`, `prism-scroll 2s linear` (`background-position-x` from 0 to −cycle).
  - Gummy: `gummy-squish 4s cubic-bezier(.44,.29,.48,1)`, delayed by `letter-index*50ms`. Keyframes: 7% `scale(1.28,.72)`, 14% `(.82,1.18)`, 21% `(1.1,.9)`, 28% `(.96,1.04)`, 35% back to rest.
  - Neon: white text, `text-shadow 0 0 (4px+.12em) main`, stroke `1px+.04em`, `neon-flicker 4s`.
  - Toon: stroke `1.6px+.04em`, `toon-animation 4s` on `:before`.
  - Pop: stroke `1.2px+.04em`, bounces −.05em and +.08em over 4s.

### "Member Since"
- **OBSERVED** (`d8b718b174e866ba.js` mod 743987): Discord date = `SnowflakeUtils.extractTimestamp(userId)`, and server date = `member.joinedAt`.
  - Both are formatted with `toLocaleDateString(locale,{month:"short",day:"numeric",year:"numeric"})`, which gives "Jan 5, 2021".
  - With no guild or member, only the Discord date shows.
  - Otherwise the row is `[Clyde icon 28px, tooltip "Discord"] date · divider · [guild icon SMOL, tooltip guild.name] date`.
  - The heading is "Member Since" (`a6XYD9`). For bots it is "Created On" (`A//N4k`).

### "Friends Since"
- **OBSERVED** (`dd9e93b2dc744c42.js`): heading "Friends Since" (`wlTO8v`). It shows only when the relationship type is FRIEND, and the value is `RelationshipStore.getSince(id)`, formatted the same way as Member Since.
- **OBSERVED** (`web.*`): `since` is read from `RELATIONSHIP_ADD` / `RELATIONSHIP_UPDATE`: `{id,type,user,since,nickname,...}`. READY `relationships[]` uses the same shape (**INFERRED**: same reducer path).
- **OBSERVED**: the modal's section order is:
  1. Game Friend (`Uv/eTx`)
  2. Member Since
  3. Friends Since
  4. Roles (guild)
  5. Connections (`3fe7U5`)
  6. Apps (`PHjkRE`)

### Recent activity / recently played
- **OBSERVED**: there is no `application-activity-statistics` endpoint for other users. Profile "Recent Activity" uses `GET /content-inventory/users/{id}/outbox` and stores `{entries[], …}`.
  - Entry shape: `{id, author_id, author_type, content_type, extra, participants, traits}`.
  - `content_type`: `PLAYED_GAME=1`, `WATCHED_MEDIA=2`, `TOP_GAME=3`, `LISTENED_MEDIA=4`, `LISTENED_SESSION=5`, `TOP_ARTIST=6`, `CUSTOM_STATUS=7`, `LAUNCHED_ACTIVITY=8`, `LEADERBOARD=9`.
  - Played-game `extra`: `{type:"played_game_extra", game_name, application_id, platform, …}`.
- **OBSERVED** (`97bdb7d93626dfe8.js`): `live` = current activities. `recent` = outbox entries filtered to drop items that duplicate a live activity.
- **OBSERVED** (`dd9e93b2dc744c42.js`, activity tab): the "Current activity" list renders **all** live activities plus stream and voice. There is **no cap and no "+N"**. Then comes "Recent activity" (`jzgEoL`). For yourself it adds the intro "You'll see your activity from the last 30 days here."
- **OBSERVED**: `/users/@me/activities/statistics/applications` exists (`USER_ACTIVITY_STATISTICS`). It is self-only, cached for 6h, and not used by the profile.

### Avatar click / game click
- **OBSERVED** (`web.*` mod 305385): clicking the avatar (profile modal, read-only mode) opens the media viewer with `user.getAvatarURL(guildId, 1024, animate)`. The displayed size is `min(1024, round(0.7*min(winW,winH)))`, and `shouldHideMediaOptions: true`.
- **OBSERVED** (`d2d0ae6c95846197.js`): clicking a played-game card's image or title opens the Game Profile (`GameProfileSources.UserProfile`, by `extra.application_id`). If no game profile resolves, the card is non-interactive. Applications are fetched through `GET /applications/{id}/public?with_guild=` or `GET /applications/public?application_ids=…`.
- **INFERRED**: the game profile modal's own data endpoint is in a lazy chunk not traced. `GAME_PROFILE` (`/games/{id}`) is a web route here, not an API call.

### Avatar decoration on the popout
- **OBSERVED** (`web.*`): the URL is `getCollectiblesItemAssetUrl({skuId, assetFormat: animated|static})`. The fallback is `https://{CDN}/avatar-decoration-presets/{asset}.png?size={px}&passthrough={animate}`.

---

## B. Editing your profile

### Endpoints and payloads
- **OBSERVED** (`web.*` `_L`): `PATCH /users/@me` body:
  - account fields: `username, email, email_token, password, new_password, discriminator, legacy_username`
  - `global_name`
  - avatar fields: `avatar` (data URI), `avatar_description`, `avatar_id` (re-apply a recent avatar)
  - collectibles: `avatar_decoration_sku_id`, `nameplate_sku_id`, `primary_guild_id`
  - display-name styles: `display_name_font_id`, `display_name_effect_id`, `display_name_colors` (all null when cleared)
  - `typing_indicator_style`
  - It also sends an md5 header for the avatar original.
- **OBSERVED** (`web.*` `gi`): `PATCH /users/@me/profile` (`USER_PROFILE(ME)`) is the main profile. `PATCH /guilds/{gid}/profile/@me` (`USER_GUILD_PROFILE`) is the per-server profile.
  - The body is built in `27f0bb31e49495e5.js` `yX`: `banner` (data URI or null), `bio`, `pronouns`, `accent_color`, `theme_colors`, `collectibles_sku_ids` (the full list after swapping the effect or frame SKU).
- **OBSERVED** (`web.*` mod 159001): per-server identity uses `PATCH /guilds/{gid}/members/@me` with:
  - `nick`, `avatar`, `avatar_description`, `avatar_id`, `avatar_decoration_sku_id`
  - `collectibles:{nameplate:{sku_id}|null}`
  - `display_name_font_id`, `display_name_effect_id`, `display_name_colors`
- **OBSERVED** (`52b6e35ba37f71f0.js` mod 974504): Save Changes sends the user PATCH, then the profile PATCH, then the legacy-username setting, then the server tag. The server tag is `PUT /users/@me/clan {identity_guild_id, identity_enabled: id !== null}` (`0f490c0bf343641f.js`). The guild tab sends the member PATCH, then the guild-profile PATCH.

### Limits
- **OBSERVED** (`52b6e35ba37f71f0.js`):
  - Display name `maxLength:32`.
  - Server nickname `maxLength:32`. The field is disabled without CHANGE_NICKNAME or MANAGE_NICKNAMES.
  - Pronouns `maxLength:40`, placeholder "Add your pronouns".
  - Registered-game name `maxLength:128`.
- **OBSERVED** (`web.*` mod 290386): bio max is controlled by an experiment, `2026-08-user-bio-max-length`. Default is **190**; variation 1 is **300**. The bio is a markdown input with a counter, and its description is "You can use markdown and links if you'd like."

### Avatar history
- **OBSERVED**: `GET /users/@me/avatars` returns `{avatars:[{id, storage_hash, description, …}]}`. `DELETE /users/@me/avatars/{id}` deletes one (shift-click skips the confirm dialog, and the toast says "Avatar removed").
- **OBSERVED**: the image is `https://{CDN}/avatars/{userId}/archived/{avatarId}/{storageHash}.{webp|gif|png}?size=…`, with `&animated=true` for animated WebP.
- **OBSERVED**: the limit is a hard-coded **6** (`recentAvatarsLimit:6`, `Math.max(6-j,0)` empty slots).
- **OBSERVED**: picking an avatar downloads it at size 1024 and opens the crop modal with `originalAsset`. If it is saved without edits (`ARCHIVED_ASSET`), the PATCH sends `avatar_id` instead of `avatar`.

### Server Tag and nameplates
- **OBSERVED**: the Server Tag section is titled "Server Tag", with "Choose a server tag to rep next to your name across Discord."
  - It is a select (max 8 visible, clearable) of guilds with `profile.tag`, showing tag and badge.
  - The current value is `user.primaryGuild{guildId, tag, badge}`.
  - The badge image is `https://{CDN}/clan-badges/{guildId}/{badge}.png?size=…`.
- **OBSERVED**: a nameplate is `collectibles.nameplate{sku_id, asset, label, palette, expires_at}` on the user and also on the guild member (per-server).

### Settings page
- **OBSERVED**: the page title is `t["vi7f+q"]`, with top tabs "Main Profile" (`2p07FR`) and "Per-server Profiles" (`kPHroX`).
- **OBSERVED**: the Main Profile sections, in order:
  1. Display Name (+ name styles, Nitro)
  2. Pronouns
  3. Profile Widgets ("Add Widgets")
  4. Avatar
  5. Avatar Decoration
  6. Nameplate
  7. Profile Effect
  8. Profile Frame
  9. Nitro users: Profile Banner and Profile Theme. Others: a single colour picker instead.
  10. Bio
  11. Server Tag (only if eligible)
  12. legacy username toggle
  13. Badges
- **OBSERVED**: the Per-server section is the description "Show who you are with different profiles for each of your servers.", then a guild picker, then:
  1. Server Nickname
  2. Pronouns
  3. Avatar*
  4. Avatar Decoration*
  5. (display name styles)
  6. Nameplate*
  7. Profile Effect*
  8. Profile Frame*
  9. Profile Banner
  10. Profile Theme
  11. Bio* (placeholder "Tell this server a bit about yourself")

  `*` = shows the Nitro badge icon. Non-Nitro users get an upsell overlay on these sections. The preview is titled "Preview For {guildName}".
- **OBSERVED** (`13af0a55535ab31a.js`): the unsaved-changes bar reads "Careful – you have unsaved changes!", with buttons "Reset" (link look) and "Save Changes" (green, submitting state).
  - Switching tabs or guilds with pending changes is blocked. The bar flashes `data-emphasized` for 1s, the app shakes, and screen readers hear "…You must save or reset your changes before closing your profile."
  - The error text replaces the message on failure.

---

## C. Own-account rich presence

### Registered Games page (`52b6e35ba37f71f0.js`)
- **OBSERVED**: the panel is "Registered Games", with category "Current Game" first, then "Added Games".
  - Subtitle: "No games added", or the IGDB credit when games exist.
  - Related link: Activity Privacy.
- **OBSERVED**: with no current game, the page shows "No game detected" and a row reading "Not seeing your game?" with an **"Add it!"** button.
  - The button opens a popout: a "Select" dropdown of `getCandidateGames()` (running processes, label = name, id = pid), plus Cancel and "Add Game".
  - Confirming calls `addGame(pid, name)`, which dispatches `RUNNING_GAME_ADD_OVERRIDE`.
- **OBSERVED**: each game row has the following.
  - Name: verified or detected games show a "Verified" check. Others get an editable text input (128 chars) that commits on blur or Enter (`RUNNING_GAME_EDIT_NAME`).
  - Subline: "Now playing!" or "Last played {when}". Hovering shows the exe path.
  - A "More" menu with "Report" (incorrect detection) and "Remove".
  - A "Toggle detection" switch. Turning off a parent that has subgames (Roblox) asks "Turn off all {platform}".
  - There is **no overlay toggle on the row** in this build. `toggleOverlay` still exists in the actions.
- **OBSERVED** (`web.*` RunningGameStore): storage is **local**, not a user-settings proto.
  - `Storage.set("RunningGameStore", {gamesSeen[], gameOverrides{}, enableOverlay{}, enableOverlayV3{}, enableDetection{}})`.
  - Key = `${exePath}:${name}`, or `${exePath}:${id}` for Roblox-style subgames.
  - An added game is stored as `gameOverrides[key] = {...process, add:true, hidden:false, lastFocused}`. The edited-name form is `{name, exePath, cmdLine, lastFocused, add:true}`.
  - Detection toggle: `enableDetection[key]=bool`, default true.
- **OBSERVED**: the detectables list comes from `https://{CDN}/detectables/games-v1.json` (with ETag), or `GET /games/detectable` as the fallback. Exclusions come from `/games/detectable/exclusions`.

### Sharing settings
- **OBSERVED**: the Activity Privacy panel has two categories.
  - **"What Activity I Share"**: the first toggle is "Share my activity" / "Share activity information from games and connected apps I use…", bound to proto `status.show_current_game` (default true). Below it are the notify-friends and go-live toggles.
  - **"Where I Share Activity"**:
    - "Automatically share my activity when joining a server", a select bound to `privacy.default_guilds_activity_restricted` with options "Share in all servers", "Share in servers with 200 or fewer members", and "Do not share in any servers".
    - A per-server list bound to `privacy.activity_restricted_guild_ids`. When sharing is off it warns "Server settings ignored when not sharing".
- **OBSERVED**: the "Share your detected activities with others" string (`4q/EwS`) is still in the locale file but referenced by no code in this build.
- **OBSERVED**: proto fields (PrivacySettings): `activity_restricted_guild_ids`=13, `default_guilds_activity_restricted`=14, `activity_joining_restricted_guild_ids`=15. The join-restricted ids sit behind "Allow friends to join my game".
- **OBSERVED**: a per-server "Share my activity with this server" modal exists (`a5e33d4f3f4a4ce8.js`).
- **INFERRED**: it is reached from the server privacy / guild-space UI; the entry point was not traced.
- **OBSERVED**: the per-game share gate (`web.*` `kv`) returns false when `show_current_game` is false or the status is invisible. A library application with the `PRIVATE` flag is also hidden.
- **OBSERVED**: a `PLAYING` activity with no `application_id` is shared only if the name matches exactly one detectable game (that game's rules then apply). Otherwise the `show_current_game` value applies.

### Presence payload
- **OBSERVED** (`web.*` LocalActivityStore): for the visible running game the client builds
  `{type:0 (PLAYING), name:d.name, application_id: d.id ?? GameStore.findGame(d)?.id, timestamps:{start: streamStart ?? d.start}, ...robloxExtras}`.
  - `d.start` = `Date.now()` when the game became visible.
  - Roblox extras are `{name: gameName, sync_id: timeStarted}`.
  - This activity is skipped if an SDK or RPC activity already covers the same app or name.
- **OBSERVED**: the activity is sent over the gateway as op `PRESENCE_UPDATE` `{status, since, activities, afk}`, throttled to at most 5 updates per 20s.
- **INFERRED**: for an "Add it!" game that is not in the detectables DB, both `d.id` and `findGame` are null, so `application_id` is `undefined` and is dropped by JSON serialization. The activity is then `{type:0, name, timestamps:{start}}`.

---

## Not verified
- Anything specific to the native desktop: process scanning, the overlay, and whether desktop uses a different chunk set from the web build.
- The game profile modal's data endpoint and fields.
- The popout's own compact activity layout. Only the modal's activity tab was traced, and it showed no "+N" collapse.
- The nameplate CDN asset path.
- Where the per-server activity privacy toggle is entered from in the server menu.

## Addendum: popout with several activities

- **OBSERVED** (`web.*` module 646986): the popout's "Current activity" section (`J6STd9`) builds `cards` (stream, then live activities, then voice; module 758873) and passes them to module 80330.
- **OBSERVED** (module 80330): only the first card renders: `let [f]=A` then `<li className=firstCardContainer>{f}</li>`. If `A.length>1`, one more `<li className=backgroundCardContainer>` is added. It is an empty overlay card with `aria-label` "View More" (`OBCR+p`), and its `onClick` opens the profile modal at `tabSection: ACTIVITY` (analytics `PRESS_SHOW_MORE_ACTIVITY`).
- **OBSERVED** (CSS `container_M2WrKU`, in `242187.*.css`):
  - `--custom-peek-base:8px; --custom-lift-hover:2px; --custom-peek-hover:2px; --custom-transition-duration:200ms`.
  - The background card is inset `padding-inline:8px` and shows 8px below the first card.
  - On hover the first card lifts 2px and the peek grows to 12px, with `ease-in-out`.
  - The first card gets `box-shadow: var(--shadow-low)`.
  - There is no count, no dots and no carousel.
- **OBSERVED** (module 584904): in POPOUT theme, an activity card is wrapped in a clickable that opens the modal's Activity tab (`PRESS_CARD`, label "View All Activity"). The card's context menu also offers "View All Activity" in the popout only.
- **INFERRED**: "Recent activity" (the outbox) appears only in the full profile modal. The popout section reads current activities only.
- **OBSERVED** (`web.*` PrivacySettings descriptor): field 14 `default_guilds_activity_restricted` is the enum `GuildActivityStatusRestrictionDefault` with `OFF=0`, `ON_FOR_LARGE_GUILDS=1` and `ON=2`. Field 29 `default_guilds_activity_restricted_v2` (`ACTIVITY_STATUS_UNSET=0`, `OFF=1`, `ON_FOR_LARGE_GUILDS=2`, `ON=3`) exists too. Which of the two the select writes today was not traced. Waffle writes field 14.
