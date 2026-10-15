use shin_core::vm::command::types::WipeFlags;
use shin_tasks::AsyncTask;

use super::prelude::*;
use crate::wiper::AnyWiper;

#[derive(Debug)]
enum WipeState {
    WaitingForWipeLoader {
        load_task: Option<AsyncTask<anyhow::Result<Option<AnyWiper>>>>,
    },
    WaitingForRetry,
    WaitingForWipe,
    Finished,
}

#[derive(Debug)]
pub struct WIPE {
    token: Option<command::token::WIPE>,
    flags: WipeFlags,
    ty: i32,
    duration: Ticks,
    params: shin_core::format::scenario::instruction_elements::UntypedNumberArray,
    error: Option<anyhow::Error>,
    load_wait_attempted: bool,
    state: WipeState,
}

fn spawn_loader(
    server: Arc<crate::asset::system::AssetServer>,
    scenario: Arc<Scenario>,
    ty: i32,
    duration: Ticks,
    params: shin_core::format::scenario::instruction_elements::UntypedNumberArray,
) -> Option<AsyncTask<anyhow::Result<Option<AnyWiper>>>> {
    (duration > Ticks::ZERO && (0..=9).contains(&ty)).then(|| {
        shin_tasks::async_io::spawn(async move {
            super::catch_preparation_panic(AnyWiper::load_raw_checked(
                &server, &scenario, ty, duration, params,
            ))
            .await?
        })
    })
}

impl StartableCommand for command::runtime::WIPE {
    type StateInfo = bool;
    fn apply_state(&self, state: &mut VmState) -> bool {
        if state.layers.is_page_back_started {
            state.layers.is_page_back_started = false;

            true
        } else {
            false
        }
    }

    fn start(
        self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &VmState,
        needs_wipe: bool,
        adv_state: &mut AdvState,
    ) -> CommandStartResult {
        if !needs_wipe {
            return self.token.finish().into();
        }

        let load_task = spawn_loader(
            context.asset_server.clone(),
            scenario.clone(),
            self.ty,
            self.duration,
            self.params,
        );

        if !self.flags.contains(WipeFlags::DONT_BLOCK_ANIMATIONS) {
            adv_state.allow_running_animations = false;
        }

        Yield(
            WIPE {
                token: Some(self.token),
                flags: self.flags,
                ty: self.ty,
                duration: self.duration,
                params: self.params,
                error: None,
                load_wait_attempted: false,
                state: WipeState::WaitingForWipeLoader { load_task },
            }
            .into(),
        )
    }
}

#[cfg(test)]
impl WIPE {
    pub(super) fn reference_hold_loading(&mut self) -> futures::channel::oneshot::Sender<()> {
        let WipeState::WaitingForWipeLoader { load_task } = &mut self.state else {
            panic!("Wipe loader phase");
        };
        let task = load_task.take().expect("Asset-backed pending wipe loader");
        let (sender, receiver) = futures::channel::oneshot::channel();
        *load_task = Some(shin_tasks::async_io::spawn(async move {
            receiver
                .await
                .expect("Publication gate released or task canceled");
            task.await
        }));
        sender
    }
}

impl UpdatableCommand for WIPE {
    fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }
    fn update(
        &mut self,
        context: &mut UpdateContext,
        scenario: &Arc<Scenario>,
        _vm_state: &mut VmState,
        adv_state: &mut AdvState,
        _is_fast_forwarding: bool,
    ) -> Option<CommandResult> {
        if self.error.is_some() {
            return None;
        }
        // TODO: state machines like this can be useful in multiple commands
        // it would be nice to have a generic abstraction for this
        let mut break_the_loop = false;

        while !break_the_loop {
            replace_with::replace_with(
                &mut self.state,
                || WipeState::Finished,
                |state| match state {
                    WipeState::WaitingForRetry => {
                        if !self.flags.contains(WipeFlags::DONT_BLOCK_ANIMATIONS) {
                            adv_state.allow_running_animations = false;
                        }
                        WipeState::WaitingForWipeLoader {
                            load_task: spawn_loader(
                                context.asset_server.clone(),
                                scenario.clone(),
                                self.ty,
                                self.duration,
                                self.params,
                            ),
                        }
                    }
                    WipeState::WaitingForWipeLoader { load_task } => match load_task {
                        Some(mut load_task) => {
                            let ready = if std::mem::replace(&mut self.load_wait_attempted, true) {
                                load_task.poll_naive()
                            } else {
                                load_task.poll_with_timeout(context.loader_wait_budget())
                            };
                            if let Some(result) = ready {
                                match result {
                                    Ok(wiper) => {
                                        adv_state.screen_layer_mut().apply_transition(wiper);
                                        adv_state.allow_running_animations = true;
                                        WipeState::WaitingForWipe
                                    }
                                    Err(error) => {
                                        self.error = Some(error);
                                        adv_state.allow_running_animations = true;
                                        self.load_wait_attempted = false;
                                        break_the_loop = true;
                                        WipeState::WaitingForRetry
                                    }
                                }
                            } else {
                                break_the_loop = true;
                                WipeState::WaitingForWipeLoader {
                                    load_task: Some(load_task),
                                }
                            }
                        }
                        None => {
                            adv_state.screen_layer_mut().apply_transition(None);
                            adv_state.allow_running_animations = true;

                            WipeState::WaitingForWipe
                        }
                    },
                    WipeState::WaitingForWipe => {
                        if self.flags.contains(WipeFlags::DONT_WAIT)
                            || !adv_state.screen_layer().is_transition_active()
                        {
                            WipeState::Finished
                        } else {
                            break_the_loop = true;
                            WipeState::WaitingForWipe
                        }
                    }
                    WipeState::Finished => unreachable!(),
                },
            );

            if let WipeState::Finished = &self.state {
                return self.token.take().unwrap().finish().into();
            }
        }

        None
    }
}
