use super::*;

impl Waffle {
    // Discord's stream thumbnails (GET /streams/{key}/preview) for everyone live in the open server
    // and our call, refetched when older than a minute.
    pub(crate) fn refresh_stream_previews(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let wanted: Vec<String> = [self.guild_id.clone(), self.call.as_ref().and_then(|c| c.guild_id.clone())].into_iter().flatten().collect();
        let mut keys: Vec<(String, String)> = Vec::new();
        for guild in self.guilds.iter().filter(|g| wanted.contains(&g.id)) {
            for state in guild.voice_states.iter().filter(|v| v.streaming) {
                let Some(channel) = &state.channel_id else { continue };
                let fresh = self.stream_previews.get(&state.user_id).is_some_and(|(_, at)| at.elapsed() < PREVIEW_AGE);
                if !fresh {
                    keys.push((state.user_id.clone(), format!("guild:{}:{channel}:{}", guild.id, state.user_id)));
                }
            }
        }
        for (user_id, key) in keys {
            // Marked fetched now so a burst of events doesn't fire duplicate requests.
            let old = self.stream_previews.get(&user_id).map(|(url, _)| url.clone()).unwrap_or_default();
            self.stream_previews.insert(user_id.clone(), (old, std::time::Instant::now()));
            let client = client.clone();
            cx.spawn(async move |this, cx| {
                let result = client.call(reqwest::Method::GET, format!("/streams/{key}/preview"), None).await;
                if cfg!(debug_assertions) && std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                    eprintln!("stream preview {key}: {:?}", result.as_ref().map(|b| b.to_string()));
                }
                let url = match &result {
                    Ok(body) => body["url"].as_str().map(str::to_owned),
                    Err(_) => None,
                };
                let Some(url) = url else {
                    // Often just too early: the streamer uploads its first preview a few seconds
                    // after going live. Ask again soon instead of waiting for the next voice event.
                    cx.background_executor().timer(PREVIEW_RETRY).await;
                    this.update(cx, |this, cx| {
                        this.expire_stream_preview(&user_id);
                        this.refresh_stream_previews(cx);
                    })
                    .ok();
                    return;
                };
                if std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                    eprintln!("stream preview fetched for {key}");
                }
                this.update(cx, |this, cx| {
                    this.stream_previews.insert(user_id, (url, std::time::Instant::now()));
                    cx.notify(); // lint-allow(global-notify): the stage tiles (main), the voice rows (nav) and profile cards (overlays) all show it
                })
                .ok();
            })
            .detach(); // lint-allow(detached-task): a one-shot fetch holding a WeakEntity; it ends when the response lands or the update fails
        }
    }

    // Lets the next refresh fetch this streamer's preview again, keeping the picture shown so far.
    pub(crate) fn expire_stream_preview(&mut self, user_id: &str) {
        if let Some((_, at)) = self.stream_previews.get_mut(user_id)
            && let Some(old) = std::time::Instant::now().checked_sub(PREVIEW_AGE)
        {
            *at = old;
        }
    }

    // Tiles without a camera take their avatar's colour, as Discord's do; fetched once per person.
    pub(crate) fn refresh_avatar_colors(&mut self, cx: &mut Context<Self>) {
        if self.call.is_none() {
            return;
        }
        let wanted: Vec<(String, String)> = self.participants().into_iter().filter(|p| !self.avatar_colors.contains_key(&p.user_id)).map(|p| (p.user_id.clone(), p.user.map(|u| u.avatar).unwrap_or_else(|| self.user_avatar(&p.user_id)))).collect();
        for (user_id, avatar) in wanted {
            self.avatar_colors.insert(user_id.clone(), None);
            let url = small_png(&avatar);
            cx.spawn(async move |this, cx| {
                let color = discord::runtime().spawn(async move { avatar_color(&url).await }).await.ok().flatten();
                this.update(cx, |this, cx| {
                    this.avatar_colors.insert(user_id, color);
                    this.redraw(Column::Main, cx);
                })
                .ok();
            })
            .detach(); // lint-allow(detached-task): a one-shot colour fetch holding a WeakEntity; it ends when the response lands
        }
    }

    // What everyone sees of our Go Live before watching, uploaded as Discord's own client does:
    // a 512px-wide JPEG shortly after going live, then every five minutes (a minute after a failure).
    pub(crate) fn upload_stream_previews(&mut self, source: voice::ShareSource, cx: &mut Context<Self>) {
        let (Some(client), Some(call), Some(me)) = (self.client.clone(), self.call.as_ref(), self.me.as_ref()) else { return };
        let key = match &call.guild_id {
            Some(guild) => format!("guild:{guild}:{}:{}", call.channel.id, me.id),
            None => format!("call:{}:{}", call.channel.id, me.id),
        };
        self.preview_generation += 1;
        let generation = self.preview_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            loop {
                let live = this.update(cx, |this, _| this.preview_generation == generation && this.call.as_ref().is_some_and(|c| c.sharing.is_some()));
                if !live.unwrap_or(false) {
                    return;
                }
                let thumbnail = cx.background_spawn(async move { voice::thumbnail(source, 512).ok().and_then(|frame| jpeg_data_url(&frame)) }).await;
                let sent = match thumbnail {
                    Some(url) => {
                        let result = client.call(reqwest::Method::POST, format!("/streams/{key}/preview"), Some(serde_json::json!({ "thumbnail": url }))).await;
                        if let Err(e) = &result {
                            eprintln!("stream preview upload {key} failed: {e}");
                        } else {
                            if std::env::var_os("WAFFLE_STREAM_DEBUG").is_some() {
                                eprintln!("stream preview uploaded for {key}");
                            }
                            // Show our own new preview right away rather than at the next refresh.
                            let _ = this.update(cx, |this, cx| {
                                if let Some(me) = this.me.as_ref().map(|m| m.id.clone()) {
                                    this.expire_stream_preview(&me);
                                }
                                this.refresh_stream_previews(cx);
                            });
                        }
                        result.is_ok()
                    }
                    None => {
                        eprintln!("stream preview: couldn't grab a still of {source:?}");
                        false
                    }
                };
                cx.background_executor().timer(Duration::from_secs(if sent { 300 } else { 60 })).await;
            }
        })
        .detach(); // lint-allow(detached-task): the preview loop holds a WeakEntity and returns when the generation changes or the share ends
    }

    pub(crate) fn stream_preview(&self, user_id: &str) -> Option<String> {
        self.stream_previews.get(user_id).map(|(url, _)| url.clone()).filter(|url| !url.is_empty())
    }
}

