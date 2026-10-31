// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Native input lifecycle and submission fence. No source/document authority.
#[derive(Debug)]
pub struct NativeInput {
    pub window_id: u32,
    pub focused: bool,
    pub started: bool,
    pub shift: bool,
    pub text_after: u64,
    submit: SubmitFence,
    batch_draft: Option<u64>,
}
#[derive(Debug, Default)]
struct SubmitFence {
    blocked: bool,
    batch_has_text: bool,
    batch_text_blocked: bool,
}
impl NativeInput {
    pub fn new(window_id: u32) -> Self {
        Self {
            window_id,
            focused: true,
            started: false,
            shift: false,
            text_after: 0,
            submit: SubmitFence::default(),
            batch_draft: None,
        }
    }
    /// Inspect the complete SDL drain before dispatch: a Return preceding the
    /// commit event in this same drain must not review/accept that commit.
    pub fn begin_batch(&mut self, has_text: bool, draft: Option<u64>) {
        self.submit.batch_has_text = has_text;
        self.submit.batch_text_blocked = false;
        self.batch_draft = draft;
    }
    pub fn text_allowed(&self, draft: Option<u64>, timestamp: u64) -> bool {
        self.focused
            && !self.submit.batch_text_blocked
            && self.batch_draft == draft
            && draft.is_some()
            && timestamp >= self.text_after
    }
    pub fn refocus(&mut self, timestamp: u64) {
        self.focused = true;
        self.text_after = timestamp;
        self.submit.batch_text_blocked = true;
    }
    /// Explicit cancellation invalidates text already drained for the old IME
    /// session. A natural empty `TextEditing` can precede a legitimate commit and
    /// must not use this cancellation transition.
    pub fn cancel_composition(&mut self) {
        self.submit.batch_text_blocked = true;
        self.submit.blocked = true;
    }
    pub fn text_activity(&mut self) {
        self.submit.blocked = true;
    }
    pub fn return_released(&mut self, composing: bool) {
        if !composing {
            self.submit.blocked = false;
        }
    }
    pub fn submit_allowed(&self, composing: bool) -> bool {
        self.focused && !composing && !self.submit.blocked && !self.submit.batch_has_text
    }
    pub fn lose_focus(&mut self) {
        self.focused = false;
        self.shift = false;
        self.submit.blocked = true;
        self.submit.batch_text_blocked = true;
    }
}

/// Advertise the composition UI that the app actually renders. SDL requires
/// this capability hint before initialization; candidates remain OS-owned.
pub fn configure_ime_ui() -> Result<(), String> {
    let name = sdl3::hint::names::IME_IMPLEMENTED_UI;
    if !sdl3::hint::set_with_priority(name, "composition", &sdl3::hint::Hint::Override) {
        return Err(format!(
            "SDL refused application IME composition capability: {}",
            sdl3::get_error()
        ));
    }
    if sdl3::hint::get(name).as_deref() != Some("composition") {
        return Err("SDL application IME composition capability was not retained".into());
    }
    Ok(())
}

/// SDL has no safe Rust wrapper for this operation in sdl3 0.20. Keep the
/// required FFI confined here; the borrowed live Window owns its raw pointer.
#[allow(unsafe_code)]
pub fn clear_composition(window: &sdl3::video::Window) -> Result<(), String> {
    // SAFETY: Window::raw belongs to the live borrowed SDL Window. This function
    // is called on the video/event thread and SDL does not retain the pointer.
    if unsafe { sdl3::sys::keyboard::SDL_ClearComposition(window.raw()) } {
        Ok(())
    } else {
        Err(sdl3::get_error().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commit_in_drain_fences_return_in_either_event_order() {
        for composing in [false, true] {
            let mut input = NativeInput::new(1);
            input.begin_batch(true, None);
            assert!(!input.submit_allowed(composing));
            input.text_activity();
            assert!(!input.submit_allowed(false));
            input.return_released(false);
            assert!(!input.submit_allowed(false));
            input.begin_batch(false, None);
            assert!(input.submit_allowed(false));
        }
    }
    #[test]
    fn focus_loss_requires_explicit_refocus_and_fresh_return_release() {
        let mut input = NativeInput::new(1);
        input.lose_focus();
        input.begin_batch(false, None);
        input.return_released(false);
        assert!(!input.submit_allowed(false));
        input.focused = true;
        assert!(input.submit_allowed(false));
    }
    #[test]
    fn composition_capability_is_set_in_fresh_process() {
        const CHILD: &str = "TILEWRIGHT_TEST_IME_HINT_CHILD";
        if std::env::var_os(CHILD).is_some() {
            configure_ime_ui().unwrap();
            assert_eq!(
                sdl3::hint::get(sdl3::hint::names::IME_IMPLEMENTED_UI).as_deref(),
                Some("composition")
            );
            return;
        }
        // Hints are SDL process-global. Test in a fresh subprocess, without
        // initializing SDL or changing another test's environment or UI state.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                concat!(
                    module_path!(),
                    "::composition_capability_is_set_in_fresh_process"
                ),
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("SDL_IME_IMPLEMENTED_UI", "candidates")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
}
