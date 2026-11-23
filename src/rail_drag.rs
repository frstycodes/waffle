// Dragging servers and folders in the rail, as in Discord: the top and bottom quarter of a target
// reorder, the middle combines (server on server makes a folder, server on folder joins it). The
// tile itself is carried like any dragged row (drag.rs).
use gpui_kit::Context;

use crate::{
    Waffle,
    discord::Folder,
    sortable::{Finished, Place},
};

// The dragged rail item: a guild id, or `folder-<id>` for a folder.
#[derive(Clone)]
pub struct RailDrag {
    pub key: String,
}

impl Waffle {
    // A rail drag let go of over the rail or a folder: the move the rail was showing. Every rail list
    // drops beside or onto an item; none has a header target.
    pub(crate) fn finish_rail_sort(&mut self, done: Finished, cx: &mut Context<Self>) {
        let Some(hint) = done.hint else { return };
        let Some(target) = hint.target else { return };
        self.drop_on_rail(&done.dragged, &target, hint.place, cx);
    }

    pub(crate) fn drop_on_rail(&mut self, moved: &str, target: &str, drop: Place, cx: &mut Context<Self>) {
        let mut folders = normalize(&self.folders, self.guilds.iter().map(|g| g.id.as_str()));
        if !apply(&mut folders, moved, target, drop, &new_folder_id) {
            return cx.notify(); // lint-allow(global-notify): the landing ghost is drawn by the root over the rail, whose tile was held as a hole under it
        }
        self.folders = folders.clone();
        if let Some(client) = self.client.clone() {
            crate::discord::runtime().spawn(async move { client.set_guild_folders(folders).await });
        }
        cx.notify(); // lint-allow(global-notify): folders show in the rail and in the server menus, and the root draws the landing ghost
    }
}

fn new_folder_id() -> String {
    rand::random::<u32>().to_string()
}

fn is_loose(folder: &Folder) -> bool {
    folder.id.is_empty() || folder.id == "0"
}

fn loose(guild_id: &str) -> Folder {
    Folder { id: String::new(), name: None, color: None, guild_ids: vec![guild_id.to_owned()] }
}

// One entry per loose server, and every server present, so positions map 1:1 onto the rail.
fn normalize<'a>(folders: &[Folder], guild_ids: impl Iterator<Item = &'a str>) -> Vec<Folder> {
    let mut out: Vec<Folder> = Vec::new();
    for folder in folders {
        match is_loose(folder) {
            true => out.extend(folder.guild_ids.iter().map(|g| loose(g))),
            false => out.push(folder.clone()),
        }
    }
    let missing: Vec<Folder> = guild_ids.filter(|id| !out.iter().any(|f| f.guild_ids.iter().any(|g| g == id))).map(loose).collect();
    out.extend(missing);
    out
}

// Returns false when the drop changes nothing.
fn apply(folders: &mut Vec<Folder>, moved: &str, target: &str, drop: Place, new_id: &dyn Fn() -> String) -> bool {
    if moved == target || top_index(folders, target).is_none() {
        return false;
    }
    let moved_ok = match moved.strip_prefix("folder-") {
        Some(id) => move_folder(folders, id, target, drop),
        None => move_guild(folders, moved, target, drop, new_id),
    };
    folders.retain(|f| !f.guild_ids.is_empty());
    moved_ok
}

fn top_index(folders: &[Folder], target: &str) -> Option<usize> {
    match target.strip_prefix("folder-") {
        Some(id) => folders.iter().position(|f| !is_loose(f) && f.id == id),
        None => folders.iter().position(|f| f.guild_ids.iter().any(|g| g == target)),
    }
}

fn move_folder(folders: &mut Vec<Folder>, id: &str, target: &str, drop: Place) -> bool {
    let Some(from) = folders.iter().position(|f| !is_loose(f) && f.id == id) else { return false };
    if folders[from].guild_ids.iter().any(|g| g == target) {
        return false;
    }
    let folder = folders.remove(from);
    let at = top_index(folders, target).unwrap_or(from);
    folders.insert(if drop == Place::After { at + 1 } else { at }, folder);
    true
}

fn move_guild(folders: &mut Vec<Folder>, moved: &str, target: &str, drop: Place, new_id: &dyn Fn() -> String) -> bool {
    for folder in folders.iter_mut() {
        folder.guild_ids.retain(|g| g != moved);
    }
    let Some(at) = top_index(folders, target) else { return false };
    if target.starts_with("folder-") {
        match drop {
            Place::Into => folders[at].guild_ids.push(moved.to_owned()),
            Place::Before => folders.insert(at, loose(moved)),
            Place::After => folders.insert(at + 1, loose(moved)),
        }
        return true;
    }
    if is_loose(&folders[at]) {
        match drop {
            Place::Into => folders[at] = Folder { id: new_id(), name: None, color: None, guild_ids: vec![target.to_owned(), moved.to_owned()] },
            Place::Before => folders.insert(at, loose(moved)),
            Place::After => folders.insert(at + 1, loose(moved)),
        }
        return true;
    }
    let position = folders[at].guild_ids.iter().position(|g| g == target).unwrap_or(0);
    folders[at].guild_ids.insert(if drop == Place::After { position + 1 } else { position }, moved.to_owned());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(folders: &[Folder]) -> String {
        folders
            .iter()
            .map(|f| match is_loose(f) {
                true => f.guild_ids.join(","),
                false => format!("[{}:{}]", f.id, f.guild_ids.join(",")),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn start() -> Vec<Folder> {
        let folder = Folder { id: "9".into(), name: None, color: None, guild_ids: vec!["c".into(), "d".into()] };
        normalize(&[loose("a"), folder], ["a", "b", "c", "d", "e"].into_iter())
    }

    fn run(moved: &str, target: &str, drop: Place) -> String {
        let mut folders = start();
        apply(&mut folders, moved, target, drop, &|| "new".into());
        layout(&folders)
    }

    #[test]
    fn drops() {
        assert_eq!(layout(&start()), "a [9:c,d] b e");
        assert_eq!(run("e", "a", Place::Before), "e a [9:c,d] b");
        assert_eq!(run("a", "b", Place::After), "[9:c,d] b a e");
        assert_eq!(run("b", "a", Place::Into), "[new:a,b] [9:c,d] e");
        assert_eq!(run("e", "folder-9", Place::Into), "a [9:c,d,e] b");
        assert_eq!(run("e", "d", Place::Before), "a [9:c,e,d] b");
        assert_eq!(run("c", "b", Place::After), "a [9:d] b c e");
        assert_eq!(run("folder-9", "e", Place::After), "a b e [9:c,d]");
        assert_eq!(run("folder-9", "c", Place::Before), "a [9:c,d] b e");
        assert_eq!(run("c", "folder-9", Place::Before), "a c [9:d] b e");
        assert_eq!(run("a", "a", Place::Into), "a [9:c,d] b e");
    }
}
