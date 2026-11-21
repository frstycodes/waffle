// Onboarding: the full-screen questions after joining, then the server rules (design/explore-joining.html §2).
use gpui_kit::{AnyElement, Context, Window};

use crate::Waffle;

pub struct OnboardingView {}

impl Waffle {
    // Right after joining: fetch the server's onboarding and open it when it has questions for us.
    pub(crate) fn start_onboarding_if_needed(&mut self, _guild_id: String, _cx: &mut Context<Self>) {}

    // (answered, total) when onboarding was closed halfway, for the Finish joining row.
    pub(crate) fn onboarding_progress(&self, _guild_id: &str) -> Option<(usize, usize)> {
        None
    }

    // Reopens onboarding at the first unanswered question.
    pub(crate) fn open_onboarding(&mut self, _guild_id: String, _cx: &mut Context<Self>) {}

    pub(crate) fn onboarding_overlay(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }
}

pub fn demo(_waffle: &mut Waffle, _scene: &str, _window: &mut Window, _cx: &mut Context<Waffle>) {}
