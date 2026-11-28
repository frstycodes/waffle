use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}

// Discord's choices and ranges (docs/discord-settings-re.md).
pub const CHAT_FONT_SIZES: [u8; 7] = [12, 14, 15, 16, 18, 20, 24];
pub const GROUP_SPACINGS: [u8; 5] = [0, 4, 8, 16, 24];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Density {
    Compact,
    #[default]
    Default,
    Spacious,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum RoleStyle {
    #[default]
    InNames,
    NextToNames,
    Hidden,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Spoilers {
    #[default]
    OnClick,
    IfModerator,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum InputProfile {
    VoiceIsolation,
    Studio,
    #[default]
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum NoiseMode {
    #[default]
    Advanced,
    Standard,
}

// The interface fonts Appearance offers: (family as the OS names it, label), the first the default. Figtree and
// Gabarito are bundled (Figtree's weights cut from its variable font, so gpui can pick each); the rest are macOS's.
// The bundled faces (OFL, in assets/fonts), used until another installed font is picked.
pub const UI_FONT: &str = "Figtree";
pub const MONO_FONT: &str = "Roboto Mono";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemePref,
    // macOS's Liquid Glass behind the sidebar, where the system has it.
    pub native_sidebar: bool,
    // The interface's font family: Figtree (bundled) unless picked in Appearance.
    pub ui_font: String,
    pub mono_font: String,
    // Discord's "Enable Reduced Motion": used while `reduce_motion_sync` is off, which follows the OS instead.
    pub reduce_motion: bool,
    pub reduce_motion_sync: bool,
    // Accessibility, kept on this computer as Discord does (docs/discord-settings-re.md §2).
    // Text size in chat, one of `CHAT_FONT_SIZES`.
    pub chat_font_size: u8,
    // Space between message groups, one of `GROUP_SPACINGS`; None follows the display (16 cozy, 0 compact).
    pub group_spacing: Option<u8>,
    pub ui_density: Density,
    // 0..=1, in 0.05 steps. Leaves images, video and user colours alone unless `desaturate_user_colors`.
    pub saturation: f32,
    pub desaturate_user_colors: bool,
    pub high_contrast: bool,
    // Discord's "Show on/off indicators": a tick or cross inside each switch's thumb.
    pub switch_icons: bool,
    // Advanced › Developer: a framerate counter over the window, for testing.
    pub show_fps: bool,
    pub role_style: RoleStyle,
    pub underline_links: bool,
    pub display_name_styles: bool,
    // Toasts stay at least this long, 3..=20 s.
    pub min_toast_secs: u8,
    // Text-to-speech rate, 0..=10 in 0.25 steps, 1 is normal.
    pub tts_rate: f32,
    // Chat. Not verified: Discord syncs these three, but only in the protobuf settings Waffle doesn't use
    // (`display_compact_avatars`, `render_spoilers`); the send button is local in Discord too.
    pub compact_avatars: bool,
    pub spoilers: Spoilers,
    pub send_button: bool,
    pub app_icon: crate::app_icon::AppIcon,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub camera: Option<String>,
    // 0..=2, 1 is 100%.
    pub input_volume: f32,
    pub output_volume: f32,
    pub input_profile: InputProfile,
    pub sensitivity_auto: bool,
    // -100..=0 dB.
    pub sensitivity_db: f32,
    // Discord's "Advanced Voice Activity": a neural speech detector drives automatic sensitivity.
    pub advanced_voice_activity: bool,
    pub noise_suppression: bool,
    // Which suppressor while it's on: DeepFilterNet (Discord's Krisp slot) or WebRTC's.
    pub noise_mode: NoiseMode,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    // Global Attenuation: percent other apps are lowered (0 = off), and when.
    pub attenuation: u8,
    pub attenuate_self: bool,
    pub attenuate_others: bool,
    pub silence_warning: bool,
    pub qos: bool,
    pub push_to_talk: bool,
    pub ptt_release_ms: u32,
    pub silenced_cues: Vec<Cue>,
    pub keybinds: Vec<Keybind>,
    // Per-user listening volume (0..=2) and local mutes, kept across calls as Discord does.
    // lint-allow(unbounded-growth): one volume per person whose volume you moved, a few dozen at most
    pub user_volumes: HashMap<String, f32>,
    pub local_mutes: Vec<String>,
    // Activities you started, newest first: the launcher's Recent.
    pub recent_activities: Vec<String>,
    // Sites whose links activities may open without asking (Waffle's "Trust this domain").
    // lint-allow(unbounded-growth): domains you chose to trust one by one, a few dozen at most
    pub trusted_link_domains: Vec<String>,
    // Discord's per-user "Mute Soundboard" and "Disable Video", kept locally like the volume.
    pub soundboard_mutes: Vec<String>,
    pub hidden_video: Vec<String>,
    // Voice channels where the call shows only cameras and streams (Discord's "Show Non-Video Participants" off).
    // lint-allow(unbounded-growth): one entry per voice channel you turned it off in, a few dozen at most
    pub video_only_channels: Vec<String>,
    // Discord's Soundboard Volume as it stores it: amplitude, 100 = as uploaded, 0 = soundboard muted.
    // Shown on its perceptual scale (soundboard::perceptual).
    pub soundboard_volume: f32,
    // Soundboard favorites by sound id, oldest first, and how often we've played each sound. Discord syncs
    // both in its frecency settings; Waffle keeps them here.
    pub soundboard_favorites: Vec<String>,
    pub soundboard_uses: Vec<(String, u32)>,
    // Voice channels where our own Go Live's preview is hidden (Discord's per-channel "Hide Preview").
    pub hidden_self_streams: Vec<String>,
    // Registered games: ids not to detect, programs added as games, and every game seen here.
    pub games_off: Vec<String>,
    pub games_added: Vec<crate::games::Added>,
    pub games_seen: Vec<crate::games::Seen>,
    // How often we've reacted with each emoji, for the "frequently used" rows.
    // lint-allow(unbounded-growth): one entry per distinct emoji you have sent, a few hundred in practice
    pub emoji_uses: Vec<(crate::discord::Emoji, u32)>,
    // When each emoji was last used (unix seconds, newest last, at most FRECENCY_SAMPLES), for Discord's frecency.
    // lint-allow(unbounded-growth): one entry per distinct emoji you have sent, a few hundred in practice, each keeping only its last FRECENCY_SAMPLES times
    pub emoji_times: Vec<(crate::discord::Emoji, Vec<i64>)>,
    // lint-allow(unbounded-growth): one entry per distinct sticker you have sent, a few dozen in practice
    pub sticker_uses: Vec<(crate::discord::Sticker, u32)>,
    pub mention_filters: crate::inbox::MentionFilters,
    // The voice channel chat's width beside the call, and the voice channels whose new message
    // previews are off (Discord's "Show New Message Previews", local only).
    pub call_chat_width: f32,
    pub call_previews_off: Vec<String>,
    // Go Live quality: height (0 = source), frame rate, and whether the source's sound goes too.
    pub stream_resolution: u32,
    pub stream_fps: u32,
    pub stream_audio: bool,
    // What we last went live with: preselected in the picker, reused by the Toggle Screen Share keybind.
    pub last_share: Option<crate::go_live::LastShare>,
    // Generations of default keybinds this file has been given. A file from before the field reads 0
    // (the field's own default, not `Prefs::default`'s), so it gets newer defaults once; deleting one
    // afterwards sticks.
    #[serde(default)]
    pub keybinds_version: u32,
    // Signed-in accounts (no tokens: those live in the keyring) and the one to start with.
    pub accounts: Vec<crate::accounts::Account>,
    pub account: Option<String>,
    pub desktop_notifications: bool,
    pub dock_badge: bool,
    pub clock_24h: bool,
    // Discord keeps this in its protobuf settings (textAndImages.viewImageDescriptions), which Waffle
    // doesn't sync, so it's local.
    pub show_image_descriptions: bool,
    // Discord's "Allow playback and usage of /tts command" (textAndImages.enableTtsCommand), kept locally.
    pub tts_command: bool,
    // Off: the updater still checks and says so, but downloads only when asked.
    pub auto_download_updates: bool,
    // Recent message searches per account id, newest first.
    pub search_history: HashMap<String, Vec<crate::search::Saved>>,
    // Window zoom, 1 = 100% (⌘= / ⌘- / ⌘0).
    pub zoom: f32,
    // The server voice channel (guild, channel) a restart left, offered back once the new session is ready.
    pub rejoin_voice: Option<(String, String)>,
    // Announcement channels whose "Share with your followers!" bump was turned off with Don't show again.
    // lint-allow(unbounded-growth): the posts whose bump notice you hid, a few dozen at most
    pub publish_bump_hidden: Vec<String>,
    // Discord's one playback speed for every voice message.
    pub voice_rate: f32,
    // Where each voice message was left, by attachment id, oldest first; Discord keeps 25.
    pub voice_resume: Vec<(String, f32)>,
}

pub(super) const KEYBINDS_VERSION: u32 = 1;
pub(super) const STUDIO_THRESHOLD_DB: f32 = -84.;

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            native_sidebar: true,
            ui_font: UI_FONT.into(),
            mono_font: MONO_FONT.into(),
            reduce_motion: false,
            reduce_motion_sync: true,
            chat_font_size: 16,
            group_spacing: None,
            ui_density: Density::Default,
            saturation: 1.,
            desaturate_user_colors: false,
            high_contrast: false,
            switch_icons: false,
            show_fps: false,
            role_style: RoleStyle::InNames,
            underline_links: false,
            display_name_styles: true,
            min_toast_secs: 3,
            tts_rate: 1.,
            compact_avatars: false,
            spoilers: Spoilers::OnClick,
            send_button: false,
            app_icon: crate::app_icon::AppIcon::default(),
            input_device: None,
            output_device: None,
            camera: None,
            input_volume: 1.,
            output_volume: 1.,
            input_profile: InputProfile::Custom,
            sensitivity_auto: true,
            sensitivity_db: -60.,
            advanced_voice_activity: true,
            noise_suppression: true,
            noise_mode: NoiseMode::Advanced,
            echo_cancellation: true,
            auto_gain: true,
            attenuation: 0,
            attenuate_self: false,
            attenuate_others: true,
            silence_warning: true,
            qos: false,
            push_to_talk: false,
            ptt_release_ms: 20,
            silenced_cues: Vec::new(),
            keybinds: vec![Keybind { action: Action::PushToTalk, keys: Some("`".into()) }, share_keybind()],
            user_volumes: HashMap::new(),
            local_mutes: Vec::new(),
            recent_activities: Vec::new(),
            trusted_link_domains: Vec::new(),
            soundboard_mutes: Vec::new(),
            hidden_video: Vec::new(),
            video_only_channels: Vec::new(),
            soundboard_volume: 100.,
            soundboard_favorites: Vec::new(),
            soundboard_uses: Vec::new(),
            hidden_self_streams: Vec::new(),
            games_off: Vec::new(),
            games_added: Vec::new(),
            games_seen: Vec::new(),
            emoji_uses: Vec::new(),
            emoji_times: Vec::new(),
            sticker_uses: Vec::new(),
            mention_filters: crate::inbox::MentionFilters::default(),
            call_chat_width: crate::call::CHAT_MIN_WIDTH,
            call_previews_off: Vec::new(),
            stream_resolution: 720,
            stream_fps: 30,
            stream_audio: true,
            last_share: None,
            keybinds_version: KEYBINDS_VERSION,
            accounts: Vec::new(),
            account: None,
            desktop_notifications: true,
            dock_badge: true,
            clock_24h: false,
            show_image_descriptions: false,
            tts_command: true,
            auto_download_updates: true,
            search_history: HashMap::new(),
            zoom: 1.,
            rejoin_voice: None,
            publish_bump_hidden: Vec::new(),
            voice_rate: 1.,
            voice_resume: Vec::new(),
        }
    }
}

