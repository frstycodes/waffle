// Server settings › Members as Discord's members table (docs/discord-moderation-re.md §3): recent members
// until you search, filtered by safety signals and sorted by either date, paged 12 to 100 at a time from
// chunks of `members-search`, each row's moderation in its overflow menu.
use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
    component::{
        ActiveTheme, Icon,
        input::{InputEvent, InputState},
    },
    div,
    prelude::FluentBuilder,
    px,
};
use serde_json::Value;

use super::{
    access::Page,
    api::{self, MemberCursor, MemberSearch, MemberSort, Signal},
    member_bulk::{self, BulkPowers},
    member_dates::{DateColumn, DateMenu},
    perms,
    view::{failed, loading_line, plural, snowflake_ms},
};
use crate::{
    Waffle,
    channel_settings::Load,
    discord::{Role, avatar_url, perm},
    icons::Glyph,
    link_preview::grouped,
    palette,
    ui::{self, Align, Check, Col},
};

pub const PAGE_SIZES: [usize; 4] = [12, 25, 50, 100];
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
const INDEX_RETRIES: u8 = 3;
// User public flag SPAMMER.
const SPAMMER: u64 = 1 << 20;
// Member flags AUTOMOD_QUARANTINED_USERNAME_OR_GUILD_NICKNAME, _BIO and _SERVER_TAG.
const QUARANTINED: u64 = 128 | 256 | 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub username: String,
    pub avatar: String,
    pub bot: bool,
    pub joined_at: Option<DateTime<Utc>>,
    pub roles: Vec<String>,
    pub timed_out_until: Option<DateTime<Utc>>,
    pub unusual_dm_until: Option<DateTime<Utc>>,
    pub flags: u64,
    pub public_flags: u64,
    // None until Discord says how they joined.
    pub join: Option<JoinSource>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JoinSource {
    pub kind: u64,
    pub code: Option<String>,
    pub integration: Option<String>,
}

// The open popover: one of the toolbar's menus, the page-size list, or a row's overflow menu (its user id).
#[derive(Clone, Debug, PartialEq)]
pub enum Pop {
    Signals,
    Sort,
    PageSize,
    Row(String),
    Date(DateColumn),
}

pub struct Members {
    pub search: Entity<InputState>,
    // What the rows answer.
    pub filter: MemberSearch,
    pub rows: Load<Vec<Member>>,
    pub total: u64,
    pub page: usize,
    pub page_size: usize,
    // Discord answered 202: it is building the server's search index.
    pub indexing: bool,
    // A further chunk is on its way.
    pub more: bool,
    pub pop: Option<Pop>,
    pub joined_menu: DateMenu,
    pub account_menu: DateMenu,
    // Picked rows by user id, kept across pages and searches as Discord keeps them.
    pub selected: Vec<String>,
    // A bulk action is on its way.
    pub bulk_busy: bool,
    // Bumped by every new search, so an answer to an older one is dropped.
    pub generation: u64,
    typed: u64,
}

impl Members {
    pub fn new(window: &mut Window, cx: &mut Context<Waffle>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search by username or id"));
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => this.debounce_member_search(cx),
            InputEvent::PressEnter { .. } => this.apply_member_search(cx),
            _ => {}
        })
        .detach();
        let (joined_menu, account_menu) = (DateMenu::new(window, cx), DateMenu::new(window, cx));
        Members { search, filter: MemberSearch::default(), rows: Load::Loading, total: 0, page: 0, page_size: PAGE_SIZES[0], indexing: false, more: false, pop: None, joined_menu, account_menu, selected: Vec::new(), bulk_busy: false, generation: 0, typed: 0 }
    }
}

// The search box's text as Discord reads it: comma-separated, snowflake-shaped terms are user ids and the
// rest usernames, and a query of one character or none lists recent members. Returns (usernames, ids).
pub fn search_terms(text: &str) -> (Vec<String>, Vec<String>) {
    let text = text.trim();
    if text.chars().count() <= 1 {
        return (Vec::new(), Vec::new());
    }
    let (ids, names): (Vec<&str>, Vec<&str>) = text.split(',').map(str::trim).filter(|t| !t.is_empty()).partition(|t| is_snowflake(t));
    (names.into_iter().map(str::to_owned).collect(), ids.into_iter().map(str::to_owned).collect())
}

// Not verified: Discord's own pattern; ids today run 17 to 20 digits.
fn is_snowflake(term: &str) -> bool {
    (17..=20).contains(&term.len()) && term.bytes().all(|b| b.is_ascii_digit())
}

// The signals a member carries now, in the table's order.
pub fn signals(m: &Member, now: DateTime<Utc>) -> Vec<Signal> {
    Signal::ALL
        .into_iter()
        .filter(|s| match s {
            Signal::UnusualDmActivity => m.unusual_dm_until.is_some_and(|t| t.timestamp_millis() >= now.timestamp_millis() - api::UNUSUAL_DM_WINDOW_MS),
            Signal::TimedOut => m.timed_out_until.is_some_and(|t| t > now),
            Signal::UnusualAccountActivity => m.public_flags & SPAMMER != 0,
            Signal::Quarantined => m.flags & QUARANTINED != 0,
        })
        .collect()
}

