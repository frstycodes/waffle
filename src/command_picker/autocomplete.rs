// An app's autocomplete answers, the debounce that asks for them, and what the caret's option offers.

use super::*;

// AUTOCOMPLETE

// An app's answers for the option the caret is in: Discord caches them per query and drops the cache
// when the other options' values change.
#[derive(Default)]
pub(super) struct Autocomplete {
    // `command_line::autocomplete_context` the answers belong to.
    pub(super) context: String,
    pub(super) answers: HashMap<String, Vec<Choice>>,
    pub(super) failed: HashSet<String>,
    // Requests in flight by nonce: their context and query.
    pub(super) awaiting: HashMap<String, (String, String)>,
    pub(super) debounce: Debounce,
    pub(super) timer: Option<Task<()>>,
}

impl Autocomplete {
    pub(super) fn follow(&mut self, context: &str) {
        if self.context != context {
            self.context = context.to_owned();
            self.answers.clear();
            self.failed.clear();
        }
    }
}

/// lodash's `debounce(…, wait, {leading: true, trailing: true})`: the first call fires at once, calls
/// within `wait` of the last one wait, and the end of a pause fires once more if any did.
#[derive(Default, Debug)]
pub(crate) struct Debounce {
    // When the pause ends, while one is running.
    pub until: Option<Instant>,
    // A call came during the pause.
    trailing: bool,
}

impl Debounce {
    /// True when this call fires now.
    pub(crate) fn call(&mut self, now: Instant, wait: Duration) -> bool {
        let pausing = self.until.is_some_and(|until| now < until);
        self.until = Some(now + wait);
        self.trailing = pausing;
        !pausing
    }

    /// The pause ended: true when a call is owed.
    pub(crate) fn expire(&mut self) -> bool {
        self.until = None;
        std::mem::take(&mut self.trailing)
    }
}

pub(super) enum Suggestions<'a> {
    Loading,
    Failed,
    Ready(&'a [Choice]),
}

// The option the caret is in, when its app autocompletes it: the arg, the query and the cache context.
pub(super) struct AutocompleteTarget {
    pub(super) arg: usize,
    pub(super) query: String,
    pub(super) context: String,
}

impl Waffle {
    pub(super) fn autocomplete_target(&self, typed: &Typed) -> Option<AutocompleteTarget> {
        let (n, arg) = typed.arg()?;
        typed.option(arg).autocomplete.then_some(())?;
        let context = command_line::autocomplete_context(typed.command(), &typed.line, n, typed.text());
        Some(AutocompleteTarget { arg: n, query: typed.text()[arg.value.clone()].to_owned(), context })
    }

