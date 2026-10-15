//! Actual738b0/71fb4 Manual lifetime with atlas-only First and caller callbacks.
#[cfg(test)]
use super::ViewState;
use super::{Action, Kind, Model, Services, Task, view::View};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
        },
        input_area::{InputArea, PointerFrame},
    },
    app::higurashi::save_menu::Environment,
};
use anyhow::{Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{time::Ticks, vm::command::types::Volume};
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

#[cfg(test)]
mod publication_tests;

#[cfg(test)]
type ReferenceGate = Box<
    dyn FnOnce(std::sync::Weak<CharacterTextures>) -> futures::future::BoxFuture<'static, ()>
        + Send,
>;
#[derive(Default)]
struct Gates {
    #[cfg(test)]
    before_publication: Option<ReferenceGate>,
    #[cfg(test)]
    after_publication: Option<ReferenceGate>,
}

struct Job {
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
struct Loader {
    job: Option<Job>,
    //73f70 publishes the parent archive before temporary/file cleanup and
    //before executor completion. Workers never receive a Model/source pointer.
    published: Arc<Mutex<Option<Arc<CharacterTextures>>>>,
    completed: bool,
}
impl Loader {
    fn new(server: Arc<crate::asset::system::AssetServer>, mut gates: Gates) -> Self {
        let published = Arc::new(Mutex::new(None));
        let destination = published.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            let atlas: Arc<CharacterTextures> = server.load("/manual.txa").await?;
            #[cfg(test)]
            let weak = Arc::downgrade(&atlas);
            #[cfg(test)]
            if let Some(gate) = gates.before_publication.take() {
                gate(weak.clone()).await;
            }
            //Move the decoded owner so no temporary Arc prolongs its lifetime.
            *destination.lock() = Some(atlas);
            #[cfg(test)]
            if let Some(gate) = gates.after_publication.take() {
                gate(weak).await;
            }
            let _ = &mut gates;
            Ok(())
        });
        Self {
            job: Some(Job {
                retirement: task.retirement_handle().unwrap(),
                task: Some(task),
            }),
            published,
            completed: false,
        }
    }
    fn atlas(&self) -> Option<Arc<CharacterTextures>> {
        self.published.lock().clone()
    }
    fn poll(&mut self, model: &mut Model) -> Result<bool> {
        if let Some(output) = self
            .job
            .as_mut()
            .and_then(|job| job.task.as_mut())
            .and_then(|task| task.poll_naive())
        {
            self.job.as_mut().unwrap().task = None;
            output?;
            let atlas = self.atlas().ok_or_else(|| {
                anyhow::anyhow!("Completed Manual worker did not publish its atlas")
            })?;
            // Native72b80 discovers consecutive %03d names; width3 is a
            // minimum format width, not an artificial999-page upper bound.
            let mut count = 0i32;
            while atlas.contains(&format!(
                "{:03}",
                count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("Manual page count overflow"))?
            )) {
                count = count.checked_add(1).unwrap();
            }
            // Desktop malformed-archive boundary: the original permits zero
            // pages and can select-1. Supplied assets have18 valid pages.
            ensure!(count > 0, "Manual archive has no contiguous numbered pages");
            for name in ["caption", "close", "help_manual", "key", "obi"] {
                ensure!(atlas.contains(name), "Manual archive is missing {name}");
            }
            model.publish_resource(count as usize)?;
            self.completed = true;
        }
        Ok(self.completed)
    }
    fn retire_first(&mut self) {
        assert!(
            self.completed && self.atlas().is_some(),
            "Manual First retired before observed resource completion"
        );
        self.job.take();
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        //73798 joins First before72204 releases the parent atlas. Keep shared
        //publication storage and the current atlas alive through nested drain.
        //Cancellation does not run the completed-First prune assertion.
        self.job.take();
        self.published.lock().take();
    }
}
struct FrameServices<'a, 'p> {
    env: &'a Environment,
    area: &'a mut InputArea,
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    requests: &'a mut Vec<u32>,
}
impl Services for FrameServices<'_, '_> {
    fn input(&mut self, model: &mut Model, permission: bool) {
        let captured = if permission {
            self.area.update(model, self.pointer)
        } else {
            self.area.disable(model);
            false
        };
        model.poll_controls(permission, captured, self.buttons);
    }
    fn event(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Cue(name) => self
                .env
                .sounds
                .lock()
                .play(&self.env.server, name, Volume(1.)),
            Action::DisableInput => {}
            Action::ReturnSource(stage) => self.requests.push(stage),
        }
        Ok(())
    }
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    ready: bool,
    loader: &'a mut Loader,
    services: FrameServices<'a, 'p>,
}
impl Mode<Frame<'_, '_>> for Task {
    fn completion(&mut self) -> u32 {
        u32::from(self.done)
    }
    fn retire(&mut self, frame: &mut Frame<'_, '_>) {
        if self.kind == Kind::First {
            frame.loader.retire_first();
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        for task in self.poll_native(
            frame.model,
            if frame.ready { 2 } else { 1 },
            delta,
            permission,
            &mut frame.services,
        )? {
            pushes.push(task);
        }
        Ok(0)
    }
}
pub(in crate::app::higurashi) struct Owner {
    model: Model,
    scope: ModeStack<Task>,
    loader: Loader,
    view: View,
    area: InputArea,
    extent: (u16, u16),
    requests: Vec<u32>,
    fault: Option<String>,
    env: Environment,
}
impl Owner {
    pub fn new(env: Environment, extent: (u16, u16)) -> Result<Self> {
        Self::construct(env, extent, Gates::default())
    }
    fn construct(env: Environment, extent: (u16, u16), gates: Gates) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Manual requires a nonzero input extent"
        );
        let mut model = Model::new();
        let mut scope = ModeStack::default();
        for task in model.take_pending_tasks() {
            scope.push(task);
        }
        let loader = Loader::new(env.server.clone(), gates);
        Ok(Self {
            model,
            scope,
            loader,
            view: View::new(),
            area: InputArea::new(extent.0, extent.1),
            extent,
            requests: Vec::new(),
            fault: None,
            env,
        })
    }
    pub fn complete(&self) -> bool {
        self.model.complete
    }
    pub fn take_requests(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.requests)
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(extent.0 > 0 && extent.1 > 0, "Invalid Manual input extent");
        if extent != self.extent {
            self.area.disable(&mut self.model);
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut self.model);
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid Manual delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Manual failed:{fault}");
        }
        let result = (|| {
            let ready = self.loader.poll(&mut self.model)?;
            self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model: &mut self.model,
                    ready,
                    loader: &mut self.loader,
                    services: FrameServices {
                        env: &self.env,
                        area: &mut self.area,
                        buttons,
                        pointer,
                        requests: &mut self.requests,
                    },
                },
            )?;
            //722a4 polls the borrowed private scope before both clock tails.
            self.model.advance_clocks(delta);
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    #[cfg(test)]
    pub fn state(&self) -> ViewState {
        let mut state = self.model.view_state();
        //72320 acquires the parent archive independently of First/job status.
        //Getters never discover numbered pages or mutate the retained Model.
        state.ready = self.loader.atlas().is_some();
        state
    }
    pub fn render(&self, pass: &mut RenderPass) {
        let atlas = self.loader.atlas();
        let mut state = self.model.view_state();
        state.ready = atlas.is_some();
        self.view.render(&state, atlas.as_deref(), false, pass);
        // All actual private children draw through literal1ec54 RET.
        self.scope.draw_ref(|_| {});
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    #[allow(dead_code, reason = "Inspection hook shared by native tests and optional window probes.")]
    pub fn reference_state(&self) -> (usize, i32, bool, u32, i32) {
        (
            self.model.count,
            self.model.selected,
            !self.model.closing
                && self.model.scroll.is_idle()
                && self
                    .scope
                    .iter()
                    .any(|task| task.kind == Kind::Input && !task.done),
            self.model.scroll.value().to_bits(),
            self.model.contact,
        )
    }
}
