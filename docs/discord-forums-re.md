# Discord forum & media channels: reverse-engineered

Source: Discord web client (build loaded from `https://discord.com/app`, fetched 2026-11-22). The entry bundle is `web.355a76e660bee7a5.js`. Lazy chunks come from its `p.u` map (`assets/<hash>.js`, 6331 chunks), and copy comes from the en-US intl chunks (hash key → string). Everything here is static reading of minified code; none of it was watched running.

Tags: **[O]** observed in code or copy. **[I]** inferred from code shape, not proven.

Main files (scratchpad `bundles/`):
- `chunks/10923e5c961551e9.js`: forum channel view (header, composer, Sort & View, tag popout, list/grid, Older Posts)
- `chunks/6651e9581db5d17c.js`: list post card and post tag row (module 52933)
- `chunks/f1f701412b22365e.js`: thread context menu, including Edit Tags
- `chunks/e751ad05577cfaf5.js`: channel settings Overview (forum fields)
- `chunks/ab257a40674a1225.js`: Create/Edit Tag modal
- `chunks/cce509dd6b324aa0.js`: per-channel view state (module 422844)
- `chunks/5b470f604031e8b3.js`: forum hooks (search debounce, message counts)
- `js/web.*.js`: stores (ForumActivePostStore, ForumSearchStore, ArchivedThreadsStore), REST actions, ChannelRecord

## Channel fields

| Field | Client use |
|---|---|
| `available_tags[] {id,name,emoji_id,emoji_name,moderated}` | Mapped to `availableTags {id,name,emojiId,emojiName,moderated}` **[O]**. Reordered in settings by drag-and-drop, which PATCHes the whole array **[O]**. |
| `applied_tags` | `appliedTags` on the thread **[O]** |
| `default_reaction_emoji {emoji_id,emoji_name}` | `null` clears it **[O]**. Shown on the card only when the first message has no reactions **[O]**. |
| `default_sort_order` | Enum `LATEST_ACTIVITY=0, CREATION_DATE=1` **[O]**. `getDefaultSortOrder()` returns `defaultSortOrder ?? LATEST_ACTIVITY`; game-invite channels always use CREATION_DATE **[O]**. |
| `default_forum_layout` | Enum `DEFAULT=0, LIST=1, GRID=2` **[O]**. `getDefaultLayout()`: media and game-invite channels → GRID; `null` or DEFAULT → LIST **[O]**. |
| `default_tag_setting` | `"match_some"` / `"match_all"`, default `match_some` **[O]** |
| `default_thread_rate_limit_per_user` | Sent in the settings PATCH **[O]**. Copy: "Members will be limited to one message per this interval for any new posts…" **[O]** |
| `rate_limit_per_user` (on the forum) | Post-creation slowmode. Copy: "Members will be restricted to creating one post per this interval…" **[O]** |
| `default_auto_archive_duration` | Fallback `?? 4320` (3 days) **[O]**. Options: 60, 1440, 4320, 10080 min ("1 Hour", "24 Hours", "3 Days", "1 Week") **[O]** |
| `topic` | Post Guidelines. Forum max is 4096 chars (other channels 1024) **[O]** |
| `template` | Text that pre-fills the composer body. The settings field ("Template", max 256) appears only for `isStaff()` users **[O]** |
| `flags` | `REQUIRE_TAG = 1<<4 (16)`, `PINNED = 1<<1` (on a post), `IS_MEDIA_CHANNEL = 1024` **[O]** |

Evidence:
```js
r=((i={})[i.LATEST_ACTIVITY=0]="LATEST_ACTIVITY",i[i.CREATION_DATE=1]="CREATION_DATE",i)
r=((i={})[i.DEFAULT=0]="DEFAULT",i[i.LIST=1]="LIST",i[i.GRID=2]="GRID",i)
getDefaultLayout(){return this.isMediaChannel()||this.isGameInvitesChannel()?l.C.GRID:null==this.defaultForumLayout||this.defaultForumLayout===l.C.DEFAULT?l.C.LIST:this.defaultForumLayout}
```

