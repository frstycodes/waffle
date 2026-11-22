# Discord: soundboard playback (RE notes)

Source: public web client bundle fetched 2026-11-22 from `https://discord.com/app`. Copy resolved from the en-US intl chunk `c30b23bab1166ada.js`. Static reading only.

Labels: **[obs]** read in the bundle. **[inf]** inferred. `Not verified:` no evidence.

## REST and CDN

| Purpose | Route [obs] |
|---|---|
| Play in a call | `POST /channels/{channel_id}/send-soundboard-sound` |
| Echo (replay someone's sound) | `POST /channels/{channel_id}/send-soundboard-echo`, body `{sound_id, source_guild_id}` |
| Default sounds | `GET /soundboard-default-sounds` |
| Guild sounds CRUD | `GET/POST /guilds/{g}/soundboard-sounds`, `PATCH/DELETE /guilds/{g}/soundboard-sounds/{id}` |
| Top sounds | `GET /guilds/{g}/top-soundboard-sounds` |
| Sound file | `https://cdn.discordapp.com/soundboard-sounds/{sound_id}` (no extension) |

- **[obs]** Play body (`web.355a76e660bee7a5.js`):
  ```
  o={sound_id:t.soundId,emoji_id:t.emojiId,emoji_name:t.emojiName??a?.name};"0"!==t.guildId&&(o.source_guild_id=t.guildId),
  A.Bo.post({url:N.Rsh.SEND_SOUNDBOARD_SOUND(e),body:o,signal:s.signal,...})
  ```
  The body is `{sound_id, emoji_id, emoji_name}`. `source_guild_id` is added only for non-default sounds. Default sounds use the pseudo guild id `"0"`, and the client maps the default list with `N0(e,"0")`.
- **[obs]** The request is aborted if the user leaves that voice channel while it is uploading. The abort check is throttled to 1 s.
- **[obs]** The sender plays the sound locally right away (`GUILD_SOUNDBOARD_SOUND_PLAY_LOCALLY`) without waiting for the echo back, then tracks usage (`SOUNDBOARD_TRACK_USAGE`).
- **[obs]** The CDN URL comes from `https://${CDN_HOST}` + `/soundboard-sounds/${id}`, with `CDN_HOST = "cdn.discordapp.com"` from GLOBAL_ENV in `app.html`.
- **[obs]** Guild sounds are fetched over the gateway, not REST: op **31 `REQUEST_SOUNDBOARD_SOUNDS` {guild_ids}**, answered by `SOUNDBOARD_SOUNDS`. Fetching happens when RTC connects and when the picker opens (`E7`).

## Gateway `VOICE_CHANNEL_EFFECT_SEND`

- **[obs]** Payload mapping:
  ```
  emoji, channel_id, user_id, animation_type, animation_id, sound_id, sound_volume, name, source_guild_id, is_echo, author_id
  ```
  `name` is the sound name. `is_echo` and `author_id` belong to the echo feature.
- **[obs]** `animation_type` enum: `PREMIUM=0`, `BASIC=1`. It is stored locally in `VoiceChannelEffectsPersistedStore` (default PREMIUM).

## Receiver playback

Code is `SoundboardManager` (`web.*.js`). It subscribes to `VOICE_CHANNEL_EFFECT_SEND`.
- **[obs]** The sound is skipped if `sound_id` is null, if `user_id` is self (the local copy already played), or if the channel is not the user's current voice channel.
- **[obs]** It is also skipped when **self-deafened** (`isDeaf()`) or when the sender is **soundboard-muted by you** (`isLocalSoundboardMuted(userId)`). Self-mute does not block hearing sounds.
- **[obs]** Effects of other actions: deafening stops and clears all playing sounds, and so does changing voice channel (`VOICE_CHANNEL_SELECT`).
- **[obs]** Volume (`904054`):
  ```
  Math.min(soundVolume * (M(setting.volume ?? 100)/100) * Math.min(outputVolume/100, 1), 1)
  M(v) = v<100 ? 100*(v/100)^(1/2.8) : 100*(20*log10(v/100)/6+1)     // amplitude -> perceptual
  ```
  `soundVolume` is the per-sound volume from the event (0..1, default 1). `setting.volume` is the stored soundboard volume. `outputVolume` is the user's output volume, capped at 1.
- **[obs]** Desktop/native path: the decoded buffer is played with `startSamplesLocalPlayback(key, buffer, volume)` on the DEFAULT media connection, and the buffer is cached per URL. Web path: an `<audio>` element. Replaying the same key restarts it (`currentTime=0`).
- **[obs]** The sound key is `${userId}-${soundId}`, or an anonymised `optout_${n}` when that user disallows voice recording. This is for clips.
- **[obs]** `GUILD_SOUNDBOARD_SOUND_PLAY_START {soundId,userId}` drives the "playing soundboard" state on the participant tile.

### Per-user soundboard mute
- **[obs]** Copy: `Mute Soundboard` / `Unmute Soundboard` (user context menu), and the tile label `Soundboard muted by you`.
- **[obs]** Stored in user settings `audioContextSettings.user[userId].soundboardMuted`, toggled through `AUDIO_TOGGLE_LOCAL_SOUNDBOARD_MUTE`.

## Sending rules and gating

- **[obs]** Channel eligibility (`123973`): DMs and group DMs are always allowed. Guild channels need `isGuildVoiceOrThread()`, `USE_SOUNDBOARD` (1<<42), and `SPEAK`.
- **[obs]** Sound eligibility `W(user, sound, channel)`:
  ```
  (canUseSoundboardEverywhere(user) || sound.guildId===channel.guild_id || sound.guildId==="0")
  && (no guild || can(USE_EXTERNAL_SOUNDS) || "0" || same guild) && sound.available
  ```
  Sounds from other guilds need Nitro (`canUseSoundboardEverywhere`) **and** `USE_EXTERNAL_SOUNDS` (1<<45) in the target channel.
- **[obs]** The voice-tray button is disabled when server-muted, suppressed, or deafened. Tooltips: `Soundboard disabled when server muted`, `Soundboard disabled when suppressed`, `Soundboard disabled when deafened`, otherwise `Open Soundboard` (`Open Soundboard, new sounds available` when the new-sounds badge applies).
- **[obs]** Nitro copy: `Use custom soundboard sounds across voice channels. Only with Nitro.`, `Access the best sounds from Soundboard across any of your Discord servers. Only with Nitro.` USE_EXTERNAL_SOUNDS description: `Allows members to use sounds for Soundboard and Soundmoji from other servers, only for Nitro members.` USE_SOUNDBOARD description: `Allows members to send sounds from server soundboard.`
- Not verified: soundboard rate limits or cooldown. No soundboard-specific cooldown copy or timer was found. The play POST has no error UI (`.then(FXj, abort-check)`), so a 429 fails silently on the client.
- **[obs]** Emoji reactions do have a client cooldown (`VoiceChannelEffectsStore`): after 20 sends inside 10 s, sending is blocked for the rest of that window. The server can also set the cooldown through `cooldownEndsAtMs`.

## Entry points

- **[obs]** Voice control tray button. Click toggles the soundboard popout. Hovering may show a "quick access" strip (experiment). Right-click opens the soundboard context menu.
- **[obs]** Keybinds: `SOUNDBOARD` ("Open Soundboard" / `Toggle soundboard`, which opens it at the mouse location) and `SOUNDBOARD_HOLD` (`Open Soundboard (Hold)`, the radial "wheel" while held). Education copy: `{$b} Hold {keybind} to use Soundboard in app. {openSettingsHook}.`
- **[obs]** Picker modes: `full`, `quick access`, `wheel`. The wheel is 424×424 with 100 px padding.

## Picker layout

- **[obs]** Sidebar categories in order: `FAVORITES=0`, `GUILD=1`, `DEFAULTS=2`, `SEARCH=3`, `FREQUENTLY_USED=4`, `TOP_SOUNDS=5`.
- **[obs]** Headers and icons:
  - `Favorites` (star)
  - `Frequently Used` (clock)
  - `<guild name>` (guild icon)
  - `Discord Sounds` (Clyde icon)
  - `Search Results` (magnifier)
  - `Top Sounds in {guildName}` (trophy)
- **[obs]** Sections are collapsible. Collapsed state syncs in `textAndImages.soundboardPickerCollapsedSections`. Nitro-locked guild sections sit below a divider, and a new-sounds badge shows `{count}`.
- **[obs]** Search placeholder candidate `Find the perfect sound`. Search is debounced at 350 ms (analytics `SEARCH_STARTED`). Not verified: what the search matches. Presumably name and emoji, but that was not read.
- **[obs]** Quick-access strip (`453997`): 6 slots (5 plus one locked upsell if a locked sound exists). Fill order: favourites → top 3 frequently used not in favourites → current guild → defaults. Nitro users get 6 unlocked slots.
- **[obs]** Volume control in the picker: aria `Soundboard Volume` (or `Incoming Soundboard Volume` under an experiment). In the expression picker it is a popout with tooltip `Control volume`.
- **[obs]** Each sound has a preview button (`Preview {emojiName} {soundName}`). At volume 0 it says `Increase your Soundboard volume setting to preview` instead.

## Preview versus play
- **[obs]** **Play** = `playSoundboardSound` → POST send-soundboard-sound plus local playback. Everyone in the call hears it.
- **[obs]** **Preview** = a local `<audio>` of the CDN URL only, with no REST call. On desktop it can route to the voice output device.

## Favourites
- **[obs]** Synced in the `FrecencyUserSettings` proto field **8** `favorite_soundboard_sounds` → `FavoriteSoundboardSounds { repeated fixed64 sound_ids = 1; repeated fixed64 ordered_sound_ids = 2; }`.
- **[obs]** Limit **250**. Over the limit the client shows `Oh no!` / `You cannot have more than {count} favorites.`
- **[obs]** Usage frecency: fields 10 `heard_sound_frecency` and 11 `played_sound_frecency`.

## Soundboard volume setting (Voice & Video)
- **[obs]** Proto `VoiceAndVideoSettings` field **9** `soundboard_settings` → `SoundboardSettings { float volume = 1; }`. It defaults to 100 when unset.
- **[obs]** The slider uses `maxValue:100` and shows `M(stored)` (perceptual). On change it stores `w(slider)`, which is amplitude (`(v/100)^2.8*100`). Writes are debounced 1 s.
- **[obs]** A stored value of 0 means "soundboard muted" (`isSoundboardVolumeMuted`) and switches the icon to `VoiceXIcon`. Hint copy: `Crank your Soundboard volume up or down in User Settings -> Voice & Video`.
- **[obs]** A separate setting, `soundmoji_volume`, is field 11.

## Emoji burst on the speaking tile
- **[obs]** `66004` (`d6caa202792cce11.js`) renders on the participant tile. It requires `emoji`, `animation_type`, and `animation_id` all non-null. It is filtered to the tile's `userId`, capped at **50** concurrent effects, and disabled under reduced motion.
- **[obs]** Custom non-animated emoji are resized. Each effect is removed when its animation completes.
- **[obs]** Timing constants nearby (`706787`): 6 s / 2 s / 2 s. A rotate-and-scale transform `scale(1+deg/120) rotate(deg)`.
- **[obs]** A second, simpler layer (`125248`) floats the emoji from the call bottom (`top: callHeight-50, left: 52%`) with one of 4 random CSS animations.
- **[obs]** The last effect per user is held in `VoiceChannelEffectsStore.getEffectForUserId` and cleared by `VOICE_CHANNEL_EFFECT_CLEAR`.
- Not verified: the exact keyframes, which live in CSS that was not downloaded.

## Unverified
- Not verified: the server-side play rate limit and its error code.
- Not verified: whether the server sends `VOICE_CHANNEL_EFFECT_SEND` back to the sender. The client ignores events from self either way.
- Not verified: the `animation_id` meaning (premium animation variant index).
- Not verified: the search matching rules.
