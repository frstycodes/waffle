// Files waiting in the composer: what each is, whether Discord will take it, and what a paste holds.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
};

use gpui_kit::{ClipboardEntry, ClipboardItem, ImageFormat};

use crate::discord::{MAX_FILES, Upload};

#[derive(Clone, Debug, PartialEq)]
pub struct PendingFile {
    pub path: PathBuf,
    // What the message names it; Modify Attachment can change it before sending.
    pub name: String,
    pub size: u64,
    pub spoiler: bool,
    // Alt text, for pictures and videos.
    pub description: Option<String>,
}

impl PendingFile {
    /// None for anything that isn't a readable file (a folder, a dead path).
    pub fn new(path: PathBuf) -> Option<Self> {
        let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(Self { path, name, size: meta.len(), spoiler: false, description: None })
    }

    pub fn kind(&self) -> FileKind {
        FileKind::of(&self.path)
    }

    // The name goes as typed: a spoiler travels as the message's `is_spoiler`, not a `SPOILER_` prefix.
    pub fn upload(&self) -> Upload {
        Upload { path: self.path.clone(), name: self.name.clone(), size: self.size, description: self.description.clone(), spoiler: self.spoiler, sent: Arc::new(AtomicU64::new(0)) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FileKind {
    Image,
    Video,
    Other,
}

impl FileKind {
    pub fn of(path: &Path) -> Self {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "heif" | "avif" | "bmp" | "tif" | "tiff" => Self::Image,
            "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" => Self::Video,
            _ => Self::Other,
        }
    }
}

/// Adds `new` to `pending` within Discord's caps: `limit` bytes a file and MAX_FILES a message. Returns
/// why any were left out, in Discord's words where it has them.
pub fn admit(pending: &mut Vec<PendingFile>, new: Vec<PendingFile>, limit: u64) -> Option<String> {
    let (mut too_big, mut over_count) = (Vec::new(), 0);
    for file in new {
        if pending.iter().any(|p| p.path == file.path) {
            continue;
        }
        if file.size > limit {
            too_big.push(file.name);
            continue;
        }
        if pending.len() >= MAX_FILES {
            over_count += 1;
            continue;
        }
        pending.push(file);
    }
    let limit = crate::chat::format_size(limit);
    let size = match too_big.as_slice() {
        [] => None,
        [name] => Some(format!("{name} is over your {limit} upload limit.")),
        names => Some(format!("{} files are over your {limit} upload limit.", names.len())),
    };
    let count = (over_count > 0).then(|| format!("You can only attach {MAX_FILES} files to a message."));
    match (size, count) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    }
}

#[derive(Debug, PartialEq)]
pub enum Pasted {
    Files(Vec<PathBuf>),
    Image(gpui_kit::Image),
    Text,
}

/// What a Cmd/Ctrl+V brings. Files copied in Finder or Explorer come with their names as text too; the
/// files win. Text stays the text field's own paste.
pub fn classify(item: &ClipboardItem) -> Pasted {
    let entries = item.entries();
    if let Some(paths) = entries.iter().find_map(|e| match e {
        ClipboardEntry::ExternalPaths(p) if !p.paths().is_empty() => Some(p.paths().to_vec()),
        _ => None,
    }) {
        return Pasted::Files(paths);
    }
    match entries.iter().find_map(|e| match e {
        ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    }) {
        Some(image) => Pasted::Image(image),
        None => Pasted::Text,
    }
}

/// A pasted picture as a file to upload, named `image.png` like Discord's own paste. Formats Discord
/// doesn't show inline (TIFF, BMP, SVG) are re-encoded as PNG.
pub fn save_pasted_image(image: &gpui_kit::Image) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("waffle-paste-{}", crate::nonce()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (ext, bytes) = match image.format {
        ImageFormat::Png => ("png", image.bytes.clone()),
        ImageFormat::Jpeg => ("jpg", image.bytes.clone()),
        ImageFormat::Gif => ("gif", image.bytes.clone()),
        ImageFormat::Webp => ("webp", image.bytes.clone()),
        _ => ("png", to_png(&image.bytes)?),
    };
    let path = dir.join(format!("image.{ext}"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let decoded = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let mut out = std::io::Cursor::new(Vec::new());
    decoded.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use gpui_kit::{ClipboardString, ExternalPaths};

    use super::*;

    fn file(name: &str, size: u64) -> PendingFile {
        PendingFile { path: PathBuf::from(format!("/tmp/{name}")), name: name.into(), size, spoiler: false, description: None }
    }

    #[test]
    fn admits_up_to_ten_files() {
        let mut pending = Vec::new();
        let note = admit(&mut pending, (0..12).map(|i| file(&format!("{i}.txt"), 1)).collect(), 10 << 20);
        assert_eq!(pending.len(), MAX_FILES);
        assert_eq!(note.as_deref(), Some("You can only attach 10 files to a message."));
    }

    #[test]
    fn refuses_files_over_the_limit_by_name() {
        let mut pending = Vec::new();
        let note = admit(&mut pending, vec![file("small.png", 1 << 20), file("huge.mov", 11 << 20)], 10 << 20);
        assert_eq!(pending, vec![file("small.png", 1 << 20)]);
        assert_eq!(note.as_deref(), Some("huge.mov is over your 10.0 MB upload limit."));
        let note = admit(&mut pending, vec![file("a.mov", 11 << 20), file("b.mov", 11 << 20)], 10 << 20);
        assert_eq!(note.as_deref(), Some("2 files are over your 10.0 MB upload limit."));
    }

    #[test]
    fn a_file_exactly_at_the_limit_goes() {
        let mut pending = Vec::new();
        assert_eq!(admit(&mut pending, vec![file("edge.bin", 10 << 20)], 10 << 20), None);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn the_same_file_twice_is_added_once() {
        let mut pending = vec![file("a.txt", 1)];
        assert_eq!(admit(&mut pending, vec![file("a.txt", 1)], 10 << 20), None);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn copied_files_win_over_their_names_as_text() {
        let paths = vec![PathBuf::from("/tmp/a.pdf")];
        let item = ClipboardItem { entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(paths.clone().into())), ClipboardEntry::String(ClipboardString::from("a.pdf".to_owned()))] };
        assert_eq!(classify(&item), Pasted::Files(paths));
    }

    #[test]
    fn a_screenshot_pastes_as_an_image() {
        let image = gpui_kit::Image::from_bytes(ImageFormat::Png, vec![1, 2, 3]);
        let item = ClipboardItem { entries: vec![ClipboardEntry::Image(image.clone())] };
        assert_eq!(classify(&item), Pasted::Image(image));
    }

    #[test]
    fn text_stays_text() {
        assert_eq!(classify(&ClipboardItem::new_string("hello".into())), Pasted::Text);
    }

    #[test]
    fn spoilers_keep_their_name() {
        let mut f = file("cat.png", 1);
        f.spoiler = true;
        let upload = f.upload();
        assert_eq!(upload.name, "cat.png");
        assert!(upload.spoiler);
    }

    #[test]
    fn kinds_by_extension() {
        assert_eq!(FileKind::of(Path::new("a.JPG")), FileKind::Image);
        assert_eq!(FileKind::of(Path::new("a.mov")), FileKind::Video);
        assert_eq!(FileKind::of(Path::new("a.zip")), FileKind::Other);
        assert_eq!(FileKind::of(Path::new("Makefile")), FileKind::Other);
    }
}
