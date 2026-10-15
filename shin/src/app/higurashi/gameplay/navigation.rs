//! Native navigation request acceptance uses the current owned VM/history.
use super::*;
use crate::adv::higurashi::{
    forward_search::{Kind, Status as SearchStatus},
    input::Request,
    navigation::{self, Chapter, PreviousMessage},
};
use shin_core::vm::command::types::Volume;

struct Pending {
    replay: Option<ReplaySession>,
    mode: u32,
}
#[cfg(test)]
mod tests;
#[derive(Default)]
pub(super) struct Navigation {
    pending: Option<Pending>,
    event_candidate: Option<ReplaySession>,
}
impl Navigation {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn event_jump(
        &mut self,
        runner: &HigurashiRunner,
        bindings: &Bindings,
        read_only: bool,
    ) -> Result<bool> {
        self.event_candidate = None;
        let Some(mut search) = runner.begin_forward_search(
            &bindings.memory,
            &bindings.progress,
            Kind::EventEnd,
            read_only,
        )?
        else {
            return Ok(false);
        };
        loop {
            match search.pump(&bindings.history, 4096, 100000, || false)? {
                SearchStatus::CommandBudget => {}
                SearchStatus::Complete => {
                    self.event_candidate = search.take_result()?;
                    return Ok(self.event_candidate.is_some());
                }
                SearchStatus::Cancelled => bail!("Native synchronous event search was cancelled"),
            }
        }
    }
    fn restore(host: &HigurashiHost, bindings: &Bindings, target: u32) -> Result<ReplaySession> {
        ReplaySession::new(
            host.runner.scenario().clone(),
            bindings
                .memory
                .fork_for_restore(target, target, &bindings.progress)?,
        )
    }
    fn forward(
        host: &HigurashiHost,
        bindings: &Bindings,
        read_only: bool,
    ) -> Result<Option<ReplaySession>> {
        let Some(mut search) = host.runner.begin_forward_search(
            &bindings.memory,
            &bindings.progress,
            Kind::ReadAhead,
            read_only,
        )?
        else {
            return Ok(None);
        };
        loop {
            match search.pump(&bindings.history, 4096, 100000, || false)? {
                SearchStatus::CommandBudget => {}
                SearchStatus::Complete => return search.take_result(),
                SearchStatus::Cancelled => bail!("Native synchronous chapter search was cancelled"),
            }
        }
    }
    fn cue(name: &str, sounds: &Arc<Mutex<SystemSoundPlayer>>, assets: &AssetServer) {
        let mut sounds = sounds.lock();
        sounds.update();
        sounds.play(assets, name, Volume(1.0));
    }
    ///17d1c/17f04 act only after the real question's stage1/Yes callback. Next
    /// searches again from the current VM; its opening preflight is discarded.
    pub fn process(
        &mut self,
        host: &mut HigurashiHost,
        bindings: &Bindings,
        controls: &mut controls::Controls,
        read_only: bool,
        sounds: &Arc<Mutex<SystemSoundPlayer>>,
        assets: &AssetServer,
    ) -> Result<()> {
        for kind in host.take_chapter_requests() {
            ensure!(
                self.pending.is_none(),
                "Navigation already has a replay owner"
            );
            let replay = match kind {
                Chapter::Previous => Some(Self::restore(
                    host,
                    bindings,
                    navigation::previous_chapter(
                        host.runner.native_message_timeline().ok_or_else(|| {
                            anyhow::anyhow!("Chapter callback requires live counters")
                        })?,
                    ),
                )?),
                Chapter::Next => Self::forward(host, bindings, read_only)?,
            };
            if let Some(replay) = replay {
                controls.state.temporary_indicator = 0;
                self.pending = Some(Pending {
                    replay: Some(replay),
                    mode: 0,
                });
            }
        }
        if self.pending.is_some() {
            return Ok(());
        }
        match controls.requests.front().copied() {
            Some(Request::SceneJump) => {
                let replay = self.event_candidate.take().ok_or_else(|| {
                    anyhow::anyhow!("Event jump requires the owned11664 search result")
                })?;
                self.pending = Some(Pending {
                    replay: Some(replay),
                    mode: 0,
                });
            }
            Some(Request::PreviousMessage) => {
                let vm = host
                    .runner
                    .native_read_state()
                    .ok_or_else(|| anyhow::anyhow!("Previous message requires a VM"))?;
                let timeline = host.runner.native_message_timeline().unwrap();
                let has_text = host
                    .runner
                    .native_playback_descriptors()
                    .is_some_and(|d| d.message.text.is_some());
                match navigation::previous_message(
                    vm,
                    timeline,
                    controls.read_policy(read_only),
                    has_text,
                ) {
                    PreviousMessage::Suppressed => {
                        controls.requests.pop_front();
                    }
                    PreviousMessage::Unavailable => {
                        Self::cue("error", sounds, assets);
                        controls.requests.pop_front();
                    }
                    PreviousMessage::Restore(target) => {
                        let replay = Self::restore(host, bindings, target)?;
                        Self::cue("decide", sounds, assets);
                        self.pending = Some(Pending {
                            replay: Some(replay),
                            mode: 1,
                        });
                    }
                }
            }
            Some(request @ (Request::NextChapter | Request::PreviousChapter)) => {
                let kind = if request == Request::NextChapter {
                    Chapter::Next
                } else {
                    Chapter::Previous
                };
                if kind == Chapter::Next && Self::forward(host, bindings, read_only)?.is_none() {
                    Self::cue("error", sounds, assets);
                    controls.requests.pop_front();
                } else {
                    host.open_chapter_question(kind, sounds.clone())?;
                    controls.state.modes = 0;
                    Self::cue("decide", sounds, assets);
                    controls.requests.pop_front();
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn prepare(
        &mut self,
        host: &mut HigurashiHost,
        bindings: &Bindings,
        controls: &mut controls::Controls,
        context: &mut PreRenderContext,
        screens: &mut HigurashiSceneTextures,
        seeds: Seeds,
    ) -> Result<()> {
        if let Some(pending) = &mut self.pending {
            host.begin_scene_reconstruction(
                &mut pending.replay,
                pending.mode,
                context,
                screens,
                bindings.history.clone(),
                move || (seeds.lock())(),
            )?;
            if matches!(
                controls.requests.front(),
                Some(Request::PreviousMessage | Request::SceneJump)
            ) {
                controls.requests.pop_front();
            }
            self.pending = None;
        }
        Ok(())
    }
}
