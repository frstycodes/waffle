# Waffle (dango)

## Pointer: nothing floats without occluding

gpui hit-tests every hitbox under the pointer, front to back, until one occludes. An element drawn over others (popover, hover card, menu card, floating bar, toast, PiP, overlay button row, anything `absolute()`/`anchored()`/`deferred()` over content) passes hover and clicks through to whatever is behind it unless it blocks them.

- Every floating surface gets `.id(...)` and `.occlude()` on its outermost element, in the same edit that creates it. Use `.block_mouse_except_scroll()` instead only when the content under it must keep scrolling (a header over a list).
- A backdrop that occludes covers the surface only if the surface sits inside it; a card beside its backdrop still needs its own.
- Decoration inside an element that already handles the pointer (a badge, a scrim, a label on a tile) needs nothing. A transparent full-window layer (the GIF layer, drag ghosts) must not occlude.
- Hover-to-open surfaces keep themselves open with their own `on_hover`, since occluding takes the hover from the row under them.
- Check before calling it done: hover and click the surface's edges and its body over something interactive; nothing behind may light up or respond.

## Illustrations: Discord-style spot art, Waffle's own

Where Discord leads a dialog, empty state or first-run screen with a spot illustration (the voice status dialog's teapot, "No one's around to play with Wumpus"), Waffle does too, with its own art.

- Generate with Codex's image tool (`codex exec -s workspace-write --image=<reference.png> - < prompt.txt`; `--image` before the prompt swallows it). Save transparent PNGs to `assets/illustrations/<feature>.png`, prompt beside it as `<feature>.prompt.txt` so the art can be redrawn.
- One look across the set: polished stylised 3D-ish objects, soft light, three-quarter view, breakfast and café things from Waffle's world. Honey `#EFA831` and amber `#BF7D06` lead, cream and graphite `#1d1f24` support; no purple or blue, no text, no logos, no background tile. Reuse an existing illustration as the reference image so new ones match.
- Every piece reads on both the dark and the light surface; check both before shipping.
- Layout follows Discord's: art centred above the title, title a playful question or line, one muted subtitle. Copy stays Discord's.