pub(super) fn share_keybind() -> Keybind {
    Keybind { action: Action::ToggleScreenShare, keys: Some(hotkeys::SHARE_KEYS.into()) }
}

impl Prefs {
    pub fn load() -> Self {
        let text = prefs_file().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        let mut prefs: Self = serde_json::from_str(&text).unwrap_or_default();
        // Before the sync switch, Reduce motion added to the OS setting; a file that had it on keeps it on.
        if prefs.reduce_motion && !text.contains("\"reduce_motion_sync\"") {
            prefs.reduce_motion_sync = false;
        }
        prefs.add_new_keybinds();
        prefs
    }

    pub(super) fn add_new_keybinds(&mut self) {
        if self.keybinds_version >= KEYBINDS_VERSION {
            return;
        }
        if !self.keybinds.iter().any(|k| k.action == Action::ToggleScreenShare) {
            self.keybinds.push(share_keybind());
        }
        self.keybinds_version = KEYBINDS_VERSION;
    }

    pub fn reduces_motion(&self) -> bool {
        if self.reduce_motion_sync { crate::material::system_reduce_motion() } else { self.reduce_motion }
    }

    // Discord's default spacing is 16 in cozy and 0 in compact until it's moved.
    pub fn group_spacing(&self, compact: bool) -> u8 {
        // Cozy defaults to Discord's widest, 24px, with the Spacious density (chat::row).
        self.group_spacing.unwrap_or(if compact { 0 } else { 24 })
    }

