// What was under the pointer when the message menu opened: Discord adds its image and link items for it.
use gpui_kit::{ClipboardItem, Context, Image, ImageFormat};

use crate::{
    Waffle,
    discord::runtime,
    menu::{Entry, item},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuTarget {
    Image(String),
    Link(String),
}

impl Waffle {
    // Discord's G7 (Copy Image, Save Image) and G8 (copy and open the link) for the target.
    pub(super) fn target_entries(&self, target: &MenuTarget) -> Vec<Entry> {
        let (url, link_kind) = match target {
            MenuTarget::Image(url) => (url.clone(), "Image Link"),
            MenuTarget::Link(url) => (url.clone(), "Link"),
        };
        let mut out = Vec::new();
        if matches!(target, MenuTarget::Image(_)) {
            let (copy, save) = (url.clone(), url.clone());
            out.push(item("Copy Image", move |this, _, cx| this.copy_image(copy.clone(), cx)).entry());
            out.push(item("Save Image", move |this, _, cx| this.save_image(save.clone(), cx)).entry());
            out.push(Entry::Separator);
        }
        let (copy, open) = (url.clone(), url);
        out.push(item(format!("Copy {link_kind}"), move |this, _, cx| this.copy_text(copy.clone(), cx)).entry());
        out.push(item(format!("Open {link_kind}"), move |_, _, cx| cx.open_url(&open)).entry());
        out
    }

    fn copy_image(&mut self, url: String, cx: &mut Context<Self>) {
        let task = runtime().spawn(fetch(url));
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            this.update(cx, |this, cx| match result {
                Ok((bytes, format)) => cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(format, bytes))),
                Err(error) => this.show_toast(format!("Couldn't copy the image: {error}"), cx),
            })
            .ok();
        })
        // lint-allow(detached-task): ends when the one fetch returns, holding only a WeakEntity
        .detach();
    }

    fn save_image(&mut self, url: String, cx: &mut Context<Self>) {
        let name = file_name(&url);
        self.save_download(url, move |_| name.clone(), cx);
    }

    // Downloads `url`, then asks where to save it under the name `name` gives for its MIME type.
    pub(crate) fn save_download(&mut self, url: String, name: impl Fn(&str) -> String + 'static, cx: &mut Context<Self>) {
        let task = runtime().spawn(fetch_raw(url));
        cx.spawn(async move |this, cx| {
            let fetched = task.await.map_err(|e| e.to_string()).and_then(|r| r);
            let (bytes, mime) = match fetched {
                Ok(got) => got,
                Err(error) => return this.update(cx, |this, cx| this.show_toast(format!("Couldn't download it: {error}"), cx)).unwrap_or(()),
            };
            // Windows sets USERPROFILE, not HOME.
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
            let downloads = home.map(|h| std::path::PathBuf::from(h).join("Downloads")).unwrap_or_default();
            let target = cx.update(|cx| cx.prompt_for_new_path(&downloads, Some(&name(&mime))));
            let Ok(Ok(Some(to))) = target.await else { return };
            let dest = to.clone();
            let result = cx.background_executor().spawn(async move { std::fs::write(&dest, bytes).map_err(|e| e.to_string()) }).await;
            this.update(cx, |this, cx| match result {
                Ok(()) => cx.reveal_path(&to),
                Err(error) => this.show_toast(format!("Couldn't save it: {error}"), cx),
            })
            .ok();
        })
        // lint-allow(detached-task): ends after one fetch, the save dialog's answer and one write, holding only a WeakEntity
        .detach();
    }
}

async fn fetch_raw(url: String) -> Result<(Vec<u8>, String), String> {
    let res = reqwest::get(&url).await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    let mime = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let bytes = res.bytes().await.map_err(|e| e.to_string())?;
    Ok((bytes.to_vec(), mime))
}

async fn fetch(url: String) -> Result<(Vec<u8>, ImageFormat), String> {
    let (bytes, mime) = fetch_raw(url).await?;
    Ok((bytes, ImageFormat::from_mime_type(&mime).unwrap_or(ImageFormat::Png)))
}

// The file name in the URL's path, without its query.
fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("image.png").to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_name_drops_the_query() {
        assert_eq!(super::file_name("https://cdn.discordapp.com/attachments/1/2/cat.png?ex=1&is=2"), "cat.png");
        assert_eq!(super::file_name("https://x/"), "image.png");
    }
}
