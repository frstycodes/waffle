// Screen and system-audio capture for Go Live: ScreenCaptureKit on macOS, delivering packed BGRA
// frames and interleaved 48 kHz stereo audio. Waffle's own output (voice chat) is excluded so
// viewers don't hear the call echoed. Needs the Screen Recording permission; macOS asks the first time.

use std::sync::{
    Arc,
    mpsc::{Receiver, Sender, SyncSender, channel, sync_channel},
};

use screencapturekit::{cv::CVPixelBufferLockFlags, prelude::*, screenshot_manager::SCScreenshotManager};

use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSRunningApplication};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::{ShareSource, ShareableScreen, ShareableWindow, video::VideoFrame};

// Stops capturing when dropped.
pub struct Capture {
    stream: SCStream,
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

// Frames arrive scaled to fit `max_width` x `max_height`; when the encoder lags, new frames are
// dropped rather than queued so latency stays flat.
pub struct Captured {
    pub capture: Capture,
    pub frames: Receiver<Arc<VideoFrame>>,
    pub audio: Receiver<Vec<f32>>,
}

pub fn start(source: ShareSource, fps: u32, max_width: u32, max_height: u32, with_audio: bool) -> Result<Captured, String> {
    let content = shareable_content()?;
    let filter = content_filter(&content, source)?;
    // Capture at the content's pixel size (points x Retina scale), then fit the stream bounds.
    let rect = filter.content_rect();
    let scale = filter.point_pixel_scale() as f64;
    let (width, height) = fit((rect.size.width * scale) as u32, (rect.size.height * scale) as u32, max_width, max_height);
    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_pixel_format(PixelFormat::BGRA)
        .with_shows_cursor(true)
        .with_fps(fps)
        .with_captures_audio(with_audio)
        .with_sample_rate(48_000)
        .with_channel_count(2)
        .with_excludes_current_process_audio(true);

    let (tx, rx) = sync_channel(1);
    let mut stream = SCStream::new(&filter, &config).map_err(|e| e.to_string())?;
    stream
        .add_output_handler(move |sample: CMSampleBuffer, kind: SCStreamOutputType| forward(&sample, kind, &tx), SCStreamOutputType::Screen)
        .map_err(|e| e.to_string())?;
    let (audio_tx, audio) = channel();
    stream
        .add_output_handler(move |sample: CMSampleBuffer, kind: SCStreamOutputType| forward_audio(&sample, kind, &audio_tx), SCStreamOutputType::Audio)
        .map_err(|e| e.to_string())?;
    stream.start_capture().map_err(|e| e.to_string())?;
    Ok(Captured { capture: Capture { stream }, frames: rx, audio })
}

fn shareable_content() -> Result<SCShareableContent, String> {
    SCShareableContent::get().map_err(|e| format!("Screen capture unavailable (Screen Recording permission?): {e}"))
}

// Audio capture follows the filter: a whole-display filter hears every app, an app or window
// filter only that app. Waffle's own audio is excluded separately in the stream configuration.
fn content_filter(content: &SCShareableContent, source: ShareSource) -> Result<SCContentFilter, String> {
    let displays = content.displays();
    let builder = SCContentFilter::create();
    let builder = match source {
        ShareSource::Screen(id) => {
            let display = displays.iter().find(|d| d.display_id() == id).ok_or("That screen is no longer connected")?;
            builder.with_display(display).with_excluding_windows(&[])
        }
        ShareSource::App(pid) => {
            // ponytail: an app filter needs a display; apps spread over several screens show the first.
            let display = displays.first().ok_or("No display to capture")?;
            let app = content.applications().into_iter().find(|a| a.process_id() == pid).ok_or("That app is no longer running")?;
            builder.with_display(display).with_including_applications(&[&app], &[])
        }
        ShareSource::Window(id) => {
            let window = content.windows().into_iter().find(|w| w.window_id() == id).ok_or("That window is gone")?;
            builder.with_window(&window)
        }
    };
    builder.build().map_err(|e| e.to_string())
}

pub fn shareable_windows() -> Result<Vec<ShareableWindow>, String> {
    let own_pid = std::process::id() as i32;
    let windows = shareable_content()?.windows().into_iter().filter_map(|w| {
        let app = w.owning_application()?;
        let title = w.title().filter(|t| !t.is_empty())?;
        let normal = w.is_on_screen() && w.window_layer() == 0 && app.process_id() != own_pid;
        normal.then(|| ShareableWindow { id: w.window_id(), title, app_name: app.application_name(), pid: app.process_id() })
    });
    Ok(windows.collect())
}

pub fn screens() -> Result<Vec<ShareableScreen>, String> {
    let displays = shareable_content()?.displays();
    let screens = displays.iter().enumerate().map(|(i, d)| ShareableScreen { id: d.display_id(), name: format!("Screen {}", i + 1), width: d.width(), height: d.height() });
    Ok(screens.collect())
}

pub fn thumbnail(source: ShareSource, max_width: u32) -> Result<VideoFrame, String> {
    let filter = content_filter(&shareable_content()?, source)?;
    let rect = filter.content_rect();
    let scale = filter.point_pixel_scale() as f64;
    let (width, height) = fit((rect.size.width * scale) as u32, (rect.size.height * scale) as u32, max_width, u32::MAX);
    let config = SCStreamConfiguration::new().with_width(width).with_height(height).with_pixel_format(PixelFormat::BGRA).with_shows_cursor(false);
    let sample = SCScreenshotManager::capture_sample_buffer(&filter, &config).map_err(|e| e.to_string())?;
    sample.pixel_buffer().and_then(|buffer| copy_bgra(&buffer)).ok_or_else(|| "Screenshot had no image".into())
}

// Draws NSRunningApplication's icon into a BGRA bitmap we own.
pub fn app_icon(pid: i32, size: u32) -> Option<VideoFrame> {
    let icon = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?.icon()?;
    let side = size.max(1) as isize;
    // SAFETY: null planes make AppKit allocate the buffer; the sizes describe 8-bit RGBA rows.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            side,
            side,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            side * 4,
            32,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    icon.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side as f64, side as f64)));
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let (stride, len) = (bitmap.bytesPerRow() as usize, side as usize * 4);
    let data = bitmap.bitmapData();
    if data.is_null() {
        return None;
    }
    // SAFETY: AppKit owns `stride * side` bytes at `data` for as long as `bitmap` lives.
    let pixels = unsafe { std::slice::from_raw_parts(data, stride * side as usize) };
    let bgra = pixels.chunks(stride).flat_map(|row| row[..len].as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [b, g, r, a])).collect();
    Some(VideoFrame { width: side as u32, height: side as u32, bgra })
}

