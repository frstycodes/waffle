// The camera device, held by the voice session rather than one connection, so a reconnect keeps it open
// (no light blinking off, no frozen self-view) and only hands its frames to the new connection.

use std::sync::{Arc, Mutex, mpsc::SyncSender};

use super::{Shared, VideoSource, camera, video::VideoFrame};

pub(super) struct HeldCamera {
    id: String,
    _capture: camera::CameraCapture,
    // Where frames go to be sent: the current connection's send loop, replaced on reconnect.
    sink: Arc<Mutex<Option<SyncSender<Arc<VideoFrame>>>>>,
}

// Sends `id`'s frames to `to` from now on, opening the device only if it isn't the one already held. The
// send loop that fed on the camera before ends as its sender is replaced.
pub(super) fn attach(shared: &Arc<Shared>, id: &str, size: (u32, u32), to: SyncSender<Arc<VideoFrame>>) -> Result<(), String> {
    let mut held = shared.camera_feed.lock().map_err(|_| "Camera poisoned".to_owned())?;
    if let Some(camera) = held.as_ref().filter(|camera| camera.id == id) {
        *camera.sink.lock().map_err(|_| "Camera poisoned".to_owned())? = Some(to);
        return Ok(());
    }
    // The old device closes before the new one opens.
    *held = None;
    let sink = Arc::new(Mutex::new(Some(to)));
    let (feed, me, weak) = (sink.clone(), shared.user_id.clone(), Arc::downgrade(shared));
    let capture = camera::start(id, size, move |frame| {
        let frame = Arc::new(frame);
        if let Some(to) = feed.lock().ok().as_ref().and_then(|to| to.as_ref()) {
            let _ = to.try_send(frame.clone());
        }
        if let Some(shared) = weak.upgrade() {
            shared.publish_frame(&me, VideoSource::Camera, frame);
        }
    })?;
    *held = Some(HeldCamera { id: id.to_owned(), _capture: capture, sink });
    Ok(())
}

// The camera turned off: the device closes.
pub(super) fn release(shared: &Shared) {
    if let Ok(mut held) = shared.camera_feed.lock() {
        *held = None;
    }
}