pub fn join_label(join: Option<&JoinSource>) -> String {
    let Some(join) = join else { return "Unknown".into() };
    let code = join.code.as_deref();
    match (join.kind, code) {
        (1, _) => "Bot invite".into(),
        // Not verified: `integration_type` spelled as the public API's integration types.
        (2, _) => match join.integration.as_deref() {
            Some("twitch") => "Twitch".into(),
            Some("youtube") => "YouTube".into(),
            _ => "Integration".into(),
        },
        (3, _) => "Server discovery".into(),
        (4, _) => "Student hub".into(),
        (5, Some(code)) => code.into(),
        (5, None) => "Invite".into(),
        (6, Some(code)) => format!("{code} (vanity)"),
        (6, None) => "Vanity URL".into(),
        (7, Some(code)) => format!("Manual verification ({code})"),
        (7, None) => "Manual verification".into(),
        (8, _) => "Linked channel".into(),
        _ => "Unknown".into(),
    }
}

// Discord's relative dates: "<1 day ago", then days, months and years.
pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let days = (now - then).num_days().max(0) as u64;
    match days {
        0 => "<1 day ago".into(),
        1..30 => plural(days, "day ago", "days ago"),
        30..365 => plural(days / 30, "month ago", "months ago"),
        _ => plural(days / 365, "year ago", "years ago"),
    }
}

fn time_left(until: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (until - now).num_minutes().max(1);
    match minutes {
        ..=59 => format!("{minutes}m"),
        60..1440 => format!("{}h {}m", minutes / 60, minutes % 60),
        _ => format!("{}d {}h", minutes / 1440, minutes % 1440 / 60),
    }
}

fn time(v: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|t| t.with_timezone(&Utc))
}

// One `members-search` entry: `{member, join_source_type, source_invite_code, ...}`.
pub fn parse_result(v: &Value) -> Option<Member> {
    parse_member(&v["member"], join_source(v))
}

pub fn parse_member(m: &Value, join: Option<JoinSource>) -> Option<Member> {
    let user = &m["user"];
    let id = user["id"].as_str()?.to_owned();
    let username = user["username"].as_str().unwrap_or_default().to_owned();
    let name = m["nick"].as_str().or(user["global_name"].as_str()).unwrap_or(username.as_str()).to_owned();
    Some(Member {
        avatar: avatar_url(&id, user["avatar"].as_str()),
        bot: user["bot"].as_bool().unwrap_or(false),
        joined_at: time(&m["joined_at"]),
        roles: m["roles"].as_array().into_iter().flatten().filter_map(|r| r.as_str().map(str::to_owned)).collect(),
        timed_out_until: time(&m["communication_disabled_until"]),
        unusual_dm_until: time(&m["unusual_dm_activity_until"]),
        flags: m["flags"].as_u64().unwrap_or(0),
        public_flags: user["public_flags"].as_u64().unwrap_or(0),
        join,
        name,
        username,
        id,
    })
}

// From a search entry or a supplemental one; None when the answer left it out.
fn join_source(v: &Value) -> Option<JoinSource> {
    let kind = v.get("join_source_type")?.as_u64()?;
    let text = |k: &str| v[k].as_str().map(str::to_owned);
    Some(JoinSource { kind, code: text("source_invite_code"), integration: text("integration_type") })
}

// A 202 while Discord indexes the server: `{retry_after}` in seconds, and no members.
fn retry_after(body: &Value) -> Option<Duration> {
    if body.get("members").is_some() {
        return None;
    }
    body["retry_after"].as_f64().map(|s| Duration::from_secs_f64(s.clamp(0.5, 30.)))
}

// Discord's client asks for five pages at a time, at least 250 members.
fn chunk_size(page_size: usize) -> usize {
    (5 * page_size).max(250)
}

fn cursor(m: &Member) -> MemberCursor {
    MemberCursor { joined_at_ms: m.joined_at.map_or(0, |t| t.timestamp_millis()), user_id: m.id.clone() }
}

fn sort_label(sort: MemberSort) -> &'static str {
    match sort {
        MemberSort::JoinedNewest => "Member since, newest",
        MemberSort::JoinedOldest => "Member since, oldest",
        MemberSort::AccountNewest => "Joined Discord, newest",
        MemberSort::AccountOldest => "Joined Discord, oldest",
    }
}

const SORTS: [MemberSort; 4] = [MemberSort::JoinedNewest, MemberSort::JoinedOldest, MemberSort::AccountNewest, MemberSort::AccountOldest];

fn signal_label(signal: Signal) -> &'static str {
    match signal {
        Signal::UnusualDmActivity => "Unusual DM activity",
        Signal::TimedOut => "Timed out",
        Signal::UnusualAccountActivity => "Unusual account activity",
        Signal::Quarantined => "Quarantined",
    }
}