    pub(crate) fn save(&self) {
        let Some(path) = prefs_file() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    // Discord's input profiles overlay the stored switches (MediaEngineStore `e7`) rather than
    // overwrite them, so going back to Custom restores what was there.
    pub(super) fn set_profile(&mut self, profile: InputProfile) {
        self.input_profile = profile;
    }
}

impl Prefs {
    pub fn voice_settings(&self) -> voice::VoiceSettings {
        voice::VoiceSettings {
            input_device: self.input_device.clone(),
            output_device: self.output_device.clone(),
            input_volume: self.input_volume,
            output_volume: self.output_volume,
            input_mode: self.input_mode(),
            echo_cancellation: self.echo_cancellation(),
            noise_suppression: self.noise(),
            auto_gain: self.auto_gain(),
            attenuation: self.attenuation(),
            qos: self.qos,
        }
    }

    // Voice Isolation: Krisp-tier suppression, echo cancellation, AGC, automatic sensitivity with
    // Advanced Voice Activity. Studio: everything off, voice activity at a fixed -84 dB.
    pub(super) fn noise(&self) -> voice::NoiseSuppression {
        match (self.input_profile, self.noise_suppression, self.noise_mode) {
            (InputProfile::VoiceIsolation, ..) => voice::NoiseSuppression::Advanced,
            (InputProfile::Studio, ..) | (InputProfile::Custom, false, _) => voice::NoiseSuppression::Off,
            (InputProfile::Custom, true, NoiseMode::Advanced) => voice::NoiseSuppression::Advanced,
            (InputProfile::Custom, true, NoiseMode::Standard) => voice::NoiseSuppression::Standard,
        }
    }

