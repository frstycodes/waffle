// Shared chrome and live behaviour for every design-system page.
const root = document.documentElement;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const PAGES = [
  ["index.html", "Overview"],
  ["better.html", "Better than Discord"],
  ["foundations.html", "Foundations"],
  ["accent-candidates.html", "Accent candidates"],
  ["waffle.html", "Waffle"],
  ["icons.html", "Icons"],
  ["window-chrome.html", "Window frame"],
  ["primitives.html", "Primitives"],
  ["navigation.html", "Navigation"],
  ["messages.html", "Messages"],
  ["embeds.html", "Rich embeds"],
  ["voice.html", "Voice and streams"],
  ["overlays.html", "Overlays"],
  ["menus.html", "Context menus"],
  ["panels.html", "Panels and views"],
  ["media.html", "Media and drag"],
  ["auth.html", "Sign in and states"],
  ["settings.html", "Settings"],
  ["profile.html", "Profile and privacy"],
  ["community.html", "Servers and invites"],
  ["server-settings.html", "Server settings"],
  ["channel-settings.html", "Channel settings"],
  ["flows.html", "Flows"],
  ["flows-account.html", "Flows: account"],
  ["flows-messaging.html", "Flows: messaging"],
  ["flows-voice.html", "Flows: voice and calls"],
  ["flows-call.html", "Flows: DM calls"],
  ["flows-community.html", "Flows: people and servers"],
  ["flows-discord-map.html", "Flows: Discord map"],
];
const FONTS = [["Figtree", "Figtree"], ["Gabarito", "Gabarito"], ["Onest", "Onest"], ["Rubik", "Rubik"], ["gg sans", "gg sans (current)"]];
// The mark, tinted with --logo (honey, deeper on light) through a mask, from the file the app ships.
const LOGO = `<span aria-hidden="true" style="display:inline-block;width:24px;height:24px;background:var(--logo);-webkit-mask:url(/assets/brand/waffle-mark.svg) center/contain no-repeat;mask:url(/assets/brand/waffle-mark.svg) center/contain no-repeat"></span>`;

const prefs = (() => { try { return JSON.parse(localStorage.getItem("waffle-ds") || "{}"); } catch { return {}; } })();
// ?theme=light|dark wins, so headless screenshots can pick a theme.
const forced = new URLSearchParams(location.search).get("theme");
if (forced) { prefs.theme = forced; root.dataset.theme = forced; }
function save(key, value) {
  prefs[key] = value;
  try { localStorage.setItem("waffle-ds", JSON.stringify(prefs)); } catch {}
  document.dispatchEvent(new CustomEvent("wave:prefs"));
}

// Header.
const here = location.pathname.split("/").pop() || "index.html";
$("#top").className = "top";
$("#top").innerHTML = `
  <a class="brand" href="index.html">${LOGO}Waffle <span>Design system</span></a>
  <div class="tools">
    <div class="seg" role="group" aria-label="Theme" id="theme">${[["", "System"], ["light", "Light"], ["dark", "Dark"]].map(([v, n]) => `<button aria-pressed="${(prefs.theme || "") === v}" data-v="${v}">${n}</button>`).join("")}</div>
    <select class="select" id="font" aria-label="Typeface">${FONTS.map(([v, n]) => `<option value="${v}" ${prefs.font === v ? "selected" : ""}>${n}</option>`).join("")}</select>
  </div>`;
$("#theme").addEventListener("click", e => {
  const b = e.target.closest("button"); if (!b) return;
  for (const x of $$("#theme button")) x.setAttribute("aria-pressed", x === b);
  b.dataset.v ? root.dataset.theme = b.dataset.v : delete root.dataset.theme;
  save("theme", b.dataset.v);
});
$("#font").addEventListener("change", e => { root.style.setProperty("--font-ui", `"${e.target.value}", system-ui, sans-serif`); save("font", e.target.value); });
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => document.dispatchEvent(new CustomEvent("wave:prefs")));

// ?only=section-id shows one section, so a headless screenshot of a long page can be taken in parts.
const only = new URLSearchParams(location.search).get("only");
if (only) for (const sec of document.querySelectorAll("main > section[id]")) if (!only.split(",").includes(sec.id)) sec.hidden = true;