## States

**Per-user view state** (zustand store, module 422844) **[O]**
- Holds `{layoutType, sortOrder, tagFilter:Set, tagSetting}` per channel id. It is seeded lazily from the channel defaults.
- It is a plain zustand `create`, with no persist middleware. So a user's sort, layout and filter choices last only for the session and are kept per channel. They are not saved per user on the server. **[O]**, though persistence was absent only in this module.

```js
n={layoutType:e.getDefaultLayout(),sortOrder:e.getDefaultSortOrder(),tagFilter:C,tagSetting:e.getDefaultTagSetting()}
setSortOrder=(t,e)=>{this.setChannelState(t,{sortOrder:e}),o.A.setSortOrder(e)};
```

**Active posts** (ForumActivePostStore) **[O]**
- Built from the in-memory threads for the parent: `getThreadsForParent`, fed by gateway `THREAD_LIST_SYNC`.
- Sorted client-side two ways:
  - LATEST_ACTIVITY: by `lastMessageId ?? threadId`, descending
  - CREATION_DATE: by snowflake, descending
- The pinned post always comes first **[I]**: the comparator's `yr()` returns -1. Read as "is pinned".
- Tag filtering is local.
  - `match_some`: any applied tag is in the filter.
  - `match_all`: every filter tag is applied.
  - A post with no tags never matches a non-empty filter.
- `THREAD_CREATE` from another user bumps `newThreadCount`. That shows the "{count} New Post(s)" button, and clicking it resorts. Your own new posts are inserted directly.

**Sections in the list/grid** **[O]**
- 0: header
- 1: active posts, or search results
- 2: archived ("Older Posts")
- 3: loading placeholders

While search results show, the archived section is dropped. The "Older Posts" divider renders only when `section===2 && !isShowingSearchResult && hasActiveThreads`.

**Archived posts** (ArchivedThreadsStore) **[O]**
- Keyed `channelId|sortOrder|tagSetting|sortedTagIds`.
- Pages of 25 (`nextOffset = offset + 25`), with an LRU of 50 keys.
- Loads on mount, then via load-more.
- Needs `READ_MESSAGE_HISTORY`. Without it, the copy is "You do not have permission to view the post history of **#{channelName}**."
- Results are re-sorted locally by the chosen sortOrder.

**Search** (ForumSearchStore) **[O]**
- State per channel: `{query, loading, results}`.
- The title input doubles as the search box. Each keystroke dispatches `FORUM_SEARCH_QUERY_UPDATED` while the composer is closed.
- After a 350 ms debounce it calls `searchForumPosts(guild, channel, query, tagFilter, tagSetting)`.
- Needs `READ_MESSAGE_HISTORY`.
- The matched query text is highlighted in card titles.

```js
let r=setTimeout(async()=>{d.current=l,s.current=t;try{await b.A.searchForumPosts(e.guild_id,e.id,l,t,n)}catch{}},350);
```

**Composer**
- Form state: `name, textAreaState, appliedTags:Set, formOpen, previewing, guidelinesOpen, …` **[O]**
- Shift+Enter in the title opens the form ("Shift + Enter to create a new post") **[O]**
- The first time the form opens in a channel (`hasSeen` is false) and `topic` is non-empty, Post Guidelines auto-open **[O]**
- Esc closes the form **[O]**

## UI and copy

**Header** **[O]**
- Title/search input:
  - Placeholder is "Search or create a post...".
  - It becomes "Title" once the form is open or the body has content.
  - It is "Search" when the user can't post (or in game-invite channels).
  - Max length 100.
- Search status line under the input: "Searching...", "No matching posts", or `{numPosts} post(s) with "{query}"`, plus a "Clear" link.
- When the user can't post: "You do not have permission to post in this channel."
- Buttons beside the input: Post Guidelines (BookCheck icon, shown only when `topic` is set) and "New Post", which becomes "Post" once the form is open.