    pub(super) fn echo_cancellation(&self) -> bool {
        self.profile_switch(self.echo_cancellation)
    }

    pub(super) fn auto_gain(&self) -> bool {
        self.profile_switch(self.auto_gain)
    }

    pub(super) fn profile_switch(&self, custom: bool) -> bool {
        match self.input_profile {
            InputProfile::VoiceIsolation => true,
            InputProfile::Studio => false,
            InputProfile::Custom => custom,
        }
    }

    pub(super) fn input_mode(&self) -> voice::InputMode {
        let ptt = voice::InputMode::PushToTalk { release_delay_ms: self.ptt_release_ms };
        match self.input_profile {
            InputProfile::Studio => voice::InputMode::VoiceActivity { auto: false, threshold_db: STUDIO_THRESHOLD_DB, advanced: false },
            _ if self.push_to_talk => ptt,
            InputProfile::VoiceIsolation => voice::InputMode::VoiceActivity { auto: true, threshold_db: self.sensitivity_db, advanced: true },
            InputProfile::Custom => {
                voice::InputMode::VoiceActivity { auto: self.sensitivity_auto, threshold_db: self.sensitivity_db, advanced: self.advanced_voice_activity }
            }
        }
    }

    pub(super) fn attenuation(&self) -> voice::Attenuation {
        voice::Attenuation { strength: self.attenuation.min(100), when_self_speaks: self.attenuate_self, when_others_speak: self.attenuate_others }
    }

