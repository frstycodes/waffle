# Waffle (dango)

These rules bind every session. Hand-written UI values and app-wide redraws are counted by `tools/ui_lint.py`: before you edit a file it tells you what that file still carries, after the edit it blocks anything new, and `cargo test` fails on it (`tests/ui_rules.rs`). `python3 tools/ui_lint.py` lists what's left across the app.

## shadcn names

`crate::ui` mirrors shadcn/ui, so what a model knows of shadcn is what it should write. A component is the shadcn name (`Button`, `Badge`, `Toggle`, `Separator`, `Kbd`, `Skeleton`, `Progress`, `Switch`, `Checkbox`, `RadioGroupItem`, `Slider`, `Tabs`, `ToggleGroup`, `Tooltip`, `Input`, `Textarea`, `InputGroup`, `Select`, `Field`, `Dialog`, `Alert`, `Sonner`, `Empty`, `Popover`, `HoverCard`, `DropdownMenu`, `Card`, `Item`, `Avatar`, `ScrollArea`, `Spinner`, `Table`) with shadcn's props as builder methods: `.variant()`, `.size()`, `.checked()`, `.pressed()`, `.value()`, `.on_value_change()`, `.disabled()`, `.open()`. Variants and sizes are enums with one `match` that says what each looks like (`ButtonVariant::look`), as cva does; where a surface has its own ink (media, art, the call stage) it is a scope (`ButtonScope`), never a lookalike. Colours are `ui::theme` (`background`, `foreground`, `card`, `popover`, `primary`, `secondary`, `muted`, `accent_wash`, `destructive`, `border`, `input`, `ring`, `sidebar::*`; shadcn's `accent` is `accent_wash` because honey is `primary`), mirrored in `design/tokens.css`. Shadcn has no name for Waffle's server tile, presence mark, role chip or speaking ring: they stay domain wrappers around the primitive.

Several names (`Field::new`, `Dialog::new`, `Alert::new`, `Item::new`, …) are façades: the shadcn name over the builder the design system already had, kept as private functions inside `src/ui/`. Reach for the façade; a new component goes in `src/ui/<shadcn_name>.rs` the same way.

## Vendored code: no patching without permission

`vendor/` is other people's framework (gpui and its platform crates), carried with Waffle's patches. Do not edit it unless the user has said so for that change; ask first, say what you would change and why, and wait. An earlier yes covers that change only, never the next one.

- **Style and feel never go in vendor.** Timing, easing, entrance and exit animations, colours, radii, delays, scroll feel, whether links underline: these are Waffle's decisions and live in `src/` (`src/ui/` for the design system). If the framework hard-codes one, the fix is a hook or a setting in the framework and the value on our side, and that framework change needs permission too.
- **A bug in the framework is reported, not quietly patched.** Say what is wrong and where, show the smallest fix, and ask. A patch the user approves carries a `Waffle patch:` comment saying what it fixes, so the next upgrade can drop it.
- **Look before you work around.** If the app does something the app code does not explain (an animation, a delay, a tilt), search `vendor/` for `Waffle patch` before changing `src/`: the cause may be in the framework.

## Design system first

`crate::ui` is the design system: tokens, layout primitives and every component, ported from `design/` (the HTML source of truth). Colours are `palette`, curves and springs `motion`, glyphs `icons::Glyph`. Start UI files with `use crate::ui::prelude::*;`. The catalog at the end of this file lists everything it has.

