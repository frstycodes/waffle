//! Discord's camera preview ("Ready to video chat?"): your camera's picture before it goes out.

use std::sync::Arc;

use futures::StreamExt;
use gpui_kit::{Context, RenderImage, Task, WeakEntity, Window, div, prelude::*};

use crate::{
    Waffle, palette,
    part::Column,
    render_image,
    ui::{self, RADIUS_MD, Squircle},
    voice,
};

// Its own view, so each frame redraws only the preview. Dropping it closes the camera.
pub(crate) struct CameraPreview {
    waffle: WeakEntity<Waffle>,
    picture: Option<Arc<RenderImage>>,
    _camera: Option<voice::PreviewCamera>,
    _frames: Task<()>,
}

impl CameraPreview {
    // `waffle` is set by the caller: it holds the Always preview video setting.
    fn new(camera: &str, cx: &mut Context<Self>) -> Self {
        let (tx, mut rx) = futures::channel::mpsc::channel::<voice::VideoFrame>(1);
        let opened = voice::preview_camera(camera, move |frame| _ = tx.clone().try_send(frame));
        if let Err(e) = &opened {
            eprintln!("camera preview: {e}");
        }
        let frames = cx.spawn(async move |this, cx| {
            while let Some(frame) = rx.next().await {
                let shown = this.update(cx, |preview, cx| {
                    let Some(image) = render_image(Arc::new(frame)) else { return };
                    if let Some(old) = preview.picture.replace(image) {
                        cx.drop_image(old, None);
                    }
                    cx.notify();
                });
                if shown.is_err() {
                    return;
                }
            }
        });
        Self { waffle: WeakEntity::new_invalid(), picture: None, _camera: opened.ok(), _frames: frames }
    }
}

impl Render for CameraPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::perf::probe("CameraPreview");
        let always = self.waffle.upgrade().is_some_and(|w| w.read(cx).prefs.always_preview_video);
        // A mirror, as our own camera shows everywhere.
        let picture = match self.picture.clone() {
            Some(image) => ui::media(gpui_kit::ImageSource::Render(image), ui::all(RADIUS_MD)).size_full().object_fit(gpui_kit::ObjectFit::Cover).map(crate::call::mirrored).into_any_element(),
            None => ui::row().size_full().justify_center().child(ui::Spinner::new("camera-preview-loading", gpui_kit::white().opacity(0.7), 16.)).into_any_element(),
        };
        let waffle = self.waffle.clone();
        let toggle = ui::row()
            .id("always-preview-video")
            .gap_2()
            .cursor_pointer()
            .child(ui::Checkbox::new("always-preview-video-box").checked(if always { ui::Check::On } else { ui::Check::Off }))
            .child(div().text_size(ui::TEXT_SM).text_color(palette::muted(cx)).child("Always preview video"))
            .on_click(move |_, _, cx| {
                waffle.update(cx, |this, cx| this.update_prefs(cx, |p| p.always_preview_video = !p.always_preview_video)).ok();
            });
        ui::col().gap_3().child(div().w_full().aspect_ratio(16. / 9.).squircle(RADIUS_MD).overflow_hidden().bg(gpui_kit::black()).child(picture)).child(toggle)
    }
}

impl Waffle {
    // Discord's preview before the camera turns on: Turn On Camera sends it, closing the dialog keeps it off.
    pub(crate) fn preview_camera(&mut self, cx: &mut Context<Self>) {
        let Some(camera) = self.prefs.camera.clone().or_else(|| voice::cameras().into_iter().next().map(|c| c.id)) else {
            self.voice_error = Some("No camera found".into());
            return self.redraw(Column::Nav, cx);
        };
        let waffle = cx.weak_entity();
        let preview = cx.new(|cx| CameraPreview { waffle, ..CameraPreview::new(&camera, cx) });
        self.confirm("Ready to video chat?", "", "Turn On Camera", |this, _, cx| this.set_camera_on(true, cx), cx);
        self.set_dialog_view(preview.into());
        if let Some(dialog) = &mut self.dialog {
            dialog.danger = false;
        }
    }
}
