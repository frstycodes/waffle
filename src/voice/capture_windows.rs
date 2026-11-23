// Screen, window and audio capture for Go Live on Windows, behind the same interface as the macOS
// ScreenCaptureKit backend (capture.rs): packed BGRA frames and interleaved 48 kHz stereo audio.
//
// Video: Windows.Graphics.Capture through the `windows-capture` crate. It delivers frames only when
// the content changes; media.rs repeats the last frame on a still screen. Frames are downscaled on
// the CPU (area average, so text stays readable) to fit the stream size.
// Audio: WASAPI process loopback (Windows 10 2004+). The whole screen captures every process except
// Waffle's own tree, so viewers don't hear the call echoed; an app or window captures only that
// app's process tree.
// ponytail: `ShareSource::App` captures the app's largest window (Windows.Graphics.Capture has no
// "all windows of an app" item); its audio does cover the whole app.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, SyncSender, channel, sync_channel},
    },
    thread::JoinHandle,
    time::Duration,
};

use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
    window::Window,
};

use super::{ShareSource, ShareableScreen, ShareableWindow, video::VideoFrame};

// Stops capturing when dropped.
pub struct Capture {
    control: Option<CaptureControl<FrameHandler, String>>,
    audio_stop: Arc<AtomicBool>,
    audio_thread: Option<JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
        self.audio_stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.audio_thread.take() {
            let _ = thread.join();
        }
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
    let (tx, frames) = sync_channel(1);
    let flags = FrameFlags { tx, max_width, max_height };
    let interval = MinimumUpdateIntervalSettings::Custom(Duration::from_micros(1_000_000 / u64::from(fps.max(1))));
    let (control, audio_pid) = match source {
        ShareSource::Screen(id) => {
            let monitor = monitor(id)?;
            (FrameHandler::start_free_threaded(settings(monitor, interval, flags)), AudioTarget::AllExcept(std::process::id()))
        }
        ShareSource::App(pid) => {
            let window = app_window(pid)?;
            (FrameHandler::start_free_threaded(settings(window, interval, flags)), AudioTarget::Only(pid as u32))
        }
        ShareSource::Window(id) => {
            let window = window_from_id(id);
            if !window.is_valid() {
                return Err("That window is gone".into());
            }
            let pid = window.process_id().map_err(|e| e.to_string())?;
            (FrameHandler::start_free_threaded(settings(window, interval, flags)), AudioTarget::Only(pid))
        }
    };
    let control = control.map_err(|e| format!("Screen capture failed: {e}"))?;

    let audio_stop = Arc::new(AtomicBool::new(false));
    let (audio_tx, audio) = channel();
    let audio_thread = with_audio.then(|| {
        let stop = audio_stop.clone();
        std::thread::spawn(move || {
            if let Err(e) = loopback(audio_pid, &audio_tx, &stop) {
                eprintln!("voice: go live audio: {e}");
            }
        })
    });
    Ok(Captured { capture: Capture { control: Some(control), audio_stop, audio_thread }, frames, audio })
}

// `ShareableScreen::id` is the display number Windows gives the monitor (`\\.\DISPLAYn`); 0 or a
// display that has gone away falls back to the primary monitor.
fn monitor(id: u32) -> Result<Monitor, String> {
    let chosen = Monitor::enumerate().ok().and_then(|all| all.into_iter().find(|m| m.index().ok() == Some(id as usize)));
    chosen.map_or_else(|| Monitor::primary().map_err(|e| format!("No display to capture: {e}")), Ok)
}

// Every display, the primary first, then in Windows' display order.
pub fn screens() -> Result<Vec<ShareableScreen>, String> {
    let primary = Monitor::primary().ok().map(|m| m.as_raw_hmonitor());
    let mut monitors: Vec<(bool, usize, Monitor)> = Monitor::enumerate()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|m| Some((Some(m.as_raw_hmonitor()) != primary, m.index().ok()?, m)))
        .collect();
    monitors.sort_by_key(|(not_primary, index, _)| (*not_primary, *index));
    let screens = monitors.iter().enumerate().map(|(n, (_, index, m))| ShareableScreen {
        id: *index as u32,
        name: format!("Screen {}", n + 1),
        width: m.width().unwrap_or(0),
        height: m.height().unwrap_or(0),
    });
    Ok(screens.collect())
}

// One still of `source`, fitted to `max_width`: a short capture session that keeps its first
// frame. Minimized windows draw nothing, so they time out.
pub fn thumbnail(source: ShareSource, max_width: u32) -> Result<VideoFrame, String> {
    let captured = start(source, 10, max_width, u32::MAX, false)?;
    let frame = captured.frames.recv_timeout(Duration::from_secs(2)).map_err(|_| "No picture from that source".to_owned())?;
    drop(captured);
    Ok(Arc::try_unwrap(frame).unwrap_or_else(|shared| VideoFrame { width: shared.width, height: shared.height, bgra: shared.bgra.clone() }))
}