1. **Look it up before you build it.** Before building any piece of UI, search for it: `py -3 tools/ui_find.py <what it is>` (`make find Q="…"`; `python3` where it's on PATH). It searches every component's description, keywords and options, and the one-offs app code was already let off a rule for. Use what it finds. Composing a lookalike from `div()` (a button, chip, badge, row, field, tooltip, surface, divider, empty state) is a bug, even when the existing one needs a small option added.
2. **Missing? Add it to the design system, then use it.** It goes in `src/ui/` and is re-exported from `src/ui.rs`. It's domain-agnostic (no Discord types; a domain wrapper such as a server tile lives in the app file and wraps the primitive), takes `cx` for colours, and uses tokens only. It gets a section on the design-system page (`src/design_system.rs`, run with `WAVE_DESIGN=1 cargo run`), checked in both themes. It gets a one-sentence comment above it, which becomes its catalog line, and a `// Keywords:` line with the words someone would search for it by (CI and the pre-commit hook block a new component without either, and list what already exists near it); regenerate the catalog with `python3 tools/ui_lint.py --update`.
3. **Everything is a token.** Spacing is gpui's own scale (`gap_2` 8px, `p_1p5` 6px, …; the rem is fixed at 16px). Type is `ui::TEXT_*` with `ui::LEADING_*`, weights are named `FontWeight`s, corners are `.squircle(ui::RADIUS_*)` (`rounded_full` for people and pills), and icons and avatars use `ui::ICON_*` and `ui::AVATAR_*`. Hairlines are `ui::HAIRLINE`, colours are `palette::` roles (light and dark come with them), and durations are `ui::DUR_*` with `motion::` curves. No number goes inside `px()` in app code. A value the scale lacks becomes a named token in `src/ui/tokens.rs`, mirrored in `design/tokens.css`. A one-off layout size (a popover's width) is a named `const` in its file.
4. **The ratchet.** `src/ui/rules.tsv` lists what app code may not write: numbers in `px()`, raw colours, numeric weights, raw floating surfaces, the kit's tooltip, and `cx.notify()` inside `impl Waffle` (an app-wide redraw). `src/ui/rules.baseline` counts what was left from before, per file and rule. Counts only fall: never add to the baseline, migrate the lines you touch, and lock in what fell with `python3 tools/ui_lint.py --update`. A line that genuinely can't comply carries `// lint-allow(<rule>): <reason>` on it or on the line above (`lint-allow(raw-px, raw-color): …` for several). Valid reasons: a size computed from data, a drag ghost that must not occlude, a notify that really changes every column. "It was easier" is not one. An allow without a reason, or with one too thin to act on, is reported. CI fails a PR that raises any rule's baseline total or drops a rule.
5. **Files stay short.** A file holds at most 800 lines of code; blank lines, comments and `#[cfg(test)]` modules don't count. Past 600, every edit warns. A file that was already longer when the rule came in (`main.rs`, `chat.rs`, `discord.rs` and others; see the baseline) may not grow at all, so adding to one means moving a self-contained part (a view, a section, a group of helpers) into its own module in the same change. New code goes in a new file, not a long one. A file that is long by nature (a generated table) carries `// lint-allow(file-lines): <reason>` in its first five lines.
6. **Components, not lookalikes.** The ratchet also matches an element's whole builder chain: a clickable, hovered, rounded div is `custom-button`; bg + shadow is `custom-surface`; an absolute mark on a corner is `custom-mark` (use `ui::badged`); `custom-divider`, `custom-badge`; a raw flex div is `raw-row`/`raw-col`; `.truncate()` is `raw-truncate`; a function named like a component (`*_button`, `*_card`, `*_tile` …) that returns an element built without any `ui::` component is `custom-component`. When the closest component lacks a look, size or slot, add it as a variant there (a `Tone`, a builder option, a slot like `Badged`'s corners), the way shadcn's variants work, rather than rebuilding it. An allow for one of these rules names the `ui::` item it tried and what it lacks (`lint-allow(custom-button): ui::chip has no cycling value`); a reason naming no catalog item is reported.
7. **One-offs are written down.** Every allow is listed by rule in `src/ui/ONE_OFFS.md` (generated; `ui_find` searches it). Search before writing an exception: if the same kind of thing is already there, the second copy belongs in `src/ui/`, and the lint fails two allowed elements that are nearly the same.
8. **Gaps, not margins.** Space between siblings is the parent's `gap_*` on a `ui::row`/`ui::col` or grid; space inside is padding; pushing to the end is `flex_1` or `justify_between`. Margins (`raw-margin`) stay near zero; an optical nudge carries an allow saying why.
9. **Lists are virtualized.** A scrolling element that maps rows into `.children(...)` builds every row each frame (`unvirtualized-list`); use `list`/`uniform_list` with a `ListState`. A list that stays small carries an allow stating how big it can get ("a server's roles, a few dozen at most"), so whoever grows it virtualizes it. The report says how many of the app's lists are virtualized.

