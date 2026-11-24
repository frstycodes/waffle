// Waffle's app icon choices: the mark in several media (design/waffle.html), picked in Settings › Appearance.
// The PNGs are written by tools/app_icons.py.
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppIcon {
    Peek,
    Sticker,
    Riso,
    Blueprint,
    // Last because it also takes any icon no longer offered, so an old pick can't fail the whole prefs
    // file, and serde wants that catch-all at the end.
    #[default]
    #[serde(other)]
    Paper,
}

impl AppIcon {
    pub const ALL: [AppIcon; 5] = [AppIcon::Paper, AppIcon::Peek, AppIcon::Sticker, AppIcon::Riso, AppIcon::Blueprint];

    pub fn label(self) -> &'static str {
        match self {
            AppIcon::Paper => "Paper",
            AppIcon::Peek => "Peek",
            AppIcon::Sticker => "Sticker",
            AppIcon::Riso => "Riso",
            AppIcon::Blueprint => "Blueprint",
        }
    }

    // The 512px PNG's asset path, served by `icons::Assets`.
    pub fn asset(self) -> &'static str {
        BRAND_ICONS[self as usize].0
    }

    pub fn bytes(self) -> &'static [u8] {
        BRAND_ICONS[self as usize].1
    }

    // Shows this icon now (Dock; window and tray on Windows) and while Waffle is closed (the bundle's
    // icon on macOS; Start menu, desktop and pinned shortcuts on Windows).
    pub fn apply(self) {
        let custom = (self != AppIcon::default()).then(|| self.bytes());
        #[cfg(target_os = "macos")]
        {
            macos::set_dock_icon(self.bytes());
            macos::set_bundle_icon(custom);
        }
        #[cfg(target_os = "windows")]
        {
            crate::tray::set_icon(self.bytes());
            windows::set_window_icon(self.bytes());
            windows::set_shortcut_icons(custom);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = custom;
    }
}

// Gives the main window the picked icon; its own class icon is the build's.
#[cfg(target_os = "windows")]
pub fn attach_window(hwnd: ::windows::Win32::Foundation::HWND) {
    windows::attach(hwnd, crate::settings::Prefs::load().app_icon.bytes());
}

// Order matches `AppIcon`.
pub const BRAND_ICONS: [(&str, &[u8]); 5] = [
    ("brand/icons/peek.png", include_bytes!("../assets/brand/icons/peek.png")),
    ("brand/icons/sticker.png", include_bytes!("../assets/brand/icons/sticker.png")),
    ("brand/icons/riso.png", include_bytes!("../assets/brand/icons/riso.png")),
    ("brand/icons/blueprint.png", include_bytes!("../assets/brand/icons/blueprint.png")),
    ("brand/icons/paper.png", include_bytes!("../assets/brand/icons/paper.png")),
];

// `png` redrawn at `px` square, as PNG bytes.
#[cfg(any(target_os = "windows", test))]
fn png_at(png: &[u8], px: u32) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
    let sized = image::imageops::resize(&image, px, px, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(&sized, px, px, image::ExtendedColorType::Rgba8).ok()?;
    Some(out)
}

// An .ico holding `png` at the sizes Explorer asks for, each entry stored as PNG.
#[cfg(any(target_os = "windows", test))]
fn ico(png: &[u8]) -> Option<Vec<u8>> {
    const SIZES: [u32; 4] = [16, 32, 48, 256];
    let images = SIZES.map(|px| png_at(png, px));
    let mut out = vec![0, 0, 1, 0, SIZES.len() as u8, 0];
    let mut offset = 6 + 16 * SIZES.len();
    for (px, image) in SIZES.iter().zip(&images) {
        let len = image.as_ref()?.len();
        // A width or height of 256 is written as 0.
        let side = (px % 256) as u8;
        out.extend([side, side, 0, 0, 1, 0, 32, 0]);
        out.extend((len as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += len;
    }
    images.into_iter().flatten().for_each(|image| out.extend(image));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_its_png_in_order() {
        for icon in AppIcon::ALL {
            let (path, bytes) = BRAND_ICONS[icon as usize];
            assert!(path.ends_with(&format!("{}.png", serde_json::to_string(&icon).unwrap().trim_matches('"'))), "{path}");
            assert_eq!(&bytes[1..4], b"PNG", "{path} is a PNG");
        }
        let saved: AppIcon = serde_json::from_str("\"peek\"").unwrap();
        assert_eq!(saved, AppIcon::Peek);
    }

    #[test]
    fn the_ico_points_each_entry_at_its_png() {
        let ico = ico(AppIcon::Peek.bytes()).expect("ico");
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 4, 0]);
        for entry in ico[6..6 + 4 * 16].chunks(16) {
            let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
            assert_eq!(&ico[offset + 1..offset + 4], b"PNG");
        }
    }

    #[test]
    fn a_retired_icon_falls_back_to_the_default() {
        let saved: AppIcon = serde_json::from_str("\"bone\"").unwrap();
        assert_eq!(saved, AppIcon::default());
    }
}
