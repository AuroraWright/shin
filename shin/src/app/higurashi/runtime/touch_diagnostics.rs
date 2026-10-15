//! Read-only transition diagnostics for the isolated AppKit mouse harness.
//! Parent registration also supplies narrow normal-build observers on
//! SceneHost, MainTitle and Host. No cfg(test) method is called here.
use super::Runtime;

impl Runtime {
    /// Observe callback boundaries without advancing clocks, modes or input.
    pub(in crate::app::higurashi) fn trace_platform_state(
        &self,
        event: &'static str,
        focused: Option<bool>,
        platform_suspended: bool,
        audio_suspended: bool,
    ) {
        if !tracing::enabled!(tracing::Level::DEBUG) {
            return;
        }
        // Keep ordinary foreground diagnostics bounded; unfocused close
        // updates are all retained to expose their neutral input snapshots.
        if event == "progress" && focused == Some(true) && self.frames % 60 != 0 {
            return;
        }
        let clock_bits = self
            .globals()
            .ok()
            .flatten()
            .map(|globals| globals.clock.ticks().to_bits());
        let position = self.touch_diagnostic_story_position();
        let buttons = self.input.buttons();
        tracing::debug!(
            event,
            ?focused,
            platform_suspended,
            audio_suspended,
            frames = self.frames,
            ?clock_bits,
            ?position,
            phase = self.touch_diagnostic_phase().unwrap_or("transition"),
            close_active = self.close_active(),
            freezes_scene = self.freezes_scene(),
            held = buttons.held,
            pressed = buttons.pressed,
            repeat = buttons.repeat,
            "HIGURASHI_PLATFORM_STATE"
        );
    }

    pub(super) fn touch_diagnostic_phase(&self) -> Option<&'static str> {
        if self.shutdown.active() {
            if self.shutdown.requested() {
                if let Some(phase) = self
                    .slot
                    .as_ref()
                    .and_then(|slot| slot.game.as_ref())
                    .and_then(|game| game.touch_diagnostic_save_menu())
                    .and_then(|menu| menu.touch_diagnostic_phase())
                {
                    return match phase {
                        "save_recovery_retry" => Some("close_waiting_save_retry"),
                        "save_recovery_return" => Some("close_waiting_save_return"),
                        "save_menu" => Some("close_waiting_save_menu"),
                        _ => self.shutdown.touch_diagnostic_phase(),
                    };
                }
            }
            return self.shutdown.touch_diagnostic_phase();
        }
        if let Some(bootstrap) = &self.bootstrap {
            return bootstrap.touch_diagnostic_phase();
        }
        let slot = self.slot.as_ref()?;
        if let Some(title) = &slot.title {
            return title.touch_diagnostic_phase();
        }
        let game = slot.game.as_ref()?;
        if let Some(menu) = game.touch_diagnostic_save_menu() {
            return menu.touch_diagnostic_phase();
        }
        let host = game.host()?;
        if !host.runner.native_vm_attached() || host.runner.reconstruction_phase().is_some() {
            return None;
        }
        if host.touch_diagnostic_browser_ready() {
            return Some("browser");
        }
        let message = &host.runner.frame().message.playback;
        // SystemMenu also conceals the message. Observe its retained input
        // owner before distinguishing the standalone HiddenWindow mode.
        if host.touch_diagnostic_menu_ready() {
            return Some("system_menu");
        }
        if message.hidden {
            return (host.modal_pause_depth() == 0 && message.modal_slide == 0.)
                .then_some("hidden");
        }
        if host.has_modes() {
            return None;
        }
        (host.touch_diagnostic_modal_idle()
            && message.modal_slide == 1.
            && message.interested_in_input())
        .then_some("dialogue")
    }

    /// Debug-only call site fingerprints the complete selected runtime Slot,
    /// including dormant payloads omitted from the disk projection. Debug
    /// formatting is deterministic for these structs/vectors within one run;
    /// this is an observation token, not a new persistence representation.
    pub(super) fn touch_diagnostic_save_slot(&self) -> Option<(usize, bool, u64, u8)> {
        let selected = self.touch_diagnostic_save_selection()?;
        let globals = self.globals().ok()??;
        let row = globals.runtime_saves?.manual.get(selected)?;
        let fingerprint = format!("{row:?}")
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
            });
        Some((
            selected,
            row.valid,
            fingerprint,
            globals.data.save_menu_position,
        ))
    }

    pub(super) fn touch_diagnostic_save_selection(&self) -> Option<usize> {
        let game = self.slot.as_ref()?.game.as_ref()?;
        usize::try_from(game.touch_diagnostic_save_menu()?.state().selected).ok()
    }

    /// The VM does not exist during the initial factory. Never call position
    /// until attachment; logs must not turn an observer into a startup panic.
    pub(super) fn touch_diagnostic_story_position(&self) -> Option<(u32, u64)> {
        let host = self.slot.as_ref()?.game.as_ref()?.host()?;
        host.runner
            .native_vm_attached()
            .then(|| (host.runner.position().0, host.runner.completed_commands()))
    }
}
