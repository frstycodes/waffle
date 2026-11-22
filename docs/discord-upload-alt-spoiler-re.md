# Discord: alt text and spoiler on attachments (RE notes)

Source: public web client, `https://discord.com/app`, build `678976`, fetched 2026-11-22 (no auth). The local Electron cache under `~/Library/Application Support/discord/` was not readable from the sandbox, so everything here comes from the web bundles. All `/assets/*.js` chunks were downloaded, including the 6.6k lazy chunks listed in the webpack map in `web.355a76e660bee7a5.js`. English strings come from the en-US message chunk `c30b23bab1166ada.js`, where hashed intl keys (for example `t.eOB2eR`) resolve to their English copy.

Labels: **observed** means seen in the bundle (filename given); **inferred** means my reading, not stated in the code.

## 1. Upload preview tile in the composer

Code: `22fbc347793fb277.js`, also inlined in `web.355a76e660bee7a5.js`.

- **observed**: there are three hover action buttons. In order:
  1. **Spoiler toggle**. Tooltip "Spoiler Attachment" (`cuurzA`), or "Spoiler Clip" for clips. The icon is `EyeIcon` when the attachment isn't a spoiler and `EyeSlashIcon` when it is. Clicking calls `update(channelId, uploadId, draftType, {spoiler: !upload.spoiler})`.
  2. **Modify**. `PencilIcon`, tooltip "Modify Attachment" (`Y8ujqr`). Opens the modal described in section 2.
  3. **Remove**. `TrashIcon`, `dangerous: true`, tooltip "Remove Attachment" (`vN7REz`), or "Remove Clip" for clips.
  ```
  tooltip:k?…MYgdY2:…cuurzA, onClick:()=>{p.A.update(t,o.id,r,{spoiler:!o.spoiler})}, children:o.spoiler?EyeSlashIcon:EyeIcon
  tooltip:k?…V8YlF7:…Y8ujqr, onClick:O, children:PencilIcon
  tooltip:k?…MskAXa:…vN7REz, onClick:R, dangerous:!0, children:TrashIcon
  ```
- **observed**: buttons 1 and 2 render only when `canEdit` (default `true`). Remove always renders.
- **observed**: tags drawn on the tile thumbnail:
  - "Alt" (`QEW81z`) when the description is non-empty.
  - "Spoiler" (`F+x38C`) when it's a spoiler. Image and video tiles show it only after the user reveals the spoiler. Non-media file tiles always show it.
  - The tile's filename caption is `aria-hidden`.
  ```
  null!=t&&""!==t?<span className:T.altTag>{"Alt"}</span>:null, o&&n?<span className:T.altTag>{"Spoiler"}</span>:null
  ```
- **observed**: a spoiler image tile renders blurred inside the shared Spoiler wrapper (`Types.ATTACHMENT`) with click-to-reveal.
- **observed**: the image tile's aria-label is "Open expanded image view for {filename} with description {alt}" (`8TRAzR`), or the form without the description when there's no alt text (`lXoOEZ`).
- **observed**: a new upload starts with `this.spoiler=!1, this.description=null` (`web.355a76e660bee7a5.js`).

## 2. "Modify Attachment" modal

Code: `f893afe5bb6d0cfb.js` (chunk 669865, module 507536). It exports `default` (pre-send) and `ModifyMosaicAttachmentModal` (post-send).

- **observed**: the title is "Modify Attachment" (`Y8ujqr`). The actions are "Cancel" (`ETE/oC`, secondary) and "Save" (`TY+auE`, primary).
- **observed**: fields, in order:
  1. Preview: the image thumbnail, or a file-type icon.
  2. "Filename" (`ILJuBq`), a text input. Pressing Enter submits. It's disabled in the post-send variant (`disableFilename:!0`).
  3. "Description (Alt Text)" (`eOB2eR`), a textarea with placeholder "Add a description" (`RNH1jn`) and **`maxLength:1024`**. It renders only when `isImage || isVideo`.
  4. "Mark as spoiler" (`gsI+xC`), a checkbox. It's hidden when `disableSpoiler`, which forum post thumbnails pass. If the filename starts with `SPOILER_`, the checkbox is forced checked and disabled.
  ```
  R=s.useMemo(()=>S.startsWith(h.MO),[S])   // h.MO === "SPOILER_"
  d.f,{label:…eOB2eR,placeholder:…RNH1jn,value:E,onChange:k,maxLength:1024}
  p.S,{checked:y||R,disabled:R,onChange:L,label:…"gsI+xC"}
  ```
