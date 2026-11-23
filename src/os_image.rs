// Pictures the OS draws for a file: a downscaled thumbnail of an image, or the icon its file manager
// gives the type. Blocking: call from a background task, never the UI thread.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use gpui_kit::RenderImage;

/// `side` pixels square: an image cropped to fill, anything else as its type's icon. On Windows a video
/// also gets its first frame, as Explorer shows it.
pub fn thumbnail(path: &Path, side: u32) -> Option<Arc<RenderImage>> {
    let (width, height, bgra) = platform::thumbnail(path, side)?;
    render(width, height, bgra)
}

/// The type's icon, cached by extension.
pub fn file_icon(path: &Path, side: u32) -> Option<Arc<RenderImage>> {
    static ICONS: OnceLock<IconCache> = OnceLock::new();
    let key = (path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase(), side);
    let icons = ICONS.get_or_init(Default::default);
    if let Some(icon) = icons.lock().ok()?.get(&key) {
        return Some(icon.clone());
    }
    let (width, height, bgra) = platform::icon(path, side)?;
    let icon = render(width, height, bgra)?;
    icons.lock().ok()?.insert(key, icon.clone());
    Some(icon)
}

// By (extension, side).
type IconCache = Mutex<HashMap<(String, u32), Arc<RenderImage>>>;

// gpui's render images take BGRA bytes as they are.
fn render(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::Path;

    use objc2::AllocAnyThread as _;
    use objc2_app_kit::{NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage, NSWorkspace};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    pub fn thumbnail(path: &Path, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path.to_str()?))?;
        let size = image.size();
        if size.width <= 0. || size.height <= 0. {
            return None;
        }
        // Cover: the centred square of the shorter edge.
        let edge = size.width.min(size.height);
        let crop = NSRect::new(NSPoint::new((size.width - edge) / 2., (size.height - edge) / 2.), NSSize::new(edge, edge));
        draw(&image, side, crop)
    }

    pub fn icon(path: &Path, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        let icon = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path.to_str()?));
        draw(&icon, side, NSRect::ZERO)
    }

    // `image`'s `from` rect (zero: all of it) drawn into a `side`-pixel square we own, as BGRA. The same
    // drawing as voice::capture::app_icon, which the voice example compiles without this module.
    fn draw(image: &NSImage, side: u32, from: NSRect) -> Option<(u32, u32, Vec<u8>)> {
        let side = side.max(1) as isize;
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
        let to = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(side as f64, side as f64));
        image.drawInRect_fromRect_operation_fraction(to, from, NSCompositingOperation::Copy, 1.0);
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
        Some((side as u32, side as u32, bgra))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{os::windows::ffi::OsStrExt as _, path::Path};

    use windows::{
        Win32::{
            Foundation::SIZE,
            Graphics::Gdi::{BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ},
            System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT},
        },
        core::PCWSTR,
    };

    // Explorer's own picture: a thumbnail where the type has one (images, videos), else the icon.
    pub fn thumbnail(path: &Path, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        shell_image(path, side, SIIGBF_RESIZETOFIT)
    }

    pub fn icon(path: &Path, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        shell_image(path, side, SIIGBF_ICONONLY)
    }

    fn shell_image(path: &Path, side: u32, flags: SIIGBF) -> Option<(u32, u32, Vec<u8>)> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: COM calls on this thread with a NUL-terminated path; the bitmap is freed below.
        unsafe {
            // Already initialised on this thread (in either mode) is fine: the shell item works in both.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
            let bitmap = factory.GetImage(SIZE { cx: side as i32, cy: side as i32 }, flags).ok()?;
            let pixels = bgra(bitmap);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            pixels
        }
    }

    // SAFETY (caller): `bitmap` is a live HBITMAP.
    unsafe fn bgra(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
        let mut info = BITMAP::default();
        if unsafe { GetObjectW(HGDIOBJ(bitmap.0), size_of::<BITMAP>() as i32, Some((&raw mut info).cast())) } == 0 {
            return None;
        }
        let (width, height) = (info.bmWidth.max(1) as u32, info.bmHeight.unsigned_abs().max(1));
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negative: rows top-down.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut out = vec![0u8; (width * height * 4) as usize];
        let dc = unsafe { CreateCompatibleDC(None) };
        let rows = unsafe { GetDIBits(dc, bitmap, 0, height, Some(out.as_mut_ptr().cast()), &mut header, DIB_RGB_COLORS) };
        let _ = unsafe { DeleteDC(dc) };
        (rows > 0).then_some((width, height, out))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use std::path::Path;

    pub fn thumbnail(_: &Path, _: u32) -> Option<(u32, u32, Vec<u8>)> {
        None
    }

    pub fn icon(_: &Path, _: u32) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
}
