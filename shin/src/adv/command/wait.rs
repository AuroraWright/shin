use std::fmt::{Debug, Formatter};

use tracing::debug;

use super::prelude::*;
use crate::update::UpdateContext;

pub struct WAIT {
    token: Option<command::token::WAIT>,
    waiting_left: Ticks,
    allow_interrupt: bool,
    started: bool,
}

impl WAIT {
    fn new(token: command::token::WAIT, duration: i32, allow_interrupt: bool) -> Self {
        Self {
            token: Some(token),
            // Native138d4 CNEG is32-bit: INT_MIN stays negative and finishes
            // immediately. Convert only after the wrapping absolute value.
            waiting_left: Ticks::from_f32(duration.wrapping_abs() as f32),
            allow_interrupt,
            started: false,
        }
    }

    fn poll_clock(&mut self, delta: Ticks, is_fast_forwarding: bool) -> bool {
        // Native5090 initializes on its first poll, after the frame's shared
        // clock update11560..11570. Do not charge that frame to a new wait.
        if self.started && self.waiting_left > Ticks::ZERO {
            self.waiting_left -= delta;
        }
        self.started = true;
        if is_fast_forwarding {
            self.waiting_left = Ticks::ZERO;
        }
        self.waiting_left <= Ticks::ZERO
    }
}

impl StartableCommand for command::runtime::WAIT {
    type StateInfo = ();
    fn apply_state(&self, _state: &mut VmState) {
        // nothing to do
    }

    fn start(
        self,
        _context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        _state_info: (),
        _adv_state: &mut AdvState,
    ) -> CommandStartResult {
        Yield(WAIT::new(self.token, self.wait_amount, self.allow_interrupt).into())
    }
}

impl UpdatableCommand for WAIT {
    fn advance(&mut self) -> bool {
        if self.allow_interrupt {
            self.waiting_left = Ticks::ZERO;
            true
        } else {
            false
        }
    }

    fn update(
        &mut self,
        context: &mut UpdateContext,
        _scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        _adv_state: &mut AdvState,
        is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if self.poll_clock(context.delta_ticks, is_fast_forwarding) {
            debug!("WAIT: done");
            // TODO: this is kinda boilerplaty, maybe we want to have a TokenCell type?
            Some(self.token.take().unwrap().finish())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::ExecutingCommand;

    fn original_wait(duration: i32, allow_interrupt: bool) -> WAIT {
        use shin_core::{
            format::scenario::instruction_elements::{NumberSpec, U8Bool},
            vm::{
                IntoRuntimeForm, VmCtx,
                command::{CompiletimeCommand, compiletime as c},
            },
        };
        let command = CompiletimeCommand::WAIT(c::WAIT {
            allow_interrupt: U8Bool(allow_interrupt),
            wait_amount: NumberSpec::constant(duration),
        })
        .into_runtime_form(&VmCtx::new(0, 42));
        let command::RuntimeCommand::WAIT(runtime) = command else {
            unreachable!()
        };
        assert_eq!(
            runtime.wait_amount, duration,
            "VM preserves signed WAIT duration"
        );
        WAIT::new(runtime.token, runtime.wait_amount, runtime.allow_interrupt)
    }

    #[test]
    fn signed_wait_preserves_first_poll_then_counts_full_duration() {
        for duration in [12, -12] {
            let mut wait = original_wait(duration, false);
            assert!(!wait.poll_clock(Ticks::from_f32(6.0), false));
            assert_eq!(wait.waiting_left, Ticks::from_f32(12.0));
            assert!(!wait.poll_clock(Ticks::from_f32(6.0), false));
            assert_eq!(wait.waiting_left, Ticks::from_f32(6.0));
            assert!(wait.poll_clock(Ticks::from_f32(7.0), false));
            assert_eq!(wait.waiting_left, Ticks::from_f32(-1.0));
        }
        for duration in [0, i32::MIN] {
            let mut wait = original_wait(duration, false);
            assert!(wait.poll_clock(Ticks::from_f32(6.0), false));
            assert_eq!(wait.waiting_left.as_f32(), duration as f32);
        }
    }

    #[test]
    fn fast_forward_clears_first_or_later_poll_even_when_advance_is_disallowed() {
        for initial_polls in [0, 1] {
            let mut wait = original_wait(-120, false);
            for _ in 0..initial_polls {
                assert!(!wait.poll_clock(Ticks::from_f32(6.0), false));
            }
            assert!(wait.poll_clock(Ticks::from_f32(6.0), true));
            assert_eq!(wait.waiting_left, Ticks::ZERO);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_wait_signed_setup_first_poll_and_clock_update() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x50a4, 0x340001a8),
            (0x50d8, 0xf9400660),
            (0x50e0, 0x940039fc),
            (0x50e8, 0x39008268),
            (0x50f4, 0x3707fe00),
            (0x11560, 0xbd449660),
            (0x11568, 0x5400006d),
            (0x1156c, 0x1e283800),
            (0x11570, 0xbd049660),
            (0x138d0, 0x7100005f),
            (0x138d4, 0x5a825448),
            (0x138dc, 0x1e220100),
            (0x138e0, 0xbd049400),
            (0x13904, 0x1e202008),
            (0x13908, 0x1a9fd7e0),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native WAIT instruction {address:x}"
            );
        }
    }

    #[test]
    fn advance_interrupts_only_waits_that_allow_it() {
        for allowed in [false, true] {
            let mut wait = WAIT {
                token: None,
                waiting_left: Ticks::from_seconds(2.0),
                allow_interrupt: allowed,
                started: false,
            };
            assert_eq!(wait.advance(), allowed);
            assert_eq!(wait.waiting_left == Ticks::ZERO, allowed);
            let mut dispatched: ExecutingCommand = wait.into();
            assert_eq!(dispatched.advance(), allowed);
        }
    }
}

impl Debug for WAIT {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WAIT").field(&self.waiting_left).finish()
    }
}