**Sort & View button** **[O]**
- Label is "Sort & View". On media and game-invite channels it is "Sort".
- Popover aria-label is "Set sort or view". On media channels it is "Change the sort for Media Posts".
- Groups:
  - "Sort by": "Recently Active" | "Date Posted". The Sort & View popover does not say "Recent Activity"; that string is in settings.
  - "View as": "List" | "Gallery". Hidden on media, moderator-report and game-invite channels.
  - "Tag Matching": "Match Some" | "Match All". Shown only if the channel has tags.
  - "Reset to default": restores all three from the channel defaults.
- Radios carry menu ids `sort-by-recent-activity`, `sort-by-date-posted`, `view-as-list`, `view-as-grid`, `match-some`, `match-all`, `reset-all`.

**Tag filter row** **[O]**
- Layout: Sort button, then a divider, then the tag chips (`availableTags`, all of them including moderated), then a dropdown button.
- Multi-select: each chip toggles in or out of `tagFilter`.
- The dropdown button reads "All" when no filter is active, or shows the selected count. Its aria-label is "see all tags" / "see more tags".
- The button is absolutely positioned at the right edge of the last chip on the first row. So the row is one clipped line, and the rest is reached through the popover **[I]** (from a `getBoundingClientRect` loop that breaks at the second row).
- Popover: "Select Tags" heading with a count badge, aria-label "Filter by tags", then the chips, then "Clear all". It lists only usable tags; moderated tags are hidden from non-`MANAGE_THREADS` users.

```js
for(let t of e.children){let{right:e,top:s,height:a}=t.getBoundingClientRect();if(s-l>a)break;e-n>i&&(i=e-n)}t.style.left=`${i}px`
```

**Empty states** **[O]**
- "There are no posts with this tag/these tags yet!"
- "Be the first to start a conversation with this tag/these tags"
- "Be the first to start the conversation!"
- "Make the first post!"

**Post card, list view** (module 173404/52933) **[O]**
- Tag row:
  - "New" badge when unread-new
  - Pin icon (tooltip "Pinned")
  - First **3** applied tags, with matching filter tags highlighted
  - "+N" overflow chip holding the remaining tags
- Title: `heading-lg`, clamped to 2 lines. Muted color unless the post has unreads.
- Snippet: "Author: first message". Alternatives:
  - "Post contains a blocked message"
  - "Post contains an ignored message"
  - "Original message was deleted"
- Footer:
  - Reaction: the top reaction (by `count + burst_count`), or the default reaction emoji when there are none.
  - Message count (ChatIcon). Shows "(N New)" when unread; N caps at "25+" and falls back to "1+".
  - "•" separator.
  - Typing facepile (max 5) when someone is typing; otherwise a relative time.
- The relative time follows the sort: CREATION_DATE shows the post age, otherwise last activity. Its tooltip is "Posted {timestamp}".
- Media thumbnail on the right: first image/video, `minWidth/minHeight 72`, respecting spoiler and obscure state.

```js
let s=(0,h.kt)(n),l=s.slice(void 0,3),a=s.slice(3),i=s.length>3?s.length-3:0,r=n.hasFlag(x.lx.PINNED)
```

**Post card, gallery view** **[O]**
- Masonry grid. Column width is 320–450 with a 16 px gap: `columns = max(floor(t+(w/320-t)/2),1)` where `t = w/450`.
- Top row: author, "posted X ago" (always creation date), "New" badge.
- Body: title (`heading-sm`), then a media area.
  - Aspect ratio is 3:2 for forum posts and 16:9 for media-channel posts.
  - Up to 4 attachments in a mosaic.
  - With no media, the text body shows instead.
- Footer: message count with read state, typing facepile, top reaction or default reaction.
- Card height estimate: `60 + imgH + 24 + 36`.