- **observed**: submit returns `{name, description, spoiler}`. Pre-send, that becomes `update(…, {filename:name, description, spoiler})`. Description state initialises to `a??""`, so after one Save the description is `""`, not `null`.
- **observed**: no help text, character counter or error strings in the modal.

## 3. Wire format

### 3a. Upload slot: `POST /channels/{id}/attachments`

- **observed**: the body is `{files:[{filename, file_size, id, is_clip, title?, application_id?, clip_*?}]}`. It carries **no** description or spoiler. The response's `attachments[0].upload_url` and `upload_filename` are stored on the upload (`web.355a76e660bee7a5.js`).
  ```
  getUploadPayload:function(e){…return{filename:t.name,file_size:t.size,id:s().uniqueId(),is_clip:null!=e.clip,…
  o.Bo.post({url:e,body:{files:[n]},…}); this.setUploadedFilename(e.body.attachments[0].upload_filename)
  ```

### 3b. Message create: `POST /channels/{id}/messages` → `attachments[]`

- **observed**: the serializer `OW(upload, index)` in `web.355a76e660bee7a5.js`:
  ```
  a={id:t?.toString()??e.id}; null!=e.description&&(a.description=e.description), a.filename=n??e.filename,
  a.uploaded_filename=e.uploadedFilename, e.spoiler&&(a.is_spoiler=!0), …duration_secs, waveform, is_thumbnail, is_clip…
  ```
  - `id` is the array **index** as a string ("0", "1", …).
  - `description` is sent only when it isn't `null`, so `""` is sent as-is.
  - `filename` is the user-editable name. `uploaded_filename` is the slot's `upload_filename`.
  - `is_spoiler: true` is set only when the upload is a spoiler, and the field is omitted otherwise.
  - **observed**: the client does **not** prepend `SPOILER_` to the filename. Spoiler travels as `is_spoiler`. `SPOILER_` (constant `MO`) is only read: a filename the user types with that prefix forces the checkbox on.
- **observed**: the same serializer feeds scheduled messages (`POST /users/@me/scheduled-messages`, `attachments`) and interactions (`data.attachments`).
- **observed**: received attachments mark a spoiler with attachment `flags & 8` (`IS_SPOILER=8`). The renderer reads the flag, not the filename:
  ```
  spoiler:(0,l.Lt)(e.flags??0,L.sbO.IS_SPOILER)
  ```
  **inferred**: the server sets flag 8 from `is_spoiler` or from a `SPOILER_` filename.

## 4. Editing alt text and spoiler after send

Code: `2420b0c09d1ff9a5.js` (message accessories), `dd476ff1959306ee.js` (mosaic hover bar).

- **observed**: entry point is the hover action bar on each attachment in the message mosaic. Buttons, in order:
  - Pencil, tooltip and aria-label "Modify Attachment" (`Y8ujqr`). Not shown for a clip that is the only mosaic item.
  - Trash, "Delete" (`/XT3ij`).
  - Download, "Download" (`1WjMbC`), for non-visual or AV types.

  Non-mosaic layouts show only an X button, "Remove Message Attachment" (`0+xZH0`).
- **observed**: who can edit: `canEditItem = canDeleteAttachments`, which is `currentUserId === message.author.id && isActiveChannelOrUnarchivableThread && !<prop l>`. Only the author can edit; `MANAGE_MESSAGES` is not enough.
  ```
  N=o===n.author.id&&_&&!l
  ```