    pub fn apply_to(&self, session: &voice::VoiceSession) {
        session.set_input_device(self.input_device.clone());
        session.set_output_device(self.output_device.clone());
        session.set_input_volume(self.input_volume);
        session.set_output_volume(self.output_volume);
        session.set_input_mode(self.input_mode());
        session.set_echo_cancellation(self.echo_cancellation());
        session.set_noise_suppression(self.noise());
        session.set_auto_gain(self.auto_gain());
        session.set_attenuation(self.attenuation());
    }
}

impl Prefs {
    // By Discord's frecency, topped up with Discord's own defaults when we haven't reacted enough yet.
    // Synced with Discord's settings-proto (frecency_sync.rs).
    pub fn frequent_emojis(&self, n: usize, pad: &[&str]) -> Vec<crate::discord::Emoji> {
        let now = chrono::Utc::now().timestamp();
        let mut uses = self.emoji_uses.clone();
        uses.sort_by_key(|(emoji, count)| {
            let times = self.emoji_times.iter().find(|(e, _)| e == emoji).map_or(&[][..], |(_, t)| t.as_slice());
            std::cmp::Reverse(frecency(*count, times, now))
        });
        let mut out: Vec<crate::discord::Emoji> = uses.into_iter().map(|(e, _)| e).take(n).collect();
        for name in pad {
            if out.len() >= n {
                break;
            }
            let emoji = crate::discord::Emoji::unicode(name);
            if !out.contains(&emoji) {
                out.push(emoji);
            }
        }
        out
    }

    // Most used first, counted like emoji.
    pub fn frequent_stickers(&self, n: usize) -> Vec<crate::discord::Sticker> {
        let mut uses = self.sticker_uses.clone();
        uses.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        uses.into_iter().map(|(s, _)| s).take(n).collect()
    }

    pub fn record_sticker(&mut self, sticker: &crate::discord::Sticker) {
        match self.sticker_uses.iter_mut().find(|(s, _)| s.id == sticker.id) {
            Some((_, count)) => *count += 1,
            None => self.sticker_uses.push((sticker.clone(), 1)),
        }
    }

    // Uses from another device (Discord's synced frecency): the higher count, and both sets of times.
    pub fn merge_emoji_uses(&mut self, remote: Vec<(crate::discord::Emoji, u32, Vec<i64>)>) {
        for (emoji, total, times) in remote {
            match self.emoji_uses.iter_mut().find(|(e, _)| *e == emoji) {
                Some((_, count)) => *count = (*count).max(total),
                None => self.emoji_uses.push((emoji.clone(), total)),
            }
            match self.emoji_times.iter_mut().find(|(e, _)| *e == emoji) {
                Some((_, kept)) => {
                    kept.extend(times);
                    kept.sort_unstable();
                    kept.dedup();
                    let excess = kept.len().saturating_sub(FRECENCY_SAMPLES);
                    kept.drain(..excess);
                }
                None => self.emoji_times.push((emoji, times.into_iter().rev().take(FRECENCY_SAMPLES).rev().collect())),
            }
        }
    }

    pub fn record_emoji(&mut self, emoji: &crate::discord::Emoji) {
        match self.emoji_uses.iter_mut().find(|(e, _)| e == emoji) {
            Some((_, count)) => *count += 1,
            None => self.emoji_uses.push((emoji.clone(), 1)),
        }
        let now = chrono::Utc::now().timestamp();
        match self.emoji_times.iter_mut().find(|(e, _)| e == emoji) {
            Some((_, times)) => {
                times.push(now);
                let excess = times.len().saturating_sub(FRECENCY_SAMPLES);
                times.drain(..excess);
            }
            None => self.emoji_times.push((emoji.clone(), vec![now])),
        }
    }
}

pub(super) const FRECENCY_SAMPLES: usize = 10;

// Discord's frecency score (its web client, module 283047): each recent use weighs by its age (100 up
// to 3 days, 70 to 15, 50 to 30, 30 to 45, 10 to 80, else 1), and the total use count scales their
// average. A count with no recorded times (from before they were kept) scores as itself.
pub(super) fn frecency(total: u32, times: &[i64], now: i64) -> u64 {
    if times.is_empty() {
        return u64::from(total);
    }
    let weight = |at: &i64| match (now - at) / 86_400 {
        ..=3 => 100,
        4..=15 => 70,
        16..=30 => 50,
        31..=45 => 30,
        46..=80 => 10,
        _ => 1,
    };
    let sum: u64 = times.iter().map(weight).sum();
    (u64::from(total) * sum).div_ceil(times.len() as u64)
}

pub(super) fn prefs_file() -> Option<std::path::PathBuf> {
    Some(crate::data_dir()?.join("settings.json"))
}