**Media channel** **[O]**
- Layout is forced to GRID and the sort control is labelled "Sort".
- The composer has "Upload thumbnail" / "Change thumbnail". The thumbnail upload is moved to index 0 on submit.
- Copy: "Add any video, photo, and files to share with members."

**New post composer** **[O]**
- Validation copy:
  - "A title is required" (empty name, or a server `INVALID_FORM_BODY.name` error)
  - "A message is required" (no content, stickers or uploads)
  - "A tag is required to create a post in this channel": shown when `REQUIRE_TAG && appliedTags.size===0 && name && body`. The Post button is disabled in that state.
- Tag picker:
  - Chips; once **5** are applied, unselected chips are disabled.
  - A dropdown with an "Enter a tag..." search.
  - Only usable tags are offered (moderated tags filtered for non-mods).
- Extras: a "Preview" / "Exit Preview" toggle, and a slowmode timer when `rate_limit_per_user > 0`.
- Body prefill: `template.trim()`.
- Guidelines panel: "Post Guidelines" heading with an edit pencil (for MANAGE_CHANNELS) and a close X. Rendered markdown with headings and lists, max-height 220 before an expand control.

```js
return s.useMemo(()=>e.hasFlag(iu.lx.REQUIRE_TAG)&&0===n.size&&l.length>0&&t,[n,l,e,t])
```

**Edit Tags** (post context menu) **[O]**
- Submenu "Edit Tags" with a checkbox per usable tag (with emoji).
- When 5 tags are applied, unchecked entries are disabled.
- Hidden when:
  - the post isn't a forum post
  - the channel has no usable tags
  - the user lacks manage rights (`Id(e)`: owner or MANAGE_THREADS **[I]**)
  - the channel is a moderator-report channel
- Each toggle PATCHes immediately.

```js
p=(0,u.kt)(e),j=p.length>=5, … else{if(j)return;t.add(n)}let i=Array.from(t).map(n=>n.id);s.A.updateForumPostTags(e.id,i)
```

**Settings** (Overview, forum-like channels) **[O]**
- "Post Guidelines" (topic) and "Template" (staff only).
- "Tags": "Help people organize their posts into subcategories by creating a tag." with a "Create Tag" button.
  - The add button is disabled at **20** tags.
  - In game-invite channels, "No Mic" "can be reordered but not edited".
- "Require people to select tags when posting": disabled when every tag is moderated.
- "Default Reaction" and "Hide After Inactivity".
  - Helper text: "New posts will not show in the channel list after being inactive for the specified duration."
  - On a single post: "Posts will not show in the channel list after being inactive for the specified duration."
- "Default Layout" (forum only): "List View" | "Gallery View".
- "Sort Order": "Recent Activity" | "Creation Time".
- "Tag Matching": "Match Some" | "Match All".
- Descriptions end "Members will still be able to toggle between these options."

**Create/Edit Tag modal** **[O]**
- Title is "Create Tag" or "Edit Tag".
- Subtitle: "Create a tag to help people organize their posts into subcategories. Maximum 20 characters."
- Name input: `maxLength:20`, placeholder "Relationships, Advice, Art, Gear, Health, Recipes".
- Emoji picker.
- Checkbox "Only allow moderators to apply tag".
- Buttons: "Remove Tag" (edit only), "Cancel", "Save".

## Limits

| Limit | Value | Status |
|---|---|---|
| Tags per channel | 20 | **[O]** `t.availableTags.length>=20` |
| Tag name length | 20 | **[O]** `maxLength:20` plus copy |
| Tags applied per post | 5 | **[O]** composer and Edit Tags |
| Tags shown on a list card | 3, then "+N" | **[O]** |
| Post title length | 100 | **[O]** `maxLength:100` |
| Guidelines (topic) length | 4096 | **[O]** |
| Template length | 256 | **[O]** |
| Archived page size | 25 | **[O]** |
| Search debounce | 350 ms | **[O]** |
| Facepile | 5 users | **[O]** |
| Unread count cap | "25+" | **[O]** |
| Gallery media | first 4 | **[O]** |
| Unread-count gateway request | ≤180 threads | **[O]** |