- **observed**: the modal is `ModifyMosaicAttachmentModal`: same title, fields and buttons, with the filename shown but disabled. The spoiler checkbox starts from `flags & IS_SPOILER`. The description textarea appears only for `IMAGE`/`VIDEO`.
- **observed**: request is `PATCH /channels/{c}/messages/{m}` with body `{attachments:[…]}`. The array lists **every** current attachment as `{id}`. Only the edited one also gets `description` and `is_spoiler`, and `is_spoiler` is always sent, `true` or `false`. There's no separate flag.
  ```
  r=s.attachments.map(e=>({id:e.id})),a=r.find(e=>e.id===t.id);null!=a&&(a.description=n,a.is_spoiler=i,x.A.patchMessageAttachments(l.id,s.id,r))
  patchMessageAttachments:async(e,t,n)=>(…,s.Bo.patch({url:D.Rsh.MESSAGE(e,t),body:{attachments:n},…}))
  ```
  **inferred**: omitting an attachment from the array deletes it, which is why the client sends all of them.

## 5. Rendering alt text in chat

Code: `web.355a76e660bee7a5.js`, settings `52b6e35ba37f71f0.js`.

- **observed**: the gate is `tm()`: the alt text shows only when **all** of these hold:
  - the "Show image descriptions" setting is on (`textAndImages.viewImageDescriptions`, **default `false`**);
  - `alt` is non-empty;
  - `disableAltTextDisplay` isn't set;
  - the spoiler isn't hidden.
  ```
  function tm(e){…return!n&&e1._z.getSetting()&&null!=t&&""!==t&&!i}
  k=(0,_.c$)("textAndImages","viewImageDescriptions",e=>e?.value??!1,…)
  ```
- **observed**: setting copy: title "Show image descriptions" (`XYvMIX`), subtitle "Image descriptions are used to describe images for screen readers." (`T0rbtM`). It lives in user settings under `CHAT_INLINE_MEDIA_IMAGE_DESCRIPTIONS`.
- **observed**: how it renders depends on the layout:
  - **Mosaic** (multi-attachment, the normal case): a button labelled **"ALT"** (`jCV1Tz`) with aria-label "Image Description (Alt Text)" (`fSiQ3A`). Clicking opens a popout `role="dialog"` with the heading "Image Description (Alt Text)" and the alt text below. Escape returns focus to the button. On video the button shows only while the controls are visible.
  - **Non-mosaic**: the alt text renders under the media as muted caption text (`body-xs/medium`, `text-muted`).
  ```
  "button",{…,"aria-label":…fSiQ3A,…children:eu.intl.string(eu.t.jCV1Tz)}
  !I&&p&&<Text variant:"body-xs/medium",color:"text-muted">{t}</Text>
  ```
- **observed**: the media viewer's "Details" menu lists Filename, size and "Description (Alt Text)" (`0d417d97e2e5417c.js`).
- **observed**: other copy exists in en-US but no web code references it, so it's probably mobile or unused: "Edit Image Description (Alt Text)" (`w7x2t4`), "ALT Text" (`hR3bsc`), "Alt text:" (`piH89s`), and a tip "Make images accessible to everyone. Add a description on upload." (`Q1PZkN`).

## 6. Limits and errors

- **observed**: description `maxLength` is 1024, enforced by the textarea only. There's no client-side error string.
- **observed**: at most 10 attachments per message (`getMaxAttachmentsCount(){return 10}`).

## Gaps

- The local desktop cache wasn't read (sandbox). The desktop build could differ from web build 678976.
- I didn't find a message context-menu entry for editing alt text after send. The hover pencil is the only web entry point I saw. The `w7x2t4` "Edit Image Description (Alt Text)" string has no web caller.
- Server behaviour isn't visible in the client: whether `SPOILER_` filenames get flag 8 server-side, the server-side description limit, and what happens when an attachment is omitted from the PATCH.
- I didn't resolve the `!l` term in the post-send edit permission.
- The date the post-send edit feature shipped can't be determined from the bundle.
