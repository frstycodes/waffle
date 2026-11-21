# Discord voice messages and audio attachments: reverse-engineered behaviour

Source: public web client `https://discord.com/app`, fetched 2026-11-22. Main bundle `web.355a76e660bee7a5.js`; lazy chunks are served as `/assets/<hash>.js`, so chunk files are named by hash only. REA `analyze-javascript-application` was run on the main bundle (evidence `ev_6dc93a12…3043`), but it does not resolve webpack module ids, so the findings below come from reading the inert bundle text with grep. Nothing was executed. Module ids are written as `m<id>`.

Tags: **(observed)** means read in code. **(inferred)** means reasoned from code but not run.

## 1. Voice message player (`m383055` local `tl`, in `web.*.js`)

- Order, left to right: play/pause button, waveform canvas, time text, speed button, volume button, then a hidden `<audio>` element. (observed)
  `children:[…Kl…, I.D{onClick:H,"aria-label":ea}, tn.A{waveform…}, ts{played,currentTime,duration}, I.D{…speed…}, e4.A{muted,value…}, t]`
- The container is `role="region"` with aria-label "Voice Message". It gets an extra class while playing (`{[tr.he]:v}`). The extra class probably means a highlighted or brand background, since the bars turn white while playing. (observed / inferred)
- Time text (`ts`) has three states (observed):
  `a=null==n?"--:--":t?rB(Math.ceil(n-i)):rB(Math.ceil(n))`
  - Duration unknown: `--:--`.
  - Not started ("played" is false): total duration, rounded up.
  - Once started, paused mid-way, or seeked: remaining time, `ceil(duration - currentTime)`. It never shows elapsed time.
  - Format is `m:ss`. Minutes are not padded and there is no hours field (`153440.er`). (observed)
- Speed: the values are `CA:[.75,1,1.5,2]` (m518950). Clicking moves to the next value and wraps after 2. (observed)
  `n=(ti.CA.indexOf(S)+1)%ti.CA.length … dispatch({type:"MEDIA_PLAYBACK_RATE_UPDATE",rate:e,playbackType:"voice_message"})`
  - The label is `${rate.toString().replace(/^0/,"")}X`, which gives `.75X`, `1X`, `1.5X`, `2X` (upper-case X, no leading zero). (observed)
  - One rate applies to every voice message. It is stored in `MediaPlaybackStore`, a `DeviceSettingsStore` with `persistKey`, so it is saved per device. The default is 1. (observed)
  - If the stored rate is not in the list, `indexOf` returns -1 and the next click goes to `.75`. (inferred)
- Volume: there is a volume control (m20504). It is a speaker button that toggles mute (aria "Control volume") plus a slider that appears on hover and hides 150 ms after the pointer leaves. ArrowUp/ArrowDown step 5%. (observed)
  - The starting volume comes from localStorage `MediaPlayerVolume.volume`. Videos and audio attachments share this value. Mute always starts off in the voice player, and toggling it writes `MediaPlayerVolume.muted`. (observed)
  - The slider is perceptual: `slider = amp^(1/2.8)` and `amp = slider^2.8` (m792251). (observed)
- One plays at a time, but only among voice messages. On play the player dispatches `VOICE_MESSAGE_PLAYBACK_STARTED {src}`, and every other voice player whose `src` differs pauses itself. (observed)
  `e9._.dispatch(VOICE_MESSAGE_PLAYBACK_STARTED,{src:n}) … function e(e){let{src:t}=e;n!==t&&P(!1)}`
  - The generic audio/video player (m153440) neither sends nor listens to this event, so a voice message does not pause an audio attachment or a video, and they do not pause it. Videos use a separate `VIDEO_EMBED_PLAYBACK_STARTED`. (observed absence in m153440 / inferred cross-effect)
- Resume position is saved per attachment under the key `voice_message-<attachment.id>`. (observed)
  - It is saved once a second while playing, and on pause and drag-end.
  - It is kept only if `0.5 < pos < 0.95*duration`; otherwise it is deleted. Storage is an LRU of 25 entries, saved per device. (observed)
  - On mount, a saved position seeks there and the player starts in the "played" state, so it shows remaining time. (observed)
- Loading: `preload="none"`, switched to `"metadata"` on the first mouseenter. `loadedmetadata` replaces `duration_secs` with the element's duration unless it is NaN. (observed)
- Safari uses a separate lazy player component (chunk 594436) instead of a plain `<audio>`. It probably exists because of Ogg/Opus support. (observed / inferred)

## 2. Waveform (`m408822`, resampler `m934581`, bar animation `m539451`)