## Requests and events

**REST** **[O]**

| Request | Detail |
|---|---|
| `GET /channels/{forum}/threads/search?name=&tag=a,b&tag_setting=match_some\|match_all` | Search. Response: `{threads, members, first_messages, most_recent_messages}` |
| `GET /channels/{forum}/threads/search?archived=true&sort_by=last_message_time&sort_order=desc&limit=25&tag=…&tag_setting=…&offset=N` | Archived posts. Response adds `has_more`. `sort_by` is always `last_message_time`, even when "Date Posted" is chosen; the client re-sorts. |
| `POST /channels/{forum}/post-data {thread_ids:[≤10]}` | Fetches first messages for visible cards → `LOAD_FORUM_POSTS` |
| `POST /channels/{forum}/threads?use_nested_fields=true` | Create a post. Body: `{name, auto_archive_duration: parent.default_auto_archive_duration ?? 4320, applied_tags, message:{content, sticker_ids, flags}}` |
| `PATCH /channels/{thread} {applied_tags:[ids]}` | Edit tags. The post is unarchived first (`unarchiveThreadIfNecessary`). |
| `PATCH /channels/{thread} {flags}` | Pin or unpin. Only one pinned post: "Pin this post?" confirms replacing the existing one. |
| `POST /channels/{forum}/tags {name, emoji_id, emoji_name, moderated}` | Create a tag. `emoji_name` is dropped when `emoji_id` is set. |
| `PUT /channels/{forum}/tags/{tag}` | Edit a tag |
| `DELETE /channels/{forum}/tags/{tag}` | Delete a tag |
| `PATCH /channels/{forum}` | Settings: `topic, rate_limit_per_user, default_thread_rate_limit_per_user, default_auto_archive_duration, default_reaction_emoji, available_tags[], default_sort_order, default_forum_layout, default_tag_setting, flags` |

Tag errors:
- `NON_MODERATED_TAG_REQUIRED`: "Failed to save tag edit" / "There are no tags available that can be set by non-moderators"
- Bad emoji: "Please select a different emoji and try again"
- Delete failure: "Failed to remove tag"

**Gateway** **[O]**
- op 28 `REQUEST_FORUM_UNREADS {guild_id, channel_id, threads:[{thread_id, ack_message_id}]}` → `FORUM_UNREADS`
- `THREAD_LIST_SYNC`, `THREAD_CREATE` (with `isNewlyCreated`), `THREAD_UPDATE`, `THREAD_DELETE`, `CHANNEL_ACK`

**Client dispatches** **[O]**
- `FORUM_SEARCH_QUERY_UPDATED`, `FORUM_SEARCH_START`, `FORUM_SEARCH_SUCCESS`, `FORUM_SEARCH_FAILURE`, `FORUM_SEARCH_CLEAR`
- `LOAD_ARCHIVED_THREADS`, `LOAD_ARCHIVED_THREADS_SUCCESS`, `LOAD_ARCHIVED_THREADS_FAIL`
- `RESORT_THREADS`, `LOAD_FORUM_POSTS`

## Observed vs inferred

- **Observed:** every enum value, limit, copy string and request shape above, plus the per-session (non-persisted) sort, layout and filter state.
- **Inferred:**
  - Pinned posts sort first (the comparator's `yr()` was not opened).
  - The tag-row overflow is a single clipped line.
  - The `Id()` manage check means owner or MANAGE_THREADS.
  - Search matches the thread title only. The client sends only `name`; what the server matches is not visible.
- **Not covered:**
  - Hide-after-inactivity is enforced on the server (auto-archive). The client only picks the duration and shows archived posts under "Older Posts". No client-side inactivity timer was found in the code read.
  - Mobile clients and runtime behaviour were not examined.