// A row's mark: its glyph, whether it takes the danger ink, and its tooltip.
fn signal_mark(signal: Signal, m: &Member, now: DateTime<Utc>) -> (Glyph, bool, String) {
    match signal {
        Signal::UnusualDmActivity => (Glyph::ChatRound, false, "Sent excessive DMs to non-friend server members in the last 24 hours".into()),
        Signal::TimedOut => (Glyph::Timer, true, format!("Timed out for {}", m.timed_out_until.map(|t| time_left(t, now)).unwrap_or_default())),
        Signal::UnusualAccountActivity => (Glyph::UserMinus, true, "Engaged in suspected spam activity".into()),
        Signal::Quarantined => (Glyph::Prohibit, false, "Can't talk in this server until they change their member name".into()),
    }
}

// One members-search request: which search it belongs to, where it starts, its try and its size.
struct Fetch {
    generation: u64,
    after: Option<MemberCursor>,
    attempt: u8,
    limit: usize,
}

// What the signed-in member may do from a row's menu.
#[derive(Clone, Copy, Default)]
struct Powers {
    kick: bool,
    ban: bool,
    timeout: bool,
}

impl Powers {
    fn any(self) -> bool {
        self.kick || self.ban || self.timeout
    }
}

// Everything the table checks before drawing: row menus, bulk actions, and Manage server for the join
// method column and the date filters.
#[derive(Clone, Copy)]
struct Gates {
    powers: Powers,
    bulk: BulkPowers,
    manage_guild: bool,
}

const CHECK_COL: Col = Col::Fixed(20.);

impl Waffle {
    fn members(&mut self) -> Option<&mut Members> {
        self.server_settings.as_mut().map(|v| &mut v.lists.members)
    }

    // Starts over from the first chunk of what `filter` asks for.
    pub(super) fn load_members(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        m.generation += 1;
        (m.rows, m.total, m.page, m.indexing, m.more) = (Load::Loading, 0, 0, false, false);
        let generation = m.generation;
        self.fetch_members(generation, None, 0, cx);
        cx.notify();
    }

    fn fetch_members(&mut self, generation: u64, after: Option<MemberCursor>, attempt: u8, cx: &mut Context<Self>) {
        let Some(view) = &self.server_settings else { return };
        let m = &view.lists.members;
        let limit = chunk_size(m.page_size);
        let call = api::members_search(&view.guild_id, &m.filter, limit, after.as_ref(), Utc::now().timestamp_millis());
        self.server_call(call, move |this, result, cx| this.members_answer(Fetch { generation, after, attempt, limit }, result, cx), cx);
    }