// ScreenCaptureKit hands over float32 audio, usually one buffer per channel.
fn forward_audio(sample: &CMSampleBuffer, kind: SCStreamOutputType, tx: &Sender<Vec<f32>>) {
    if kind != SCStreamOutputType::Audio {
        return;
    }
    let Ok(list) = sample.audio_buffer_list() else { return };
    let channels: Vec<Vec<f32>> = (0..list.num_buffers()).filter_map(|i| list.get(i)).map(|b| floats(b.data())).collect();
    let stereo = match channels.as_slice() {
        [interleaved] => interleaved.clone(),
        [left, right, ..] => left.iter().zip(right).flat_map(|(&l, &r)| [l, r]).collect(),
        [] => return,
    };
    let _ = tx.send(stereo);
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_ne_bytes(*b)).collect()
}

fn forward(sample: &CMSampleBuffer, kind: SCStreamOutputType, tx: &SyncSender<Arc<VideoFrame>>) {
    if kind != SCStreamOutputType::Screen {
        return;
    }
    let Some(frame) = sample.pixel_buffer().and_then(|buffer| copy_bgra(&buffer)) else { return };
    let _ = tx.try_send(Arc::new(frame));
}

fn copy_bgra(buffer: &screencapturekit::cv::CVPixelBuffer) -> Option<VideoFrame> {
    let guard = buffer.lock(CVPixelBufferLockFlags::READ_ONLY).ok()?;
    let (width, height, stride) = (guard.width(), guard.height(), guard.bytes_per_row());
    // SAFETY: the slice is only read while `guard` keeps the buffer locked.
    let pixels = unsafe { guard.as_slice() }?;
    let row = width * 4;
    let mut bgra = Vec::with_capacity(row * height);
    for y in 0..height {
        bgra.extend_from_slice(pixels.get(y * stride..y * stride + row)?);
    }
    Some(VideoFrame { width: width as u32, height: height as u32, bgra })
}

// Largest even size with the display's aspect ratio inside the bounds (H.264 needs even sides).
fn fit(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64).min(1.0);
    let even = |v: f64| ((v as u32) & !1).max(2);
    (even(width as f64 * scale), even(height as f64 * scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Needs the Screen Recording permission for the terminal running it.
    #[test]
    #[ignore]
    fn thumbnails_and_icons() {
        for screen in screens().expect("screens") {
            let thumb = thumbnail(ShareSource::Screen(screen.id), 400).expect("screen thumbnail");
            assert!(thumb.width <= 400 && thumb.bgra.len() == (thumb.width * thumb.height * 4) as usize, "{screen:?}");
            assert!(thumb.bgra.as_chunks::<4>().0.iter().any(|px| px[..3] != [0, 0, 0]), "{} is not all black", screen.name);
        }
        let window = shareable_windows().expect("windows").into_iter().next().expect("some window open");
        let thumb = thumbnail(ShareSource::Window(window.id), 400).expect("window thumbnail");
        assert!(thumb.width <= 400);
        let icon = app_icon(window.pid, 32).expect("app icon");
        assert_eq!((icon.width, icon.height, icon.bgra.len()), (32, 32, 32 * 32 * 4));
        assert!(icon.bgra.as_chunks::<4>().0.iter().any(|px| px[3] > 0), "{} icon has visible pixels", window.app_name);
    }

    #[test]
    fn fits_inside_bounds_keeping_aspect() {
        assert_eq!(fit(3024, 1964, 1280, 720), (1108, 720));
        assert_eq!(fit(1920, 1080, 1280, 720), (1280, 720));
        assert_eq!(fit(800, 600, 1280, 720), (800, 600));
    }
}
