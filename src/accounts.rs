// Discord's multiple accounts: up to five signed-in accounts, one connected at a time. The list (ids,
// names, avatars, last status) lives in settings.json; each token sits in the keyring under its id.
use gpui_kit::{Context, Window};
use serde::{Deserialize, Serialize};

use crate::{
    Waffle, auth,
    discord::{Status, User},
    settings::Prefs,
};

// Discord's own limit.
pub const MAX: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub avatar: String,
    // What it was set to when last connected; only the connected account's status is live.
    #[serde(default)]
    pub status: Status,
}

// The token to start with: DISCORD_TOKEN, else the last-used account's.
pub fn startup_token(prefs: &Prefs) -> Option<String> {
    std::env::var("DISCORD_TOKEN").ok().filter(|t| !t.trim().is_empty()).or_else(|| prefs.account.as_deref().and_then(auth::stored_token))
}

fn from_env(token: &str) -> bool {
    std::env::var("DISCORD_TOKEN").is_ok_and(|t| t.trim() == token)
}

// Logging out: the account's snapshot, caches and last place go with its token.
fn forget_caches(id: &str) {
    crate::store::forget_account(id);
    crate::home::forget_last_channel(id);
}

// Adds or refreshes `account`, keeping its place in the list.
pub fn upsert(accounts: &mut Vec<Account>, account: Account) {
    match accounts.iter_mut().find(|a| a.id == account.id) {
        Some(slot) => *slot = account,
        None => accounts.push(account),
    }
}

// Removes `id` and names the account to fall back to, if any is left.
pub fn remove(accounts: &mut Vec<Account>, id: &str) -> Option<String> {
    accounts.retain(|a| a.id != id);
    accounts.first().map(|a| a.id.clone())
}

impl Waffle {
    // On READY: files the token under this user and makes them the account to restart into.
    pub(crate) fn remember_account(&mut self, me: &User) {
        let Some(token) = self.token.as_deref().filter(|t| !from_env(t)) else { return };
        auth::store_token(&me.id, token);
        let account = Account { id: me.id.clone(), name: me.name.clone(), avatar: me.avatar.clone(), status: self.my_status };
        upsert(&mut self.prefs.accounts, account);
        self.prefs.account = Some(me.id.clone());
        self.prefs.save();
    }

    // Discord refused the token: forget it so the next start doesn't try it again.
    pub(crate) fn forget_rejected_account(&mut self) {
        if self.token.as_deref().is_none_or(from_env) {
            return;
        }
        let Some(id) = self.prefs.account.take() else { return };
        auth::forget_token(&id);
        forget_caches(&id);
        remove(&mut self.prefs.accounts, &id);
        self.prefs.account = self.prefs.accounts.first().map(|a| a.id.clone());
        self.prefs.save();
    }

    pub(crate) fn can_add_account(&self) -> bool {
        self.prefs.accounts.len() < MAX
    }

    pub(crate) fn switch_account(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.demo_guard(cx) {
            return;
        }
        if self.prefs.account.as_deref() == Some(id.as_str()) && self.me.is_some() {
            self.status_menu = false;
            return cx.notify(); // lint-allow(global-notify): the status menu closing is an overlay, which only follows the app
        }
        let Some(token) = auth::stored_token(&id) else {
            self.toast = Some("That account needs signing in again.".into());
            return self.remove_account(id, window, cx);
        };
        self.prefs.account = Some(id);
        self.restart(Some(token), window, cx);
    }

    // Signs in another account; the connected one keeps its token and stays in the list.
    pub(crate) fn add_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.demo_guard(cx) {
            return;
        }
        self.restart(None, window, cx);
    }

    // Logs one account out. Logging out the connected one moves to the next account, or to sign-in.
    pub(crate) fn remove_account(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.demo_guard(cx) {
            return;
        }
        auth::forget_token(&id);
        forget_caches(&id);
        let next = remove(&mut self.prefs.accounts, &id);
        if self.prefs.account.as_deref() != Some(id.as_str()) {
            self.prefs.save();
            return cx.notify(); // lint-allow(global-notify): the account list in the status menu (an overlay, which only follows the app) lost a row
        }
        self.prefs.account = next.clone();
        let token = next.as_deref().and_then(auth::stored_token);
        self.restart(token, window, cx);
    }

    pub(crate) fn sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.me.as_ref().map(|me| me.id.clone()).or_else(|| self.prefs.account.clone()) {
            Some(id) => self.remove_account(id, window, cx),
            None => self.sign_in(cx),
        }
    }

    // Tears the whole session down (gateway, voice, every cache of the old account) by rebuilding the
    // view's state, then connects with `token` or shows sign-in.
    // GAP: requests already in flight for the old account still land on the new state when they finish.
    fn restart(&mut self, token: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_status();
        self.prefs.save();
        let (hotkeys, updates) = (self.hotkeys.take(), std::mem::take(&mut self.updates));
        self.call = None;
        self.end_activities(cx);
        *self = Self::build(window, None, cx);
        self.hotkeys = hotkeys;
        self.updates = updates;
        self.apply_prefs();
        match token {
            Some(token) => {
                self.launch_from_snapshot(window, cx);
                self.connect(token, cx);
            }
            None => {
                crate::store::set_account(None);
                self.sign_in(cx);
            }
        }
    }

    // The demo's accounts are fixtures: acting on them would write them into the real settings and keyring.
    fn demo_guard(&mut self, cx: &mut Context<Self>) -> bool {
        if !(cfg!(debug_assertions) && std::env::var_os("WAFFLE_DEMO").is_some()) {
            return false;
        }
        self.status_menu = false;
        self.toast = Some("Demo accounts can't be switched.".into());
        cx.notify(); // lint-allow(global-notify): the status menu and the toast are overlays, which only follow the app
        true
    }

    fn remember_status(&mut self) {
        let Some(me) = &self.me else { return };
        if let Some(account) = self.prefs.accounts.iter_mut().find(|a| a.id == me.id) {
            account.status = self.my_status;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, name: &str) -> Account {
        Account { id: id.into(), name: name.into(), avatar: String::new(), status: Status::Online }
    }

    #[test]
    fn upsert_refreshes_in_place() {
        let mut list = vec![account("1", "a"), account("2", "b")];
        upsert(&mut list, account("1", "renamed"));
        upsert(&mut list, account("3", "c"));
        let names: Vec<_> = list.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["renamed", "b", "c"]);
    }

    #[test]
    fn remove_falls_back_to_first_left() {
        let mut list = vec![account("1", "a"), account("2", "b")];
        assert_eq!(remove(&mut list, "1").as_deref(), Some("2"));
        assert_eq!(remove(&mut list, "2"), None);
        assert!(list.is_empty());
    }
}
