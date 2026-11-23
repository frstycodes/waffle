// Discord's display-name fonts. Discord serves no font table: its web client compiles one in (font id →
// CSS class → font alias → a .woff2 under discord.com/assets). Waffle carries that table as read on
// 2026-11-19, rereads it from Discord's current build at most weekly and downloads each font once (both in
// the background), converts it to TTF, keeps it on disk and registers it with gpui. A font that isn't
// loaded draws the name in the interface font, as Discord's client does for a font id it has no class for.

use std::{
    borrow::Cow,
    collections::HashMap,
    io::Read,
    path::Path,
    sync::{LazyLock, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures::StreamExt;
use gpui_kit::{App, FontWeight, SharedString, Styled};
use regex::Regex;

use crate::discord::NameStyle;

const ORIGIN: &str = "https://discord.com";
const REFRESH: Duration = Duration::from_secs(7 * 24 * 60 * 60);

// font_id, Discord's alias for the face, its file under /assets: Discord's web build of 2026-11-19.
// Ids 1, 2, 5 and 9 (retired) and 11 (the default) have no face.
const TABLE: &[(u32, &str, &str)] = &[
    (3, "Sakura", "33d4f12a85e1f736.woff2"),
    (4, "Jellybean", "e6f5f44abb520735.woff2"),
    (6, "Modern", "c560709c3470bb66.woff2"),
    (7, "Medieval", "52b541f86401a5b6.woff2"),
    (8, "8Bit", "69c735ca5c604de7.woff2"),
    (10, "Vampyre", "8f20cb550d739cea.woff2"),
    (12, "Tempo", "27b34a77ba5d693e.woff2"),
    (13, "Monkey Bars", "45ef50dce38a931a.woff2"),
    (14, "Mainframe", "4841dace333e9054.woff2"),
    (15, "Headbang", "c00df4f25667809b.woff2"),
    (16, "Journal", "aa59ae54ce7e540a.woff2"),
    (17, "Pricedown", "a7bafac436428e6c.woff2"),
];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Entry {
    id: u32,
    alias: String,
    file: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cached {
    // The web bundle's path, which changes with every Discord build.
    build: String,
    checked: u64,
    fonts: Vec<Entry>,
}

// A registered face: its family as the font file names it, and its one weight.
#[derive(Clone)]
struct Face {
    family: SharedString,
    weight: FontWeight,
}

static FACES: LazyLock<RwLock<HashMap<u32, Face>>> = LazyLock::new(Default::default);

// The name in its style's font, at the face's own weight (Discord ships one weight per font), once loaded.
pub(crate) fn styled<E: Styled>(element: E, style: Option<&NameStyle>) -> E {
    let face = style.and_then(|s| FACES.read().ok()?.get(&s.font).cloned());
    match face {
        Some(face) => element.font_family(face.family).font_weight(face.weight),
        None => element,
    }
}

// Registers the faces already on disk before any window opens (a few small files), then fetches what's
// missing and rereads the table when it's a week old, in the background. Faces fetched later register on
// the main thread between frames and the windows redraw. GAP: gpui re-lays out open windows when its
// font set changes, and a scrolled panel can jump then (seen on the profile card), so only a first-ever
// download or a new font can cause it.
pub(crate) fn start(cx: &mut App) {
    let Some(dir) = crate::data_dir().map(|d| d.join("name-fonts")) else { return };
    // lint-allow(main-thread-io): runs once at startup, before any window opens, over a few small files.
    let cached = std::fs::read(dir.join("table.json")).ok().and_then(|b| serde_json::from_slice::<Cached>(&b).ok());
    let table = cached.as_ref().map_or_else(builtin, |c| c.fonts.clone());
    let (on_disk, missing): (Vec<Entry>, Vec<Entry>) = table.iter().cloned().partition(|e| ttf_path(&dir, e).exists());
    // lint-allow(main-thread-io): the same startup read of the fonts already on disk, before any window opens.
    register(on_disk.iter().filter_map(|e| loaded(e, std::fs::read(ttf_path(&dir, e)).ok()?)).collect(), cx);
    let (sender, mut batches) = futures::channel::mpsc::unbounded::<Vec<Loaded>>();
    crate::discord::runtime().spawn(async move {
        let http = reqwest::Client::new();
        let _ = sender.unbounded_send(fetch(&http, &dir, &missing).await);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        if cached.as_ref().is_some_and(|c| now.saturating_sub(c.checked) < REFRESH.as_secs()) {
            return;
        }
        let known = cached.as_ref().map(|c| c.build.clone());
        let (build, fonts) = match read_table(&http, known.as_deref()).await {
            Ok(Some(found)) => found,
            Ok(None) => (known.unwrap_or_default(), table.clone()),
            // Tried again next launch; the table in hand stays.
            Err(e) => return eprintln!("[name fonts] reading Discord's font table: {e}"),
        };
        // lint-allow(main-thread-io): inside the network runtime's own task, never on the UI thread.
        let _ = std::fs::write(dir.join("table.json"), serde_json::to_vec(&Cached { build, checked: now, fonts: fonts.clone() }).unwrap_or_default());
        let new: Vec<Entry> = fonts.into_iter().filter(|f| !table.contains(f)).collect();
        let _ = sender.unbounded_send(fetch(&http, &dir, &new).await);
    });
    cx.spawn(async move |cx| {
        while let Some(batch) = batches.next().await {
            cx.update(|cx| {
                if register(batch, cx) {
                    cx.refresh_windows();
                }
            });
        }
    })
    // lint-allow(detached-task): ends when the font fetcher drops its sender, and holds no view.
    .detach();
}

struct Loaded {
    id: u32,
    face: Face,
    ttf: Vec<u8>,
}

// True when any face was new.
fn register(batch: Vec<Loaded>, cx: &mut App) -> bool {
    let Ok(mut faces) = FACES.write() else { return false };
    let batch: Vec<Loaded> = batch.into_iter().filter(|l| !faces.contains_key(&l.id)).collect();
    if batch.is_empty() {
        return false;
    }
    let ids: Vec<(u32, Face)> = batch.iter().map(|l| (l.id, l.face.clone())).collect();
    if let Err(e) = cx.text_system().add_fonts(batch.into_iter().map(|l| Cow::Owned(l.ttf)).collect()) {
        eprintln!("[name fonts] registering: {e}");
        return false;
    }
    faces.extend(ids);
    true
}

fn builtin() -> Vec<Entry> {
    TABLE.iter().map(|(id, alias, file)| Entry { id: *id, alias: (*alias).into(), file: (*file).into() }).collect()
}

fn ttf_path(dir: &Path, entry: &Entry) -> std::path::PathBuf {
    dir.join(Path::new(&entry.file).with_extension("ttf"))
}

fn loaded(entry: &Entry, ttf: Vec<u8>) -> Option<Loaded> {
    Some(Loaded { id: entry.id, face: face_of(&ttf)?, ttf })
}

// Each face downloaded, converted to TTF and kept on disk.
async fn fetch(http: &reqwest::Client, dir: &Path, entries: &[Entry]) -> Vec<Loaded> {
    let _ = std::fs::create_dir_all(dir);
    let mut fetched = Vec::new();
    for entry in entries {
        match download(http, &entry.file).await {
            Ok(ttf) => {
                let _ = std::fs::write(ttf_path(dir, entry), &ttf);
                fetched.extend(loaded(entry, ttf));
            }
            Err(e) => eprintln!("[name fonts] {}: {e}", entry.alias),
        }
    }
    fetched
}

async fn download(http: &reqwest::Client, file: &str) -> Result<Vec<u8>, String> {
    let woff2 = get(http, &format!("/assets/{file}")).await?;
    wuff::decompress_woff2(&woff2).map_err(|e| format!("{e:?}"))
}

// The face's family (the typographic family when it names one, as macOS and Windows group faces by it)
// and its weight class.
fn face_of(ttf: &[u8]) -> Option<Face> {
    let face = ttf_parser::Face::parse(ttf, 0).ok()?;
    let name = |id| face.names().into_iter().find(|n| n.name_id == id && n.is_unicode()).and_then(|n| n.to_string());
    let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| name(ttf_parser::name_id::FAMILY))?;
    Some(Face { family: family.into(), weight: FontWeight(face.weight().to_number() as f32) })
}

// GET under discord.com, asking for brotli: the web bundle is 12.7 MB raw, 3.6 MB compressed. reqwest is
// built without its decompression features, so the body is unpacked here.
async fn get(http: &reqwest::Client, path: &str) -> Result<Vec<u8>, String> {
    let response = http.get(format!("{ORIGIN}{path}")).header("Accept-Encoding", "br").send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let brotli = response.headers().get("content-encoding").is_some_and(|v| v == "br");
    let body = response.bytes().await.map_err(|e| e.to_string())?;
    if !brotli {
        return Ok(body.to_vec());
    }
    let mut out = Vec::new();
    brotli_decompressor::Decompressor::new(body.as_ref(), 4096).read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

async fn get_text(http: &reqwest::Client, path: &str) -> Result<String, String> {
    String::from_utf8(get(http, path).await?).map_err(|e| e.to_string())
}

// The table from Discord's current web build, with that build's bundle path; None when the build is
// `known`. A table that disagrees with the built-in one on any of its ids is refused.
pub(crate) async fn read_table(http: &reqwest::Client, known: Option<&str>) -> Result<Option<(String, Vec<Entry>)>, String> {
    let html = get_text(http, "/app").await?;
    let asset = |pattern: &str| Regex::new(pattern).ok().and_then(|r| r.find(&html).map(|m| m.as_str().to_owned()));
    let build = asset(r"/assets/web\.[0-9a-f]+\.js").ok_or("no web bundle on /app")?;
    if known == Some(build.as_str()) {
        return Ok(None);
    }
    let bundle = get_text(http, &build).await?;
    // The font classes' stylesheet and the sheets holding @font-face rules, in page order, until every
    // alias the classes name has a face.
    let sheets = Regex::new(r"/assets/[\w.-]+\.css").map_err(|e| e.to_string())?;
    let (mut classes, mut faces) = (None, String::new());
    for sheet in sheets.find_iter(&html).map(|m| m.as_str().to_owned()) {
        let css = get_text(http, &sheet).await?;
        if css.contains("@font-face") {
            faces.push_str(&css);
        }
        if classes.is_none() && css.contains("dnsFont_") {
            classes = Some((sheet, css));
        }
        if let Some((_, css)) = &classes
            && class_aliases(css).values().all(|alias| font_files(&faces).contains_key(alias))
        {
            break;
        }
    }
    let (sheet, classes) = classes.ok_or("no display-name font stylesheet")?;
    // The stylesheet's chunk id names its JS half, which maps the bundle's keys to class names.
    let chunk = sheet.trim_start_matches("/assets/").split('.').next().unwrap_or_default();
    let module = asset(&format!(r"/assets/{chunk}\.[0-9a-f]+\.js")).ok_or("no class-name chunk")?;
    let module = get_text(http, &module).await?;
    let fonts = extract(&bundle, &module, &classes, &faces).ok_or("font table not found in the bundle")?;
    let builtin = builtin();
    let missing: Vec<_> = builtin.iter().filter(|b| !fonts.iter().any(|f| f.id == b.id && f.alias == b.alias)).map(|b| (b.id, b.alias.as_str())).collect();
    if !missing.is_empty() {
        return Err(format!("the bundle's font table lacks {missing:?}; it read {fonts:?}"));
    }
    Ok(Some((build, fonts)))
}

// The four hops, each by the shape it has in Discord's minified build, not by minified names (those
// change every build): the font-id enum and the id → class-key map in the bundle, the key → class map in
// the classes' chunk, class → alias in its stylesheet, alias → file in an @font-face rule.
fn extract(bundle: &str, module: &str, classes: &str, faces: &str) -> Option<Vec<Entry>> {
    // `{[a.x.DEFAULT]:"",[a.x.CHERRY_BOMB]:d.u_,…}`: the default font is the one without a class.
    let anchor = bundle.find(".DEFAULT]:\"\"")?;
    let start = bundle[..anchor].rfind('{')?;
    let end = anchor + bundle[anchor..].find('}')?;
    let keyed = Regex::new(r#"\[[\w$.]+\.([A-Z0-9_]+)\]:[\w$]+(?:\.([\w$]+)|\["([^"]+)"\])"#).ok()?;
    let exports: HashMap<&str, &str> = Regex::new(r#"([\w$]+|"[^"]+"):"([\w-]+)""#).ok()?.captures_iter(module).map(|c| (c.get(1).map_or("", |m| m.as_str().trim_matches('"')), c.get(2).map_or("", |m| m.as_str()))).collect();
    let ids = font_ids(bundle)?;
    let aliases = class_aliases(classes);
    let files = font_files(faces);
    let entries: Vec<Entry> = keyed
        .captures_iter(&bundle[start..end])
        .filter_map(|c| {
            let name = c.get(1)?.as_str();
            let key = c.get(2).or(c.get(3))?.as_str();
            let id = *ids.get(name)?;
            let alias = aliases.get(*exports.get(key)?)?;
            Some(Entry { id, alias: alias.clone(), file: files.get(alias)?.clone() })
        })
        .collect();
    (!entries.is_empty()).then_some(entries)
}

// The font-id enum, `((l={})[l.UNSPECIFIED=0]="UNSPECIFIED",l[l.DEFAULT=11]="DEFAULT",…,l)`, found by its
// default's number: protobuf enum numbers never change, and other enums reuse names (HEXAGON=23 elsewhere).
fn font_ids(bundle: &str) -> Option<HashMap<&str, u32>> {
    let anchor = bundle.find(".DEFAULT=11]=\"DEFAULT\"")?;
    let start = bundle[..anchor].rfind("={})")?;
    let end = anchor + bundle[anchor..].find(')')?;
    let entry = Regex::new(r#"\[[\w$]+\.([A-Z0-9_]+)=(\d+)\]="[A-Z0-9_]+""#).ok()?;
    Some(entry.captures_iter(&bundle[start..end]).filter_map(|c| Some((c.get(1)?.as_str(), c[2].parse().ok()?))).collect())
}

// `.cherryBomb_OAgVZM.cherryBomb_OAgVZM{font-family:Sakura,gg sans,…}` → cherryBomb_OAgVZM: Sakura.
fn class_aliases(css: &str) -> HashMap<String, String> {
    let Ok(rule) = Regex::new(r#"\.([\w-]+)(?:\.[\w-]+)?\{font-family:"?([^,";}]+)"#) else { return HashMap::new() };
    rule.captures_iter(css).filter(|c| c[1].starts_with(|ch: char| ch.is_ascii_lowercase())).map(|c| (c[1].to_owned(), c[2].to_owned())).collect()
}

// `@font-face{…font-family:Tempo;…src:url("/assets/27b3….woff2")…}` → Tempo: 27b3….woff2.
fn font_files(css: &str) -> HashMap<String, String> {
    let (Ok(rule), Ok(family), Ok(src)) = (Regex::new(r"@font-face\{([^}]*)\}"), Regex::new(r#"font-family:"?([^";]+)"#), Regex::new(r#"src:url\("?/assets/([0-9a-f]+\.woff2)"#)) else { return HashMap::new() };
    rule.captures_iter(css).filter_map(|c| Some((family.captures(&c[1])?[1].to_owned(), src.captures(&c[1])?[1].to_owned()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes copied from Discord's web build of 2026-11-19, cut down to one font plus the retired default.
    const BUNDLE: &str = r#"o=((s={})[s.HEXAGON=23]="HEXAGON",s),W=((l={})[l.UNSPECIFIED=0]="UNSPECIFIED",l[l.DEFAULT=11]="DEFAULT",l[l.ZILLA_SLAB=12]="ZILLA_SLAB",l[l.HEXAGON=17]="HEXAGON",l);let c={[a.x.DEFAULT]:"",[a.x.ZILLA_SLAB]:d.FR,[a.x.HEXAGON]:d["q/P"]};"#;
    const MODULE: &str = r#"516064(_){_.exports={zt:"dnsFont_OAgVZM",FR:"zillaSlab_OAgVZM","q/P":"pricedown_OAgVZM"}}"#;
    const CLASSES: &str = r#".dnsFont_OAgVZM{font-synthesis:none}.zillaSlab_OAgVZM.zillaSlab_OAgVZM{font-family:Tempo,gg sans,Arial,sans-serif;letter-spacing:.03em}.pricedown_OAgVZM.pricedown_OAgVZM{font-family:Pricedown,gg sans,Arial,sans-serif}"#;
    const FACES_CSS: &str = r#"@font-face{font-display:swap;font-family:Tempo;font-style:normal;font-weight:600;src:url("/assets/27b34a77ba5d693e.woff2") format("woff2")}@font-face{font-family:Pricedown;font-weight:700;src:url("/assets/a7bafac436428e6c.woff2") format("woff2")}"#;

    #[test]
    fn extracts_the_font_table_from_the_builds_shapes() {
        let table = extract(BUNDLE, MODULE, CLASSES, FACES_CSS).expect("table");
        assert_eq!(
            table,
            [Entry { id: 12, alias: "Tempo".into(), file: "27b34a77ba5d693e.woff2".into() }, Entry { id: 17, alias: "Pricedown".into(), file: "a7bafac436428e6c.woff2".into() }],
            "the default has no class; Hexagon's id comes from the font enum, not the earlier one, and its class (pricedown) isn't its name"
        );
        assert_eq!(extract(BUNDLE, MODULE, CLASSES, ""), None, "no face, no entry");
    }

    // Run by the scheduled CI check: Discord's live build must still yield a table that agrees with the
    // built-in one. Prints the table, so a new font can be added to TABLE from the log.
    #[test]
    #[ignore = "reads discord.com"]
    fn live_build_yields_the_font_table() {
        let found = crate::discord::runtime().block_on(read_table(&reqwest::Client::new(), None)).expect("live font table");
        let (build, fonts) = found.expect("a build");
        println!("{build}");
        for f in &fonts {
            println!("({}, {:?}, {:?}),", f.id, f.alias, f.file);
        }
        let new: Vec<_> = fonts.iter().filter(|f| !TABLE.iter().any(|(id, ..)| *id == f.id)).collect();
        assert!(new.is_empty(), "Discord added fonts; add them to TABLE: {new:?}");
    }
}