## Overflow: nothing spills

A flex child's minimum size is its content's, so long text pushes its row past its parent instead of shrinking. Build every layout as if the data were worst-case.

- Rows and columns are `ui::row()` / `ui::col()`, which shrink across (`min_w_0`) so their text truncates. A column keeps its content's height: one that may shrink in height squeezes its items together inside a scrolling list. The column that stretches to fill a bounded parent is `ui::col().min_h_0().flex_1()`. Scrolling content goes through `ui::scroll_area`, which keeps the list at its content's height.
- In a row, the piece that stretches is `flex_1()` + `min_w_0()`; fixed pieces (icons, avatars, badges, buttons, counts) are `flex_none()`.
- Text that must stay on one line is `ui::line(text)`, or `ui::truncating(key, text)` when the cut text should show in a tooltip. Running text wraps inside a width-bounded parent. Never let a name, title, channel or server label size its container.
- Anything with a variable number of children scrolls: a bounded height (`flex_1` + `min_h_0`, or `max_h`), then `ui::ScrollArea::new` or `list`/`uniform_list`. Popovers cap their height and scroll inside; `ui::floating` keeps them in the window.
- Check before calling it done: the longest real name you can find, a 99+ count, an empty list, a missing avatar, and the window at its minimum width. Nothing may clip mid-glyph, overlap, or push past its parent.

## Pointer: nothing floats without occluding

gpui hit-tests every hitbox under the pointer, front to back, until one occludes. An element drawn over others (popover, hover card, menu card, floating bar, toast, PiP, overlay button row, anything `absolute()`/`anchored()`/`deferred()` over content) passes hover and clicks through to whatever is behind it unless it blocks them.

- Floating surfaces are `ui::Popover::floating(id, content)` or `ui::Popover::floating_at(id, at, corner, content)`: deferred, kept in the window, and occluding. Anything else floating gets `.id(...)` and `.occlude()` on its outermost element, in the same edit that creates it. Use `.block_mouse_except_scroll()` instead only when the content under it must keep scrolling (a header over a list).
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

## Rendering: redraw only what changed

`Waffle` is one big entity, like a React context that every column reads. The window is split into cached views, one per column (`part.rs`): a cached view replays its last drawing until it's notified. Every column observes `Waffle`, so **`cx.notify()` on `Waffle` re-renders all three columns** and everything else `render_root` builds. A notify on a smaller view re-renders that view and its parents, and the cached columns beside it replay.

gpui notifies `window.current_view()`, the view whose render built the element, when a list or `overflow_*_scroll` div scrolls (`list.rs`) and when a `.hover()` style changes (`div.rs`). An element built in `Waffle`'s own render notifies `Waffle`.

- State that one column alone draws (a hover, an open/closed flag) → `self.redraw(Column::X, cx)`, not `cx.notify()`.
- Notify only when what's drawn changed. Check the old value first. High-rate sources (audio levels, video frames, timers, gateway events) never notify unconditionally.
- State with its own update rate (a meter, a playing video, a ticking clock, a text field) gets its own entity, as `VideoPlayer` does; its tick notifies that entity, not `Waffle`. This is the gpui version of splitting a context into store slices.
- Anything that scrolls or animates and is drawn outside a column gets its own view, like `Overlays` in `part.rs`, so its notifies stay local.
- A cached view (`.cached(style)`) is sized by its style only; its content can't size it. Cache fixed regions; give content-sized popovers an uncached view.
- Long lists use `list`/`uniform_list` with a `ListState`, never a div of every row.
- Render runs again on every redraw: derive expensive values when their inputs change and keep them in state, not in render.

