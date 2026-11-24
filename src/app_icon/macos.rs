use objc2::{
    msg_send,
    runtime::{AnyClass, AnyObject},
};

// The running app's Dock tile.
pub fn set_dock_icon(png: &[u8]) {
    // SAFETY: plain AppKit messages on the main thread; a nil image is checked before use.
    unsafe {
        let Some(app_class) = AnyClass::get(c"NSApplication") else { return };
        let image = image(png);
        if image.is_null() {
            return;
        }
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

// The bundle's own icon, which Finder and the Dock show while Waffle isn't running. `None` removes the
// custom icon, back to the AppIcon.icns built in. Only when running from a `.app`; an update swaps in a
// fresh bundle, so this runs again on every launch.
pub fn set_bundle_icon(png: Option<&'static [u8]>) {
    let Some(bundle) = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.parent()?.parent()?.to_path_buf())) else { return };
    if bundle.extension().is_none_or(|ext| ext != "app") {
        return;
    }
    let Some(path) = bundle.to_str().map(str::to_owned) else { return };
    // NSWorkspace writes an `Icon\r` file into the bundle, so keep it off the main thread.
    let _ = std::thread::Builder::new().name("bundle-icon".into()).spawn(move || {
        // SAFETY: setIcon:forFile:options: is documented as callable from any thread; a nil image clears it.
        objc2::rc::autoreleasepool(|_| unsafe {
            let (Some(workspace_class), Some(string_class)) = (AnyClass::get(c"NSWorkspace"), AnyClass::get(c"NSString")) else { return };
            let image = png.map_or(std::ptr::null_mut(), |png| image(png));
            if png.is_some() && image.is_null() {
                return;
            }
            let c_path = std::ffi::CString::new(path).unwrap_or_default();
            let path: *mut AnyObject = msg_send![string_class, stringWithUTF8String: c_path.as_ptr()];
            let workspace: *mut AnyObject = msg_send![workspace_class, sharedWorkspace];
            let _: bool = msg_send![workspace, setIcon: image, forFile: path, options: 0usize];
        });
    });
}

// A +1 NSImage from PNG bytes (never released: one per icon change), or null.
unsafe fn image(png: &[u8]) -> *mut AnyObject {
    let (Some(data_class), Some(image_class)) = (AnyClass::get(c"NSData"), AnyClass::get(c"NSImage")) else { return std::ptr::null_mut() };
    // SAFETY: the caller is in an unsafe block sending plain Foundation messages.
    unsafe {
        let data: *mut AnyObject = msg_send![data_class, dataWithBytes: png.as_ptr() as *const std::ffi::c_void, length: png.len()];
        let image: *mut AnyObject = msg_send![image_class, alloc];
        msg_send![image, initWithData: data]
    }
}