    pub(super) fn suggestions(&self, typed: &Typed) -> Option<Suggestions<'_>> {
        let target = self.autocomplete_target(typed)?;
        let ac = &self.command_picker.autocomplete;
        if ac.context != target.context {
            return Some(Suggestions::Loading);
        }
        Some(match ac.answers.get(&target.query) {
            Some(choices) => Suggestions::Ready(choices),
            None if ac.failed.contains(&target.query) => Suggestions::Failed,
            None => Suggestions::Loading,
        })
    }

    pub(super) fn schedule_autocomplete(&mut self, cx: &mut Context<Self>) {
        let target = self.typed_command(cx).and_then(|t| self.autocomplete_target(&t));
        let ac = &mut self.command_picker.autocomplete;
        let Some(target) = target else {
            ac.timer = None;
            return;
        };
        ac.follow(&target.context);
        let now = Instant::now();
        let fire = ac.debounce.call(now, AUTOCOMPLETE_DEBOUNCE);
        ac.timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOCOMPLETE_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if this.command_picker.autocomplete.debounce.expire() {
                    this.request_autocomplete(cx);
                }
            })
            .ok();
        }));
        if fire {
            self.request_autocomplete(cx);
        }
    }

    // A type-4 body straight to Discord, unless the cache has this query: the answer lands in
    // `on_autocomplete`, matched by nonce.
    pub(super) fn request_autocomplete(&mut self, cx: &mut Context<Self>) {
        let (Some(client), Some((_, channel_id, guild_id))) = (self.client.clone(), self.command_scope()) else { return };
        let Some(typed) = self.typed_command(cx) else { return };
        let Some(target) = self.autocomplete_target(&typed) else { return };
        let ac = &self.command_picker.autocomplete;
        if ac.context == target.context && ac.answers.contains_key(&target.query) {
            return;
        }
        let name = typed.command().options[typed.line.args[target.arg].option].name.clone();
        let values = command_line::autocomplete_values(typed.command(), &typed.line, target.arg, |a| self.arg_value(&typed, a));
        let nonce = crate::nonce();
        let session = self.gateway_session.clone();
        let origin = Origin { channel_id: &channel_id, guild_id: guild_id.as_deref(), session_id: &session, nonce: &nonce };
        let body = app_commands::command_body(typed.command(), &values, Some(&name), &origin);
        let ac = &mut self.command_picker.autocomplete;
        ac.failed.remove(&target.query);
        ac.awaiting.insert(nonce.clone(), (target.context, target.query));
        cx.spawn(async move |this, cx| {
            if client.interact(body).await.is_err() {
                this.update(cx, |this, cx| this.autocomplete_failed(&nonce, cx)).ok();
            }
        })
        .detach(); // lint-allow(detached-task): a one-shot interact call holding a WeakEntity; it ends when the request returns
        self.redraw(Column::Main, cx);
    }

    pub(crate) fn on_autocomplete(&mut self, nonce: String, choices: Vec<Choice>, cx: &mut Context<Self>) {
        let ac = &mut self.command_picker.autocomplete;
        let Some((context, query)) = ac.awaiting.remove(&nonce) else { return };
        if context == ac.context {
            ac.answers.insert(query, choices);
            self.command_picker.highlighted = 0;
        }
        self.redraw(Column::Main, cx);
    }

    /// INTERACTION_FAILURE (or a refused POST) for an autocomplete request: "Loading options failed".
    pub(crate) fn autocomplete_failed(&mut self, nonce: &str, cx: &mut Context<Self>) {
        let ac = &mut self.command_picker.autocomplete;
        let Some((context, query)) = ac.awaiting.remove(nonce) else { return };
        if context == ac.context {
            ac.failed.insert(query);
        }
        self.redraw(Column::Main, cx);
    }

    // Demo scenes stand in for an app's autocomplete answer for the caret's option: `None` fails it.
    #[cfg(debug_assertions)]
    pub(crate) fn suggest_for_option(&mut self, choices: Option<Vec<Choice>>, cx: &mut Context<Self>) {
        let Some(target) = self.typed_command(cx).and_then(|t| self.autocomplete_target(&t)) else { return };
        let ac = &mut self.command_picker.autocomplete;
        ac.timer = None;
        ac.follow(&target.context);
        match choices {
            Some(choices) => {
                ac.answers.insert(target.query, choices);
            }
            None => {
                ac.failed.insert(target.query);
            }
        }
        self.redraw(Column::Main, cx);
    }
}

// Discord's attachment pill reads this until a file is chosen.
pub(super) const ATTACH_FILE: &str = "Please attach a file";

// What the caret's option offers: a fixed choice, an app's suggestion, True/False, a person, channel or
// role, or a file to attach.
pub(super) struct OptionChoice {
    pub(super) label: String,
    pub(super) value: Value,
    // People, channels and roles go in as mention chips.
    pub(super) token: Option<InlineToken>,
    pub(super) lead: ChoiceLead,
    // Picking opens the file picker.
    pub(super) attach: bool,
}

pub(super) enum ChoiceLead {
    None,
    Suggest(SuggestLead),
    Role(u32),
    Glyph(Glyph),
}

impl OptionChoice {
    pub(super) fn plain(label: &str, value: Value) -> Self {
        Self { label: label.to_owned(), value, token: None, lead: ChoiceLead::None, attach: false }
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}
