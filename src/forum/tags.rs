// Edit Tags on a post's menu: the entry, each tick, and the save with its rollback.
use super::*;

impl Waffle {
    // Edit Tags on a post's menu, for its author and Manage Threads: each tick saves at once, and at
    // five the unticked ones are disabled.
    pub(crate) fn edit_tags_entry(&self, post: &Channel, manage: bool, owner: bool) -> Option<Entry> {
        if !(manage || owner) {
            return None;
        }
        let forum = self.forum_channel(post.parent_id.as_deref()?).filter(|f| f.kind == ChannelKind::Forum)?;
        let applied = post.thread.as_ref().map(|t| t.applied_tags.clone()).unwrap_or_default();
        let full = applied.len() >= MAX_TAGS;
        let rows: Vec<Entry> = forum
            .props
            .tags
            .iter()
            .filter(|t| manage || !t.moderated)
            .map(|tag| {
                let on = applied.contains(&tag.id);
                let (post_id, tag_id) = (post.id.clone(), tag.id.clone());
                let row = tag_item(tag, move |this, _, cx| this.toggle_post_tag(&post_id, &tag_id, cx)).checked(on);
                (if full && !on { row.disabled() } else { row }).entry()
            })
            .collect();
        (!rows.is_empty()).then(|| submenu("Edit Tags", rows).icon(Glyph::Tag).entry())
    }

    pub(super) fn toggle_post_tag(&mut self, post_id: &str, tag_id: &str, cx: &mut Context<Self>) {
        let Some(post) = self.forum_post(post_id).or_else(|| self.channel.clone().filter(|c| c.id == post_id)) else { return };
        let meta = post.thread.clone().unwrap_or_default();
        let mut tags = meta.applied_tags.clone();
        match tags.iter().position(|t| t == tag_id) {
            Some(at) => drop(tags.remove(at)),
            None if tags.len() < MAX_TAGS => tags.push(tag_id.to_owned()),
            None => return,
        }
        self.set_post_tags(post_id, meta.applied_tags, tags, meta.archived, cx);
    }

    // Discord reopens an archived post before changing its tags (unarchiveThreadIfNecessary).
    pub(super) fn set_post_tags(&mut self, post_id: &str, before: Vec<String>, tags: Vec<String>, archived: bool, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else { return };
        let applied = tags.clone();
        self.update_post(post_id, |p| {
            if let Some(meta) = p.thread.as_mut() {
                meta.applied_tags = applied.clone();
                meta.archived = false;
            }
        });
        cx.notify(); // lint-allow(global-notify): a post's tags draw in the forum list and the open post's header, in two columns
        let id = post_id.to_owned();
        cx.spawn(async move |this, cx| {
            let path = format!("/channels/{id}");
            let reopened = match archived {
                true => client.call(Method::PATCH, path.clone(), Some(json!({ "archived": false }))).await.map(|_| ()),
                false => Ok(()),
            };
            let result = match reopened {
                Ok(()) => client.call(Method::PATCH, path, Some(json!({ "applied_tags": tags }))).await.map(|_| ()),
                Err(error) => Err(error),
            };
            let Err(error) = result else { return };
            this.update(cx, |this, cx| {
                this.update_post(&id, |p| {
                    if let Some(meta) = p.thread.as_mut() {
                        meta.applied_tags = before.clone();
                        meta.archived = archived;
                    }
                });
                this.show_toast(error, cx);
            })
            .ok();
        })
        .detach(); // lint-allow(detached-task): ends itself once its request or timer resolves, and holds only a weak handle
    }
}