    fn members_answer(&mut self, Fetch { generation, after, attempt, limit }: Fetch, result: Result<Value, String>, cx: &mut Context<Self>) {
        let Some(m) = self.members().filter(|m| m.generation == generation) else { return };
        let body = match result {
            Ok(body) => body,
            Err(error) => {
                m.more = false;
                if after.is_some() {
                    return self.show_toast(error, cx);
                }
                m.rows = Load::Failed(error);
                return;
            }
        };
        if let Some(wait) = retry_after(&body) {
            if attempt >= INDEX_RETRIES {
                (m.indexing, m.more) = (false, false);
                if after.is_none() {
                    m.rows = Load::Failed("Discord is still indexing this server. Try again in a minute.".into());
                }
                return;
            }
            m.indexing = true;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |this, cx| this.fetch_members(generation, after, attempt + 1, cx)).ok();
            })
            .detach();
            return;
        }
        (m.indexing, m.more) = (false, false);
        let chunk: Vec<Member> = body["members"].as_array().into_iter().flatten().filter_map(parse_result).collect();
        let missing: Vec<String> = chunk.iter().filter(|r| r.join.is_none()).map(|r| r.id.clone()).collect();
        let short = chunk.len() < limit;
        match &mut m.rows {
            Load::Ready(rows) if after.is_some() => rows.extend(chunk),
            rows => *rows = Load::Ready(chunk),
        }
        let loaded = if let Load::Ready(rows) = &m.rows { rows.len() as u64 } else { 0 };
        // A chunk that came back short means there is no more, whatever the count said.
        m.total = if short { loaded } else { body["total_result_count"].as_u64().unwrap_or(loaded).max(loaded) };
        if !missing.is_empty() {
            self.load_join_sources(generation, missing, cx);
        }
    }

    // GAP: a failed lookup leaves those rows "Unknown"; the table has no per-cell error.
    fn load_join_sources(&mut self, generation: u64, user_ids: Vec<String>, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.server_call(api::members_supplemental(&guild_id, &user_ids), move |this, result, _| {
            let Ok(body) = result else { return };
            let Some(Load::Ready(rows)) = this.members().filter(|m| m.generation == generation).map(|m| &mut m.rows) else { return };
            for source in body.as_array().into_iter().flatten() {
                let Some(row) = rows.iter_mut().find(|r| source["user_id"].as_str() == Some(r.id.as_str())) else { continue };
                row.join = join_source(source);
            }
        }, cx);
    }

    // Shows page `page`, asking for the next chunk when it runs past what has loaded.
    fn show_members_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        m.page = page;
        let Load::Ready(rows) = &m.rows else { return cx.notify() };
        let needs_more = (page + 1) * m.page_size > rows.len() && (rows.len() as u64) < m.total && !m.more;
        if needs_more {
            let (after, generation) = (rows.last().map(cursor), m.generation);
            m.more = true;
            self.fetch_members(generation, after, 0, cx);
        }
        cx.notify();
    }

    fn debounce_member_search(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        m.typed += 1;
        let typed = m.typed;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                if this.members().is_some_and(|m| m.typed == typed) {
                    this.apply_member_search(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_member_search(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        let (usernames, user_ids) = search_terms(&m.search.read(cx).value());
        if usernames == m.filter.usernames && user_ids == m.filter.user_ids {
            return;
        }
        (m.filter.usernames, m.filter.user_ids) = (usernames, user_ids);
        self.load_members(cx);
    }

    fn toggle_member_signal(&mut self, signal: Signal, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        let on = !m.filter.signals.contains(&signal);
        m.filter.signals = Signal::ALL.into_iter().filter(|s| if *s == signal { on } else { m.filter.signals.contains(s) }).collect();
        self.load_members(cx);
    }

    fn pick_member_sort(&mut self, sort: MemberSort, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        m.pop = None;
        if m.filter.sort != sort {
            m.filter.sort = sort;
            self.load_members(cx);
        }
        cx.notify();
    }

    fn pick_member_page_size(&mut self, size: usize, cx: &mut Context<Self>) {
        let Some(m) = self.members() else { return };
        (m.pop, m.page_size) = (None, size);
        self.show_members_page(0, cx);
    }

    pub(super) fn member_pop(&mut self, pop: Option<Pop>, cx: &mut Context<Self>) {
        if let Some(m) = self.members() {
            m.pop = if pop.is_some() && m.pop == pop { None } else { pop };
        }
        cx.notify();
    }

    fn edit_member_row(&mut self, user_id: &str, edit: impl FnOnce(&mut Load<Vec<Member>>, usize)) {
        let Some(m) = self.members() else { return };
        let Load::Ready(rows) = &m.rows else { return };
        let Some(i) = rows.iter().position(|r| r.id == user_id) else { return };
        edit(&mut m.rows, i);
    }

    // An hour's timeout, or lifting the one they have; the row shows the change at once.
    fn member_timeout(&mut self, user_id: String, name: String, lift: bool, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        let until = (!lift).then(|| Utc::now() + chrono::Duration::hours(1));
        self.member_pop(None, cx);
        let call = api::timeout(&guild_id, &user_id, until.map(|t| t.to_rfc3339()).as_deref());
        self.server_call(call, move |this, result, cx| {
            if let Err(error) = result {
                return this.show_toast(error, cx);
            }
            this.edit_member_row(&user_id, |rows, i| {
                if let Load::Ready(rows) = rows {
                    rows[i].timed_out_until = until;
                }
            });
            if !lift {
                this.offer_undo(format!("Timed out {name} for an hour"), api::timeout(&guild_id, &user_id, None), cx);
            }
        }, cx);
    }

    fn member_ban(&mut self, user_id: String, name: String, cx: &mut Context<Self>) {
        let Some(guild_id) = self.server_settings.as_ref().map(|v| v.guild_id.clone()) else { return };
        self.member_pop(None, cx);
        self.server_call(api::ban(&guild_id, &user_id, 0), move |this, result, cx| {
            if let Some(l) = this.server_settings.as_mut().map(|v| &mut v.lists) {
                l.asking = None;
            }
            if let Err(error) = result {
                return this.show_toast(error, cx);
            }
            this.edit_member_row(&user_id, |rows, i| {
                if let Load::Ready(rows) = rows {
                    rows.remove(i);
                }
            });
            this.offer_undo(format!("Banned {name}"), api::unban(&guild_id, &user_id), cx);
        }, cx);
    }

    // The members group: its toolbar, the table and the pager.
    pub(super) fn members_group(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.server_settings.as_ref() else { return div().into_any_element() };
        let guild_id = view.guild_id.clone();
        let (powers, bulk, manage_guild) = self
            .with_access(&guild_id, |me| (Powers { kick: me.has(perm::KICK_MEMBERS), ban: me.has(perm::BAN_MEMBERS), timeout: me.has(perm::MODERATE_MEMBERS) }, BulkPowers::new(|bits| me.has(bits)), me.has(perms::MANAGE_GUILD)))
            .unwrap_or_default();
        let m = &view.lists.members;
        let title = if m.filter == MemberSearch::default() { "Recent members" } else { "Search results" };
        let body = match &m.rows {
            Load::Loading if m.indexing => loading_line("Before searching, we need to index this server. Give us a bit.", cx),
            Load::Loading => loading_line("Loading members…", cx),
            Load::Failed(error) => failed("Couldn't load members", error, cx),
            Load::Ready(_) => self.members_table(&guild_id, Gates { powers, bulk, manage_guild }, cx),
        };
        let footer = matches!(m.rows, Load::Ready(_)).then(|| self.members_footer(cx));
        ui::settings_group(title, cx).gap(px(12.)).child(self.members_toolbar(cx)).child(body).children(footer).into_any_element()
    }

    fn members_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(m) = self.server_settings.as_ref().map(|v| &v.lists.members) else { return div().into_any_element() };
        let (filter, pop) = (&m.filter, m.pop.clone());
        let caret = || Icon::new(Glyph::ChevronDown).size(px(12.));
        let signals_chip = ui::chip("ss-member-signals", Some(Glyph::Warning), "Signals", !filter.signals.is_empty(), cx).child(caret()).on_click(cx.listener(|this, _, _, cx| this.member_pop(Some(Pop::Signals), cx)));
        let signals_menu = (pop == Some(Pop::Signals)).then(|| {
            let options: Vec<(String, String, bool)> = Signal::ALL.iter().enumerate().map(|(i, s)| (i.to_string(), signal_label(*s).to_owned(), filter.signals.contains(s))).collect();
            let this = cx.entity();
            let toggle = move |id: String, _: &mut Window, cx: &mut App| {
                let Some(signal) = id.parse::<usize>().ok().and_then(|i| Signal::ALL.get(i).copied()) else { return };
                this.update(cx, |this, cx| this.toggle_member_signal(signal, cx));
            };
            ui::check_options("ss-member-signals-menu", &options, 30., toggle, self.member_dismiss(cx), cx)
        });
        let sort_glyph = if matches!(filter.sort, MemberSort::JoinedOldest | MemberSort::AccountOldest) { Glyph::ArrowUp } else { Glyph::ArrowDown };
        let sort_chip = ui::chip("ss-member-sort", Some(sort_glyph), sort_label(filter.sort), false, cx).child(caret()).on_click(cx.listener(|this, _, _, cx| this.member_pop(Some(Pop::Sort), cx)));
        let sort_menu = (pop == Some(Pop::Sort)).then(|| {
            let options: Vec<(String, String)> = SORTS.iter().map(|s| ((*s as u8).to_string(), sort_label(*s).to_owned())).collect();
            let this = cx.entity();
            let pick = move |id: String, _: &mut Window, cx: &mut App| {
                let Some(sort) = SORTS.into_iter().find(|s| (*s as u8).to_string() == id) else { return };
                this.update(cx, |this, cx| this.pick_member_sort(sort, cx));
            };
            ui::select_options("ss-member-sort-menu", &options, &(filter.sort as u8).to_string(), 30., pick, self.member_dismiss(cx), cx)
        });
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex_1().min_w_0().child(ui::search_field("ss-member-search-field", &m.search, cx)))
            .child(div().relative().flex_none().child(signals_chip).children(signals_menu))
            .child(div().relative().flex_none().child(sort_chip).children(sort_menu))
            .into_any_element()
    }

    pub(super) fn member_dismiss(&self, cx: &mut Context<Self>) -> impl Fn(&mut Window, &mut App) + 'static {
        let this = cx.entity();
        move |_, cx| this.update(cx, |this, cx| this.member_pop(None, cx))
    }

    fn members_table(&self, guild_id: &str, Gates { powers, bulk, manage_guild }: Gates, cx: &mut Context<Self>) -> AnyElement {
        let Some(m) = self.server_settings.as_ref().map(|v| &v.lists.members) else { return div().into_any_element() };
        let Load::Ready(rows) = &m.rows else { return div().into_any_element() };
        let shown: Vec<&Member> = rows.iter().skip(m.page * m.page_size).take(m.page_size).collect();
        let selecting = bulk.any();
        let page_ids: Vec<&str> = shown.iter().map(|r| r.id.as_str()).collect();
        let check_all = selecting.then(|| self.member_select_box("ss-member-check-all".into(), member_bulk::page_check(&m.selected, &page_ids), |this, cx| this.toggle_page_selected(cx), cx));
        let label = |text: &'static str| div().truncate().child(text).into_any_element();
        // Discord hides how members joined from anyone without Manage server, and only they can filter by date.
        let columns: Vec<(Col, AnyElement)> = [
            check_all.map(|b| (CHECK_COL, b)),
            Some((Col::Grow { weight: 2., min: 220. }, label("Name"))),
            Some((Col::Grow { weight: 1., min: 120. }, self.date_header(DateColumn::Joined, manage_guild, cx))),
            Some((Col::Grow { weight: 1., min: 120. }, self.date_header(DateColumn::Account, manage_guild, cx))),
            manage_guild.then(|| (Col::Grow { weight: 1., min: 140. }, label("Join method"))),
            Some((Col::Grow { weight: 1., min: 150. }, label("Roles"))),
            Some((Col::Fixed(70.), label("Signals"))),
            Some((Col::Fixed(52.), label(""))),
        ]
        .into_iter()
        .flatten()
        .collect();
        let cols: Vec<Col> = columns.iter().map(|(c, _)| *c).collect();
        let head = ui::table_head(&cols, columns.into_iter().map(|(_, cell)| cell), cx);
        let guild_roles = self.guilds.iter().find(|g| g.id == guild_id).map(|g| g.roles.clone()).unwrap_or_default();
        let now = Utc::now();
        let mut rows: Vec<AnyElement> = Vec::new();
        if shown.is_empty() {
            let note = if m.more { "Loading members…" } else { "No members match this search." };
            rows.push(ui::table_row("ss-members-none", &[Col::Grow { weight: 1., min: 0. }], [div().text_color(palette::muted(cx)).child(note).into_any_element()], cx).into_any_element());
        }
        for member in shown {
            let picked = m.selected.contains(&member.id);
            let mut cells: Vec<AnyElement> = Vec::new();
            if selecting {
                let id = member.id.clone();
                cells.push(self.member_select_box(SharedString::from(format!("ss-member-check-{id}")), if picked { Check::On } else { Check::Off }, move |this, cx| this.toggle_member_selected(id.clone(), cx), cx));
            }
            cells.extend([
                name_cell(member, cx),
                date_cell(format!("ss-ms-{}", member.id), member.joined_at.map_or_else(|| "Unknown".into(), |t| ago(t, now))),
                date_cell(format!("ss-jd-{}", member.id), DateTime::from_timestamp_millis(snowflake_ms(&member.id)).map_or_else(|| "Unknown".into(), |t| ago(t, now))),
            ]);
            if manage_guild {
                cells.push(date_cell(format!("ss-jm-{}", member.id), join_label(member.join.as_ref())));
            }
            cells.push(roles_cell(member, &guild_roles, cx));
            cells.push(signals_cell(member, now, cx));
            cells.push(self.member_actions(member, powers, cx));
            // Right-click opens the row's More menu, as Discord's members table does.
            let pop = Pop::Row(member.id.clone());
            let row = ui::table_row(SharedString::from(format!("ss-member-{}", member.id)), &cols, cells, cx)
                .when(picked, |r| r.bg(palette::selected(cx)))
                .when(powers.any(), |r| {
                    r.on_mouse_down(gpui_kit::MouseButton::Right, cx.listener(move |this, _, _, cx| {
                        if let Some(m) = this.members() {
                            m.pop = Some(pop.clone());
                        }
                        cx.notify();
                    }))
                });
            rows.push(row.into_any_element());
        }
        ui::table("ss-members-table", ui::table_min_width(&cols), head, rows, cx).into_any_element()
    }

    // Open Mod View, then the row's moderation menu.
    fn member_actions(&self, member: &Member, powers: Powers, cx: &mut Context<Self>) -> AnyElement {
        let id = member.id.clone();
        let guild_id = self.server_settings.as_ref().map(|v| v.guild_id.clone()).unwrap_or_default();
        let gated = !self.can_mod_view(&guild_id);
        let user_id = id.clone();
        // Settings cover the window, so they close first; unsaved changes keep them open and Mod View shut.
        let mod_view = ui::icon_button(SharedString::from(format!("ss-modview-{id}")), Glyph::Shield, "Open Mod View", cx).small().disabled(gated).on_click(cx.listener(move |this, _, window, cx| {
            this.close_server_settings(false, cx);
            if this.server_settings.is_none() {
                this.open_mod_view(guild_id.clone(), user_id.clone(), window, cx);
            }
        }));
        let mut cell = div().flex().items_center().gap(px(4.)).child(mod_view);
        if !powers.any() {
            return cell.into_any_element();
        }
        let open = self.server_settings.as_ref().is_some_and(|v| v.lists.members.pop == Some(Pop::Row(id.clone())));
        let pop = Pop::Row(id.clone());
        let more = ui::icon_button(SharedString::from(format!("ss-member-more-{id}")), Glyph::MoreVertical, "More options", cx).small().on_click(cx.listener(move |this, _, _, cx| this.member_pop(Some(pop.clone()), cx)));
        let menu = open.then(|| ui::menu_dropdown(SharedString::from(format!("ss-member-menu-{id}")), 24., Align::End, self.member_menu_rows(member, powers, cx), self.member_dismiss(cx), cx));
        cell = cell.child(div().relative().child(more).children(menu));
        cell.into_any_element()
    }

    fn member_menu_rows(&self, member: &Member, powers: Powers, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (id, name) = (member.id.clone(), member.name.clone());
        let mut rows = Vec::new();
        let row = |key: &str, glyph: Glyph, label: String, cx: &mut Context<Self>| ui::menu_row(SharedString::from(format!("ss-member-{key}-{id}")), cx).child(Icon::new(glyph).size(px(16.))).child(div().flex_1().min_w_0().truncate().child(label));
        if powers.timeout {
            let lift = member.timed_out_until.is_some_and(|t| t > Utc::now());
            let label = if lift { "Remove time out" } else { "Time out for 1 hour" };
            let (id, name) = (id.clone(), name.clone());
            rows.push(row("timeout", Glyph::Timer, label.into(), cx).on_click(cx.listener(move |this, _, _, cx| this.member_timeout(id.clone(), name.clone(), lift, cx))).into_any_element());
        }
        if powers.kick {
            let ask = format!("kick-{id}");
            let asking = self.asking(&ask);
            let label = if asking { format!("Click again to kick {name}") } else { "Kick".into() };
            let (g, user) = (self.server_settings.as_ref().map(|v| v.guild_id.clone()).unwrap_or_default(), id.clone());
            rows.push(
                row("kick", Glyph::DoorExit, label, cx)
                    .danger(cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.asking(&ask) {
                            return this.ask(ask.clone(), cx);
                        }
                        this.member_pop(None, cx);
                        this.act_and_reload(api::kick(&g, &user), Page::Members, cx);
                    }))
                    .into_any_element(),
            );
        }
        if powers.ban {
            let ask = format!("ban-{id}");
            let asking = self.asking(&ask);
            let label = if asking { format!("Click again to ban {name}") } else { "Ban".into() };
            let (user, name) = (id.clone(), name.clone());
            rows.push(
                row("ban", Glyph::Gavel, label, cx)
                    .danger(cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.asking(&ask) {
                            return this.ask(ask.clone(), cx);
                        }
                        this.member_ban(user.clone(), name.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        rows
    }

    fn members_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(m) = self.server_settings.as_ref().map(|v| &v.lists.members) else { return div().into_any_element() };
        let (page, size, total) = (m.page, m.page_size, m.total);
        let size_chip = ui::chip("ss-member-page-size", None, size.to_string(), false, cx).child(Icon::new(Glyph::ChevronDown).size(px(12.))).on_click(cx.listener(|this, _, _, cx| this.member_pop(Some(Pop::PageSize), cx)));
        let size_menu = (m.pop == Some(Pop::PageSize)).then(|| {
            let options: Vec<(String, String)> = PAGE_SIZES.iter().map(|n| (n.to_string(), n.to_string())).collect();
            let this = cx.entity();
            let pick = move |id: String, _: &mut Window, cx: &mut App| {
                let Ok(size) = id.parse() else { return };
                this.update(cx, |this, cx| this.pick_member_page_size(size, cx));
            };
            ui::select_options("ss-member-page-size-menu", &options, &size.to_string(), 30., pick, self.member_dismiss(cx), cx)
        });
        let prev = ui::icon_button("ss-member-prev", Glyph::ChevronLeft, "Previous page", cx).small().disabled(page == 0).on_click(cx.listener(move |this, _, _, cx| this.show_members_page(page.saturating_sub(1), cx)));
        let next = ui::icon_button("ss-member-next", Glyph::ChevronRight, "Next page", cx).small().disabled(((page + 1) * size) as u64 >= total).on_click(cx.listener(move |this, _, _, cx| this.show_members_page(page + 1, cx)));
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(4.))
            .text_color(palette::muted(cx))
            .child("Showing")
            .child(div().relative().flex_none().child(size_chip).children(size_menu))
            .child("members of")
            .child(div().font_weight(FontWeight::SEMIBOLD).text_color(palette::strong(cx)).child(grouped(total)))
            .child(div().flex_1())
            .child(prev)
            .child(next)
            .into_any_element()
    }
}