- Decoding: base64 → bytes, each byte `/255`. Invalid base64 gives no data. (observed)
  `t=window.atob(e)… n[e]=t.charCodeAt(e)/255`
- Canvas is 32 CSS px tall at DPR resolution. Bar pitch is 6 px, bar width 2 px (radius-1 rounded caps), gap 4 px. (observed)
- Bar count is `floor((width+4)/6)`. (observed)
- In chat (`fluid=false`) the width depends on duration (observed):
  `M=6*floor(((O<=.5?40:O>=45?294:(min(O,45)-.5)/44.5*254+40)+4)/6)-4`
  This gives 40 px at ≤0.5 s, growing linearly to 294 px at ≥45 s, snapped to the 6 px pitch.
- Resampling:
  - More samples than bars: average each bucket. Bucket edges are `round((k+1)*len/bars)`. (observed)
  - Fewer samples than bars, non-fluid: pad the end with zeros. (observed)
  - Fewer samples than bars, fluid (draft preview only): interpolate linearly. (observed)
- Bar height:
  - Background pass: `22*v+2` px, so at least 2 px. (observed)
  - Foreground pass: `24*v` for played bars. Unplayed bars use `max(anim, v-0.1)` with no minimum. (observed)
- Colours (observed):
  - Background layer: all bars, `BRAND_430` while playing, otherwise `BACKGROUND_MOD_MUTED`.
  - Bars before the playhead: `WHITE` while playing, `INTERACTIVE_TEXT_ACTIVE` when paused mid-way.
  - Bars after the playhead: background colour once played.
  - Before first play: every bar counts as "before the playhead" and uses `INTERACTIVE_TEXT_DEFAULT`.
  - Colour changes cross-fade over 200 ms.
  - The playhead index is `round(currentTime/duration*bars)`.
- Animation: a bar rising uses easeOutBack over 500 ms, a bar falling uses easeOutQuint over 150 ms. There is an optional staggered entrance (`animateOnMount`). (observed)
- Scrub: `mousedown` on the canvas seeks right away to `clamp((x-left)/width,0,1)`. Window `mousemove` keeps seeking until `mouseup`. Audio is paused while dragging (`playing:v&&!M`). (observed)
- Keyboard: the canvas has no key handlers and no tabIndex, so there are no arrow-key seeks. Only the play and speed buttons can take focus. (observed)
- Hover: the waveform has no hover state. The only mouseenter effect is the preload switch. (observed)

## 3. Fallbacks

- `waveform` missing or bad: an empty array is padded with zeros, giving a flat row of 2 px bars. Width still follows duration. If the container width is not measured yet, `[0,0,0,0,0]` is used. (observed)
- `duration_secs` missing: the time shows `--:--` until `loadedmetadata`. The waveform uses `duration??1`, so it falls to the minimum 40 px width. (observed)
  - The resume position is not saved, because it requires `durationSecs` and the cache key. (observed)
- Voice-message flag set but the file is not `.mp3/.m4a/.ogg/.opus/.wav/.flac`: the item is not classified as AUDIO, so it renders as a plaintext preview or generic file, not as a voice player. (inferred from the classifier)

## 4. Audio attachment player (`m383055.tN` → `H` → `m153440` type AUDIO)

- Classification is by filename, not `content_type` (observed):
  `l=/\.(mp3|m4a|ogg|opus|wav|flac)$/i … null!=t&&l.test(n)&&null!=e.url?"AUDIO"`
- Layout (observed):
  - Metadata row: the file name as a download link (plain text when not playable), then the size formatted by `Hb`. The separate download button is hidden (`hideDownloadButton:!0`).
  - The mosaic hover menu still offers a download for AUDIO.
  - Hidden `<audio controls=false>` with a `<source>`.
- Control bar, always visible for audio (observed):
  - Play / Pause / "Play again": the replay icon shows when `currentTime===duration`.
  - `current / duration` in `m:ss`, current zero-padded to the length of duration. It shows only when the player is wider than 250 px.
  - Seek bar with buffered ranges. Dragging pauses and resumes playback.
  - Volume button and slider.
- There is no speed control for audio; only VIDEO gets one, with `showPlaybackSpeedControl`. There is no fullscreen. (observed)
- Keyboard on the focused player (tabIndex 0) (observed):
  - Space plays/pauses.
  - ←/→ seek 5 s.
  - Digits 0–9 seek to N×10% and play.
- End of playback: `playing=false` and the replay icon shows. (observed)
- Errors: the `<audio>` element has no `onError` (the video path does), so no error UI appears. Unsupported codecs fail silently. (observed absence / inferred UX)
- The voice player also has no error UI: `onError` only sends analytics `voice_message_playback_failed`. (observed)