// Navigation: every page, and the current page's sections under it.
const sections = $$("main > section[id]");
$("#toc").innerHTML = PAGES.map(([href, name]) => {
  const on = href === here;
  const subs = on && sections.length > 1 ? `<div class="sub">${sections.map(s => `<a href="#${s.id}">${($("h2", s) ?? $("h1", s))?.textContent ?? s.id}</a>`).join("")}</div>` : "";
  return `<a class="page${on ? " on" : ""}" href="${href}" ${on ? 'aria-current="page"' : ""}>${name}</a>${subs}`;
}).join("");
const subLinks = $$("#toc .sub a");
const io = new IntersectionObserver(entries => {
  for (const en of entries) if (en.isIntersecting) for (const a of subLinks) a.classList.toggle("on", a.hash === "#" + en.target.id);
}, { rootMargin: "-40% 0px -55% 0px" });
for (const s of sections) io.observe(s);

// Avatars: initials on a hue picked from the name.
function paintAvatars(scope = document) {
  for (const el of $$("[data-av]:not([data-painted])", scope)) {
    const name = el.dataset.av;
    el.style.setProperty("--hue", [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7));
    el.prepend(name === "You" ? "Y" : name[0]);
    el.dataset.painted = "";
  }
}
paintAvatars();

// Live states shared by every page.
document.addEventListener("click", e => {
  const choice = e.target.closest(".choice");
  if (choice) {
    const mark = $("[aria-checked]", choice);
    if (choice.dataset.toggle === "radio") for (const r of $$("[role=radio]", choice.parentElement)) r.setAttribute("aria-checked", r === mark);
    else mark.setAttribute("aria-checked", mark.getAttribute("aria-checked") !== "true");
  }
  const seg = e.target.closest(".seg:not(#theme) button");
  if (seg) for (const b of $$("button", seg.parentElement)) b.setAttribute("aria-pressed", b === seg);
  const tab = e.target.closest("[role=tab]");
  if (tab) for (const t of $$("[role=tab]", tab.closest("[role=tablist]"))) t.setAttribute("aria-selected", t === tab);
  const toggle = e.target.closest("[aria-pressed]:not(.seg button):not(#theme button)");
  if (toggle && !seg) toggle.setAttribute("aria-pressed", toggle.getAttribute("aria-pressed") !== "true");
  const react = e.target.closest(".react:not([aria-label])");
  if (react) {
    const mine = react.classList.toggle("mine");
    const count = react.lastChild; count.textContent = +count.textContent + (mine ? 1 : -1);
  }
  const opener = e.target.closest("[data-open]");
  if (opener) $("#" + opener.dataset.open)?.showModal();
  const closer = e.target.closest("[data-close]");
  if (closer) closer.closest("dialog")?.close();
  if (e.target.tagName === "DIALOG") e.target.close();
  const busy = e.target.closest("[data-loading]");
  if (busy && !busy.dataset.busy) {
    busy.dataset.busy = 1;
    const label = busy.innerHTML;
    busy.innerHTML = `<span class="bars" style="--c: currentColor"><i></i><i></i><i></i></span>${busy.dataset.loading}`;
    setTimeout(() => { busy.innerHTML = label; delete busy.dataset.busy; }, 1400);
  }
});
document.addEventListener("input", e => {
  if (!e.target.matches(".range")) return;
  const r = e.target, p = (r.value - (r.min || 0)) / ((r.max || 100) - (r.min || 0));
  for (const el of [r, r.parentElement]) { el.style.setProperty("--p", p); el.style.setProperty("--v", p * 100 + "%"); }
});
// Each slider draws its two segments around the native thumb. Settings' sensitivity slider draws its own level track.
for (const r of $$(".range")) {
  if (!r.closest(".set-sens") && !r.parentElement.classList.contains("slider")) {
    const wrap = document.createElement("span");
    wrap.className = "slider";
    r.replaceWith(wrap);
    wrap.append(r);
  }
  r.dispatchEvent(new Event("input", { bubbles: true }));
}

// Segmented controls and tab strips: one thumb or underline slides to the chosen option.
const STRIPS = ".seg, .tabs, .pills";
function placeThumb(strip) {
  const on = $('[aria-pressed="true"], [aria-selected="true"]', strip);
  if (!on) return;
  for (const [k, v] of [["--x", on.offsetLeft], ["--y", on.offsetTop], ["--w", on.offsetWidth], ["--h", on.offsetHeight]]) strip.style.setProperty(k, v + "px");
  if (!strip.classList.contains("is-placed")) { strip.classList.add("is-placed"); requestAnimationFrame(() => strip.classList.add("is-sliding")); }
}
const placeAll = () => $$(STRIPS).forEach(placeThumb);
placeAll();
document.fonts?.ready.then(placeAll);
addEventListener("resize", placeAll);
document.addEventListener("wave:prefs", () => requestAnimationFrame(placeAll));
document.addEventListener("click", e => { const strip = e.target.closest(STRIPS); if (strip) placeThumb(strip); });