// Avatar, then the name with the username muted beside it; the username gives way first.
fn name_cell(member: &Member, cx: &App) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_w_0()
        .child(ui::avatar(member.avatar.clone(), 28.))
        .child(ui::truncating(format!("ss-mn-{}", member.id), member.name.clone()).text_color(palette::strong(cx)))
        .when(member.bot, |d| d.child(ui::tag("App", cx).flex_none()))
        .child(div().flex_1().flex_basis(px(0.)).min_w_0().truncate().text_size(px(12.)).text_color(palette::muted(cx)).child(member.username.clone()))
        .into_any_element()
}

fn date_cell(key: String, text: String) -> AnyElement {
    ui::truncating(key, text).into_any_element()
}

// The highest role as a pill, then "+N"; every role's name is in the tooltip.
fn roles_cell(member: &Member, guild_roles: &[Role], cx: &App) -> AnyElement {
    let mut roles: Vec<&Role> = guild_roles.iter().filter(|r| member.roles.contains(&r.id)).collect();
    roles.sort_by_key(|r| std::cmp::Reverse(r.position));
    let Some(top) = roles.first() else { return div().text_size(px(12.)).text_color(palette::muted(cx)).child("None").into_any_element() };
    let dark = cx.theme().is_dark();
    let color = if top.color == 0 { palette::faint(cx) } else { palette::role_ink(top.color, dark) };
    let names: SharedString = roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ").into();
    div()
        .id(SharedString::from(format!("ss-roles-{}", member.id)))
        .flex()
        .items_center()
        .gap(px(4.))
        .min_w_0()
        .child(ui::role_chip(top.name.clone(), color, cx).flex_shrink(1.).min_w_0().overflow_hidden())
        .when(roles.len() > 1, |d| d.child(div().flex_none().text_size(px(12.)).text_color(palette::muted(cx)).child(format!("+{}", roles.len() - 1))))
        .tooltip(move |window, cx| ui::tip(names.clone(), window, cx))
        .into_any_element()
}

