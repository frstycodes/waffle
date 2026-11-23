// Restart (⌘R / Ctrl+R) and Restart to update. Both go through gpui's restart, which relaunches only
// once this process has exited (through LaunchServices on macOS), so two Waffles never overlap. A call
// is no reason to refuse: one confirm names it, voice is left before Waffle goes, and the relaunched
// Waffle offers that channel back under a Reconnect bar once it's connected (`Prefs::rejoin_voice`).
use std::time::Duration;

use gpui_kit::{App, AppContext as _, Context, Global, WeakEntity};

use crate::{Waffle, updater::Staged};

// Long enough for the leave (a voice state on the gateway) to go out before the socket dies with us.
// ponytail: a fixed wait, not an ack; a gateway flush signal would make it exact.
const LEAVE_GRACE: Duration = Duration::from_millis(800);

// The main view, for the app-wide ⌘R, which can come from any window.
pub struct Main(pub WeakEntity<Waffle>);
impl Global for Main {}

pub fn restart_app(cx: &mut App) {
    let main = cx.try_global::<Main>().and_then(|m| m.0.upgrade());
    let Some(main) = main else { return cx.restart() };
    cx.activate(true);
    main.update(cx, |this, cx| this.request_restart(None, cx));
}

impl Waffle {
    // `update`: install this staged build on the way.
    pub(crate) fn request_restart(&mut self, update: Option<Staged>, cx: &mut Context<Self>) {
        let call = self.call.as_ref().map(|c| c.channel.name.clone());
        let Some((title, body)) = confirm_text(call.as_deref(), update.is_some()) else { return self.restart_now(update, cx) };
        self.confirm(title, body, "Restart", move |this, _, cx| this.restart_now(update.clone(), cx), cx);
    }

    // Installs first, so a failed install keeps the call; then leaves voice and goes.
    fn restart_now(&mut self, update: Option<Staged>, cx: &mut Context<Self>) {
        // Hides the row, so a second click can't install twice; a failed install leaves nothing to retry.
        if update.is_some() {
            self.updates.status = crate::updater::Status::Idle;
            cx.notify(); // lint-allow(global-notify): the update row draws in the user panel and in settings
        }
        self.prefs.save();
        cx.spawn(async move |this, cx| {
            if let Some(staged) = update
                && let Err(e) = cx.background_spawn(async move { crate::updater::install(&staged) }).await
            {
                eprintln!("[updater] install failed: {e}");
                this.update(cx, |this, cx| this.show_toast("Couldn't install the update. Waffle will try again later.", cx)).ok();
                return;
            }
            let in_call = this
                .update(cx, |this, cx| {
                    let in_call = this.call.is_some();
                    // ponytail: server voice only; a DM call rings again, so it isn't offered back.
                    this.prefs.rejoin_voice = this.call.as_ref().and_then(|c| Some((c.guild_id.clone()?, c.channel.id.clone())));
                    this.prefs.save();
                    this.leave_voice(cx);
                    in_call
                })
                .unwrap_or(false);
            if in_call {
                cx.background_executor().timer(LEAVE_GRACE).await;
            }
            cx.update(|cx| cx.restart());
        })
        // lint-allow(detached-task): ends when the app restarts, and holds only a WeakEntity.
        .detach();
    }
}

// The confirm before a restart that would drop a call; none otherwise.
fn confirm_text(call: Option<&str>, update: bool) -> Option<(String, String)> {
    let name = call?;
    let then = if update { ", install the update and reopen" } else { " and reopen" };
    Some((format!("Restart during {name}?"), format!("Waffle will leave the call{then}. You can reconnect when it opens.")))
}

#[cfg(test)]
mod tests {
    use super::confirm_text;

    #[test]
    fn asks_only_when_a_restart_would_drop_a_call() {
        assert_eq!(confirm_text(None, false), None);
        assert_eq!(confirm_text(None, true), None);
        let (title, body) = confirm_text(Some("hangout"), true).unwrap();
        assert_eq!(title, "Restart during hangout?");
        assert_eq!(body, "Waffle will leave the call, install the update and reopen. You can reconnect when it opens.");
        assert_eq!(confirm_text(Some("hangout"), false).unwrap().1, "Waffle will leave the call and reopen. You can reconnect when it opens.");
    }
}
