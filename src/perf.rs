// Opt-in render counters, for the occasional check that the rules hold at runtime: run with WAFFLE_PERF=1 (`make
// perf SCENE=…`). Each view's render counts itself; about once a second the counts print to stderr, and any view
// redrawing more than STORM times in that second is called out. An idle window prints nothing, since nothing
// renders. Off (the default), a probe is one read of a cached flag.
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Instant,
};

// More redraws a second than a 60Hz animation needs half of: something notifies without a change.
const STORM: u32 = 30;

static ON: OnceLock<bool> = OnceLock::new();
static COUNTS: Mutex<Option<(Instant, HashMap<&'static str, u32>)>> = Mutex::new(None);

fn on() -> bool {
    *ON.get_or_init(|| std::env::var_os("WAFFLE_PERF").is_some())
}

// Counts one render of `view`; every `impl Render` starts with it (tools/ui_lint.py, render-probe).
pub fn probe(view: &'static str) {
    if !on() {
        return;
    }
    let Ok(mut counts) = COUNTS.lock() else { return };
    let (since, views) = counts.get_or_insert_with(|| (Instant::now(), HashMap::new()));
    *views.entry(view).or_default() += 1;
    if since.elapsed().as_secs_f32() < 1. {
        return;
    }
    let mut busiest: Vec<(&str, u32)> = views.drain().collect();
    busiest.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let line: Vec<String> = busiest.iter().map(|(view, n)| format!("{view} {n}")).collect();
    eprintln!("[perf] renders in the last second: {}", line.join(", "));
    for (view, n) in busiest.iter().filter(|(_, n)| *n > STORM) {
        eprintln!("[perf] redraw storm: {view} rendered {n} times; look for a notify that fires without a change");
    }
    *since = Instant::now();
}