fn signals_cell(member: &Member, now: DateTime<Utc>, cx: &App) -> AnyElement {
    let (danger, muted) = (palette::danger_ink(cx), palette::muted(cx));
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .children(signals(member, now).into_iter().map(|signal| {
            let (glyph, red, tip) = signal_mark(signal, member, now);
            let tip = SharedString::from(tip);
            div()
                .id(SharedString::from(format!("ss-signal-{}-{signal:?}", member.id)))
                .text_color(if red { danger } else { muted })
                .child(Icon::new(glyph).size(px(16.)))
                .tooltip(move |window, cx| ui::tip(tip.clone(), window, cx))
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn member(edit: impl FnOnce(&mut Member)) -> Member {
        let mut m = parse_member(&json!({ "user": { "id": "1", "username": "wren" } }), None).unwrap();
        edit(&mut m);
        m
    }

    fn at(text: &str) -> DateTime<Utc> {
        time(&json!(text)).unwrap()
    }

    #[test]
    fn query_parsing() {
        assert_eq!(search_terms(" wren , 123456789012345678,moss ,, "), (vec!["wren".into(), "moss".into()], vec!["123456789012345678".into()]));
        assert_eq!(search_terms("w"), (vec![], vec![]));
        assert_eq!(search_terms("  "), (vec![], vec![]));
        // Too short for a snowflake: a username.
        assert_eq!(search_terms("1234"), (vec!["1234".into()], vec![]));
        assert_eq!(search_terms("ab"), (vec!["ab".into()], vec![]));
    }

    #[test]
    fn signal_thresholds() {
        let now = at("2026-11-22T12:00:00Z");
        assert!(signals(&member(|_| {}), now).is_empty());
        // Unusual DMs count until two days after `unusual_dm_activity_until`.
        assert_eq!(signals(&member(|m| m.unusual_dm_until = Some(at("2026-11-20T12:00:00Z"))), now), vec![Signal::UnusualDmActivity]);
        assert!(signals(&member(|m| m.unusual_dm_until = Some(at("2026-11-20T11:59:59Z"))), now).is_empty());
        assert_eq!(signals(&member(|m| m.timed_out_until = Some(at("2026-11-22T12:00:01Z"))), now), vec![Signal::TimedOut]);
        assert!(signals(&member(|m| m.timed_out_until = Some(now)), now).is_empty());
        assert_eq!(signals(&member(|m| m.public_flags = 1 << 20), now), vec![Signal::UnusualAccountActivity]);
        assert!(signals(&member(|m| m.public_flags = 1 << 19), now).is_empty());
        for flag in [128, 256, 1024] {
            assert_eq!(signals(&member(|m| m.flags = flag), now), vec![Signal::Quarantined]);
        }
        assert!(signals(&member(|m| m.flags = 512), now).is_empty());
        let all = member(|m| (m.unusual_dm_until, m.timed_out_until, m.public_flags, m.flags) = (Some(now), Some(at("2026-11-23T00:00:00Z")), 1 << 20, 256));
        assert_eq!(signals(&all, now), Signal::ALL.to_vec());
    }

    #[test]
    fn join_labels() {
        let j = |kind: u64, code: Option<&str>, integration: Option<&str>| join_label(Some(&JoinSource { kind, code: code.map(str::to_owned), integration: integration.map(str::to_owned) }));
        assert_eq!(join_label(None), "Unknown");
        assert_eq!(j(0, None, None), "Unknown");
        assert_eq!(j(1, None, None), "Bot invite");
        assert_eq!(j(2, None, Some("twitch")), "Twitch");
        assert_eq!(j(2, None, Some("youtube")), "YouTube");
        assert_eq!(j(2, None, None), "Integration");
        assert_eq!(j(3, None, None), "Server discovery");
        assert_eq!(j(4, None, None), "Student hub");
        assert_eq!(j(5, Some("x7Kq2"), None), "x7Kq2");
        assert_eq!(j(6, Some("nightowls"), None), "nightowls (vanity)");
        assert_eq!(j(6, None, None), "Vanity URL");
        assert_eq!(j(7, Some("abc"), None), "Manual verification (abc)");
        assert_eq!(j(8, None, None), "Linked channel");
    }

    #[test]
    fn parses_search_entries() {
        let entry = json!({
            "member": { "nick": "Wren", "joined_at": "2026-01-02T03:04:05Z", "roles": ["r1"], "flags": 128, "communication_disabled_until": null, "user": { "id": "3", "username": "wren", "public_flags": 1048576, "bot": true } },
            "join_source_type": 6, "source_invite_code": "nightowls",
        });
        let m = parse_result(&entry).unwrap();
        assert_eq!((m.name.as_str(), m.username.as_str(), m.bot, m.roles.as_slice()), ("Wren", "wren", true, ["r1".to_owned()].as_slice()));
        assert_eq!(m.join, Some(JoinSource { kind: 6, code: Some("nightowls".into()), integration: None }));
        assert_eq!(parse_result(&json!({ "member": { "user": { "id": "4", "username": "moss" } } })).unwrap().join, None);
        assert_eq!(retry_after(&json!({ "retry_after": 2.5 })), Some(Duration::from_millis(2500)));
        assert_eq!(retry_after(&json!({ "members": [], "retry_after": 1 })), None);
    }

    #[test]
    fn relative_dates() {
        let now = at("2026-11-22T12:00:00Z");
        assert_eq!(ago(at("2026-11-22T01:00:00Z"), now), "<1 day ago");
        assert_eq!(ago(at("2026-11-21T12:00:00Z"), now), "1 day ago");
        assert_eq!(ago(at("2026-08-22T12:00:00Z"), now), "3 months ago");
        assert_eq!(ago(at("2019-11-22T12:00:00Z"), now), "7 years ago");
        assert_eq!(time_left(at("2026-11-22T12:42:10Z"), now), "42m");
        assert_eq!(time_left(at("2026-11-24T15:00:00Z"), now), "2d 3h");
    }
}