fn settings<T: TryInto<GraphicsCaptureItemType>>(item: T, interval: MinimumUpdateIntervalSettings, flags: FrameFlags) -> Settings<FrameFlags, T> {
    Settings::new(
        item,
        CursorCaptureSettings::WithCursor,
        // The yellow capture border is a privacy cue Windows shows by default; keep it off the stream.
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        interval,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    )
}

// `ShareableWindow::id` is the HWND. Windows keeps HWNDs within 32 bits so 32- and 64-bit processes
// can share them; they're sign-extended back to pointer width.
fn window_from_id(id: u32) -> Window {
    Window::from_raw_hwnd(id as i32 as isize as *mut std::ffi::c_void)
}

fn window_id(window: &Window) -> u32 {
    window.as_raw_hwnd() as isize as u32
}

fn app_window(pid: i32) -> Result<Window, String> {
    let windows = Window::enumerate().map_err(|e| e.to_string())?;
    let area = |w: &Window| i64::from(w.width().unwrap_or(0)) * i64::from(w.height().unwrap_or(0));
    windows
        .into_iter()
        .filter(|w| w.process_id().ok() == Some(pid as u32))
        .max_by_key(area)
        .ok_or_else(|| "That app has no window to share".to_owned())
}

pub fn shareable_windows() -> Result<Vec<ShareableWindow>, String> {
    let own_pid = std::process::id();
    let windows = Window::enumerate().map_err(|e| e.to_string())?.into_iter().filter_map(|w| {
        let pid = w.process_id().ok()?;
        let title = w.title().ok().filter(|t| !t.trim().is_empty())?;
        if pid == own_pid {
            return None;
        }
        let app_name = w.process_name().map(|n| n.trim_end_matches(".exe").to_owned()).unwrap_or_default();
        Some(ShareableWindow { id: window_id(&w), title, app_name, pid: pid as i32 })
    });
    Ok(windows.collect())
}

struct FrameFlags {
    tx: SyncSender<Arc<VideoFrame>>,
    max_width: u32,
    max_height: u32,
}

struct FrameHandler {
    flags: FrameFlags,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for FrameHandler {
    type Flags = FrameFlags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { flags: ctx.flags, scratch: Vec::new() })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let buffer = frame.buffer().map_err(|e| e.to_string())?;
        let (width, height) = (buffer.width(), buffer.height());
        let pixels = buffer.as_nopadding_buffer(&mut self.scratch);
        let (out_w, out_h) = fit(width, height, self.flags.max_width, self.flags.max_height);
        let bgra = downscale(pixels, width, height, out_w, out_h);
        // The receiver is gone once Go Live stops; end the capture thread too.
        if let Err(std::sync::mpsc::TrySendError::Disconnected(_)) = self.flags.tx.try_send(Arc::new(VideoFrame { width: out_w, height: out_h, bgra })) {
            control.stop();
        }
        Ok(())
    }
}

// Largest even size with the source's aspect ratio inside the bounds (H.264 needs even sides).
fn fit(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let scale = (max_width as f64 / width as f64).min(max_height as f64 / height as f64).min(1.0);
    let even = |v: f64| ((v as u32) & !1).max(2);
    (even(width as f64 * scale), even(height as f64 * scale))
}

// Area-average BGRA resize: each output pixel averages the source block it covers. At 1:1 (or when
// only an odd edge is trimmed) rows are copied as they are.
fn downscale(src: &[u8], width: u32, height: u32, out_w: u32, out_h: u32) -> Vec<u8> {
    let (w, h, ow, oh) = (width as usize, height as usize, out_w as usize, out_h as usize);
    let mut out = Vec::with_capacity(ow * oh * 4);
    if ow * 2 > w && oh * 2 > h && w - ow <= 1 && h - oh <= 1 {
        for y in 0..oh {
            out.extend_from_slice(&src[y * w * 4..(y * w + ow) * 4]);
        }
        return out;
    }
    let spans = |from: usize, to: usize| -> Vec<(usize, usize)> { (0..to).map(|i| (i * from / to, ((i + 1) * from / to).max(i * from / to + 1))).collect() };
    let (xs, ys) = (spans(w, ow), spans(h, oh));
    let mut row = vec![0u32; w * 4];
    for &(y0, y1) in &ys {
        row.iter_mut().for_each(|v| *v = 0);
        for y in y0..y1 {
            for (acc, &px) in row.iter_mut().zip(&src[y * w * 4..(y + 1) * w * 4]) {
                *acc += u32::from(px);
            }
        }
        for &(x0, x1) in &xs {
            let count = ((x1 - x0) * (y1 - y0)) as u32;
            for c in 0..4 {
                let sum: u32 = (x0..x1).map(|x| row[x * 4 + c]).sum();
                out.push((sum / count) as u8);
            }
        }
    }
    out
}

#[derive(Clone, Copy)]
enum AudioTarget {
    // Everything the system plays except this process tree (Waffle itself).
    AllExcept(u32),
    // Only this process tree.
    Only(u32),
}

