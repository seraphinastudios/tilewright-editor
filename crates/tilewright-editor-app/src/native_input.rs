// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Native input lifecycle and submission fence. No source/document authority.
#[derive(Debug)]
pub struct NativeInput {
    pub window_id: u32,
    pub focused: bool,
    pub started: bool,
    pub shift: bool,
    submit_blocked: bool,
    batch_has_text: bool,
}
impl NativeInput {
    pub fn new(window_id: u32) -> Self {
        Self {
            window_id,
            focused: true,
            started: false,
            shift: false,
            submit_blocked: false,
            batch_has_text: false,
        }
    }
    /// Inspect the complete SDL drain before dispatch: a Return preceding the
    /// commit event in this same drain must not review/accept that commit.
    pub fn begin_batch(&mut self, has_text: bool) {
        self.batch_has_text = has_text;
    }
    pub fn text_activity(&mut self) {
        self.submit_blocked = true;
    }
    pub fn return_released(&mut self, composing: bool) {
        if !composing {
            self.submit_blocked = false;
        }
    }
    pub fn submit_allowed(&self, composing: bool) -> bool {
        self.focused && !composing && !self.submit_blocked && !self.batch_has_text
    }
    pub fn lose_focus(&mut self) {
        self.focused = false;
        self.shift = false;
        self.submit_blocked = true;
    }
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
            input.begin_batch(true);
            assert!(!input.submit_allowed(composing));
            input.text_activity();
            assert!(!input.submit_allowed(false));
            input.return_released(false);
            assert!(!input.submit_allowed(false));
            input.begin_batch(false);
            assert!(input.submit_allowed(false));
        }
    }
    #[test]
    fn focus_loss_requires_explicit_refocus_and_fresh_return_release() {
        let mut input = NativeInput::new(1);
        input.lose_focus();
        input.begin_batch(false);
        input.return_released(false);
        assert!(!input.submit_allowed(false));
        input.focused = true;
        assert!(input.submit_allowed(false));
    }
}