## 5. Wire

- `message.flags & 8192` (`IS_VOICE_MESSAGE`) makes every AUDIO-classified attachment of that message use the voice renderer. There is no per-attachment check. (observed)
  `renderAudioComponent:E?cE.SX:cE.Nj` with `E=(0,c.Lt)(g,er.pr7.IS_VOICE_MESSAGE)` (chunk `2420b0c09d1ff9a5.js`)
- Attachment fields used: `url` as src (not `proxy_url`), `duration_secs`, `waveform`, `size`, `filename`, and `id` for the cache key. The voice player does not read `content_type`. (observed, chunk `1f7a04bc1f57c3ec.js`)
  `src:n.url,durationSecs:n.duration_secs,waveform:n.waveform,…playbackCacheKey:l`
- Nothing is requested when you press play. Expired CDN links are handled when a channel loads instead. (observed)
  - If any cached attachment URL has no `ex` parameter, or its `ex` (hex epoch seconds) is less than 1 h away, the channel's message cache is cleared and the messages are fetched again ("Found expired attachment link, clearing messages").
  - Pins and mentions do the same.
- `POST /attachments/refresh-urls {attachment_urls:[url]}` → `refreshed_urls[0].refreshed` exists as a helper (`m679164.AN`), but the players do not call it. (observed)
- There is no voice-specific gateway event. `MEDIA_PLAYBACK_*` actions are local Flux actions. (inferred)
- Sending (desktop recording is behind the experiment `2026-09-desktop-voice-message-recording`) (observed):
  - `content:""` and `flags:IS_VOICE_MESSAGE`.
  - The attachment carries `duration_secs` and `waveform`.
  - The pending reply is honoured, so a voice message can be a reply.
  - The waveform is at most 256 bytes, bucket-averaged and clamped to 0–255.
  - The recorder's default MIME type is `audio/ogg`.
- Edit is impossible: `canEdit` returns false when `hasFlag(IS_VOICE_MESSAGE)` (m697470). The message has no text. (observed)

## 6. Copy (en-US, chunk `1fa3b6ef1ba21a57.js`)

- Voice player: "Play voice message" / "Pause voice message" (button aria); region "Voice Message"; speed aria "Change playback speed. Current speed is {playbackRate}x"; speed label `.75X|1X|1.5X|2X`; volume "Control volume". (observed)
- Audio player: "Play", "Pause", "Play again". (observed)
- Elsewhere (observed):
  - Notification body: "Sent a voice message".
  - Reply preview: "Click to see voice message", with a microphone icon.
  - Forwarding to a channel without `SEND_VOICE_MESSAGES`: "Voice messages are disabled in this channel."
- Message menu (observed):
  - "Save File" (download icon, id `save-voice-message-audio`) appears for voice messages and downloads `attachments[0].url`.
  - "Copy Text" is hidden because content is empty.
  - "Edit Message" is hidden by `canEdit`.
  - Hover download and remove-attachment actions are not shown on voice players, because `tl` ignores `renderAdjacentContent`.
- Recording toasts: "Failed to start recording" and "Couldn’t send your voice message. Please try again." (observed)

## 7. Limits

- Client-side recording stops automatically at `iA=1199500` ms, about 20:00. It shows an "approaching" warning from 1169.5 s and an "imminent" warning from 1184.5 s. Recordings shorter than 0.5 s are discarded. (observed)
- The player does not cap duration. The waveform width saturates at 45 s (294 px). (observed)
- At most 25 resume positions are kept. (observed)

## 8. End of playback

- Voice (observed): on `ended`, if the user is not dragging:
  - The saved position is set to 0 and playback pauses.
  - After 500 ms, "played" becomes false. The time switches back to total duration and all bars return to the default colour.
  - The next play starts from 0, which is ordinary `<audio>` behaviour after `ended` (inferred).
  - Dragging to the very end does the same reset.
- There is no autoplay of the next voice message; no code queues another one. (observed absence)
- Audio attachment: it stops and shows "Play again". (observed)

## Not verified

- What the "playing" container class (`tr.he`) and the decorative `tr.Kl/tr.fq` layer look like. The CSS was not inspected.
- What Safari's lazy player (chunk 594436) does differently.
- What happens to the time text when an Ogg file reports `duration = Infinity`. `isNaN(Infinity)` is false, so Infinity would be used; this is untested.
- How the volume popout is laid out inside the voice player, and whether the voice player reacts to the Space key at the container level. No handler was found.
- Server-side limits on voice message duration or size, and whether the server rejects edits to voice messages. Only the client is covered here.
- Any runtime behaviour. This is static reading only, with no browser session.