// A BGRA frame as the data URL Discord's preview upload takes.
pub(super) fn jpeg_data_url(frame: &voice::VideoFrame) -> Option<String> {
    use base64::Engine;
    let rgb: Vec<u8> = frame.bgra.as_chunks::<4>().0.iter().flat_map(|[b, g, r, _]| [*r, *g, *b]).collect();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80).encode(&rgb, frame.width, frame.height, image::ExtendedColorType::Rgb8).ok()?;
    Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg)))
}

// Discord's CDN serves any avatar as a small PNG, whatever its original format.
pub(super) fn small_png(avatar: &str) -> String {
    let base = avatar.split('?').next().unwrap_or(avatar);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    format!("{stem}.png?size=32")
}

pub(super) async fn avatar_color(url: &str) -> Option<Hsla> {
    let bytes = reqwest::get(url).await.ok()?.bytes().await.ok()?;
    let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (r, g, b) = average_color(rgba.as_raw())?;
    let mut color = Hsla::from(gpui_kit::Rgba { r: r as f32 / 255., g: g as f32 / 255., b: b as f32 / 255., a: 1. });
    // Dark enough for white names and the avatar to stand out, as on Discord's tiles.
    color.l = color.l.clamp(0.2, 0.45);
    Some(color)
}

// The alpha-weighted mean colour of RGBA pixels; None when fully transparent.
pub(crate) fn average_color(rgba: &[u8]) -> Option<(u8, u8, u8)> {
    let (mut sum, mut weight) = ([0u64; 3], 0u64);
    for px in rgba.as_chunks::<4>().0 {
        let a = px[3] as u64;
        for (total, channel) in sum.iter_mut().zip(px) {
            *total += *channel as u64 * a;
        }
        weight += a;
    }
    (weight > 0).then(|| ((sum[0] / weight) as u8, (sum[1] / weight) as u8, (sum[2] / weight) as u8))
}