The lint enforces this section (each rule's message says what to do; a genuine exception carries `lint-allow(<rule>): <reason>`):
- `render-work`: sorting, parsing, file reads, `cx.new` or `cx.subscribe`/`observe` in a function that builds elements (handlers inside it don't count; they run later).
- `hot-notify`: `cx.notify()` in a pointer-move, hover or scroll handler, or a timer loop, with no check that anything changed first. Hover counts: while a list scrolls, every row passes under a still pointer.
- `waffle-notify`: `cx.notify()` in a free function handed Waffle (`Entity`, `WeakEntity` or `Context<Waffle>`), the app-wide redraw `global-notify` can't see from outside `impl Waffle`.
- `full-refresh`: `window.refresh()`, which re-renders every view past the column caches.
- `render-scan`: a scan of every server, channel or DM (`guilds.iter()`, `dms.iter()`, `place_of`) in a function that builds elements; it runs on every redraw, once per row in a list.
- `main-thread-io`: `std::fs`, `thread::sleep`, `block_on` or blocking HTTP in a function that takes `cx`/`window`, outside `background_spawn`.
- `endless-animation`: a `.repeat()` animation; the allow says when it stops. `detached-task`: `.detach()`; the allow says how the task ends itself.
- `unbounded-growth`: a collection field the app grows and never shrinks, resets or evicts from; cap it, or the allow states the natural bound.
- `unsized-image`: a Discord CDN image without `?size=`; ask for the size it's drawn at.
- `render-probe`: every `impl Render` starts with `crate::perf::probe("Name")`. The counters are off unless `WAFFLE_PERF=1`; `make perf SCENE=…` runs a demo scene with them and memory sampling, and prints any view that redraws more than 30 times a second. Run it when you touch a hot path, not every time.

## Idiomatic Rust and gpui

- A stateless piece of UI is a function returning an element the caller keeps composing (`-> Div`, `-> Stateful<Div>`), like `ui::Separator`. When it has several options, it's a `#[derive(IntoElement)]` struct with builder methods and `RenderOnce`, like `ui::TextField`. An `Entity` exists only to hold state.
- Views talk through gpui: `cx.emit` + `cx.subscribe` for events, `cx.observe` for "it changed". Not `Rc<RefCell<…>>` shared between views.
- Handlers on a view's state use `cx.listener(...)`. Spawned tasks hold a `WeakEntity` and end when `update` fails; `.detach()` a task only when it ends itself, otherwise keep the `Task` in the view so dropping the view cancels it.
- Element ids are stable and unique: keyed by the data's id (`("message", id)`), never by index in a list that reorders.
- No `unwrap`/`expect` on runtime data in render or event paths: guard and return. `expect` is for invariants, with the invariant as its message.
- Enums over boolean pairs and stringly states; `match` exhaustively, no `_ =>` over an enum you own.
- Done means `make check` passes: clippy with warnings as errors, `tools/ui_lint.py --check` and its tests, then `cargo test`. `make help` lists the other commands (run, demo scenes, the design page).

## Finding components

The catalog (`src/ui/CATALOG.md`) and the one-offs (`src/ui/ONE_OFFS.md`) are generated and not loaded here; search them with `tools/ui_find.py`. When you skip the search, the lint runs it for you: a new lookalike or component-named function is blocked, and the block message names the nearest design-system matches.

Hooks: `make hooks` installs `.githooks/pre-commit` (the full check and the ratchet against HEAD, in seconds). CI runs the same on every PR, against the base branch.