// Process loopback in event mode, converted by Windows to 48 kHz stereo float; forwards
// interleaved samples until `stop` is set or the receiver is dropped.
fn loopback(target: AudioTarget, tx: &Sender<Vec<f32>>, stop: &AtomicBool) -> Result<(), String> {
    use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};
    let _ = wasapi::initialize_mta();
    let (pid, include) = match target {
        AudioTarget::AllExcept(pid) => (pid, false),
        AudioTarget::Only(pid) => (pid, true),
    };
    let format = WaveFormat::new(32, 32, SampleType::Float, 48_000, 2, None);
    let mut client = AudioClient::new_application_loopback_client(pid, include).map_err(|e| e.to_string())?;
    client
        .initialize_client(&format, Direction::Capture, StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 }, None)
        .map_err(|e| e.to_string())?;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
    client.start_stream().map_err(|e| e.to_string())?;
    let mut bytes = std::collections::VecDeque::new();
    while !stop.load(Ordering::Relaxed) {
        // Process loopback signals only while the target plays; a timeout just means silence.
        if event.wait_for_event(200).is_err() {
            continue;
        }
        if capture.read_from_device_to_deque(&mut bytes).is_err() {
            break;
        }
        let whole = bytes.len() / 4 * 4;
        let samples: Vec<f32> = bytes.drain(..whole).collect::<Vec<u8>>().as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
        if !samples.is_empty() && tx.send(samples).is_err() {
            break;
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_inside_bounds_keeping_aspect() {
        assert_eq!(fit(2560, 1440, 1280, 720), (1280, 720));
        assert_eq!(fit(3440, 1440, 1280, 720), (1280, 534));
        assert_eq!(fit(801, 601, 1280, 720), (800, 600));
    }

    #[test]
    fn downscale_averages_blocks() {
        // 4x2 → 2x1: each output pixel is the mean of a 2x2 block.
        let gray = |v: u8| [v, v, v, 255];
        let src: Vec<u8> = [gray(0), gray(100), gray(200), gray(40), gray(100), gray(200), gray(0), gray(40)].concat();
        assert_eq!(downscale(&src, 4, 2, 2, 1), [100, 100, 100, 255, 70, 70, 70, 255]);
        // An odd edge is trimmed, not resampled.
        let odd: Vec<u8> = (0..3 * 3).flat_map(|i| gray(i as u8)).collect();
        assert_eq!(downscale(&odd, 3, 3, 2, 2), [gray(0), gray(1), gray(3), gray(4)].concat());
    }

    // Live: captures the primary monitor for a moment. `cargo test --example voice_join -- --ignored capture_windows`
    #[test]
    #[ignore]
    fn captures_primary_monitor_and_lists_windows() {
        let captured = start(ShareSource::Screen(0), 30, 1280, 720, true).unwrap();
        let frame = captured.frames.recv_timeout(Duration::from_secs(5)).expect("a frame within 5 s");
        assert!(frame.width <= 1280 && frame.height <= 720 && frame.width.is_multiple_of(2));
        assert_eq!(frame.bgra.len(), (frame.width * frame.height * 4) as usize);
        eprintln!("captured {}x{}", frame.width, frame.height);
        let screens = screens().unwrap();
        eprintln!("screens: {:?}", screens.iter().map(|s| (s.id, &s.name, s.width, s.height)).collect::<Vec<_>>());
        assert!(!screens.is_empty());
        let still = thumbnail(ShareSource::Screen(screens[0].id), 512).unwrap();
        assert!(still.width <= 512 && still.bgra.len() == (still.width * still.height * 4) as usize);
        eprintln!("thumbnail {}x{}", still.width, still.height);
        let windows = shareable_windows().unwrap();
        eprintln!("{} shareable windows, e.g. {:?}", windows.len(), windows.first().map(|w| (&w.app_name, &w.title)));
        assert!(windows.iter().all(|w| w.pid != std::process::id() as i32));
    }

    // Live: both loopback modes open, run for a moment and stop cleanly. Samples only flow while
    // something plays, so this reports how many arrived rather than requiring any.
    #[test]
    #[ignore]
    fn process_loopback_starts_in_both_modes() {
        for target in [AudioTarget::AllExcept(std::process::id()), AudioTarget::Only(std::process::id())] {
            let (tx, rx) = channel();
            let stop = Arc::new(AtomicBool::new(false));
            let thread = { let stop = stop.clone(); std::thread::spawn(move || loopback(target, &tx, &stop)) };
            std::thread::sleep(Duration::from_millis(700));
            stop.store(true, Ordering::Relaxed);
            thread.join().unwrap().unwrap();
            let samples: Vec<f32> = rx.try_iter().flatten().collect();
            let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            eprintln!("loopback {}: {} samples, peak {peak:.3}", if matches!(target, AudioTarget::Only(_)) { "only us" } else { "all but us" }, samples.len());
            if matches!(target, AudioTarget::Only(_)) {
                assert_eq!(peak, 0.0, "this test process plays nothing, so its own tree must be silent");
            }
        }
    }
}
