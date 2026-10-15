//! Updated MessageBrowser initializer and retained opening/closing/jump tasks.
//! Atlas/worker publication and reconstruction scheduling belong to the host.
//! Question models/views stay in the retained task stack. No saves are written.
use anyhow::{Result, ensure};
use shin_core::time::Ticks;

use super::{
    browser_controls::{self, BrowserControls, Event},
    browser_navigation::{History as NavigationHistory, Navigation, view::View as NavigationView},
    mode_stack::{Mode, ModePush, ModeStack},
    progress::ProgressMemory,
    question::{self, view::View},
};
use crate::adv::input_area::PointerFrame;
use std::sync::Arc;

pub const JUMP_TEXT: &str = "シーンジャンプを実行します。@rよろしいですか？";
pub const JUMP_BUTTONS: [&str; 2] = ["はい", "いいえ"];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Question {
    owner: u64,
    pub id: u64,
    pub text: &'static str,
    pub buttons: [&'static str; 2],
    /// Original22070 sets the initial selection to the second button.
    pub selected: u32,
}
/// A completed asset load carries its request ID; publication checks that the
/// original task is still alive before transferring model/GPU ownership.
pub struct LoadedQuestion {
    owner: u64,
    id: u64,
    model: question::Question,
    view: View,
}
impl Question {
    pub async fn load(self, server: &crate::asset::system::AssetServer) -> Result<LoadedQuestion> {
        let (model, view) =
            View::load(server, self.text, &self.buttons, self.selected as i32, true).await?;
        Ok(LoadedQuestion {
            owner: self.owner,
            id: self.id,
            model,
            view,
        })
    }
}
#[derive(Clone)]
/// Asset work belongs to one browser and one retained navigation child. A
/// successful load can still expire before publication when that child closes.
pub struct NavigationViewRequest {
    owner: u64,
    pub id: u64,
    rows: Arc<[super::browser_history::Row]>,
}
pub struct LoadedNavigationView {
    owner: u64,
    id: u64,
    view: NavigationView,
}
impl NavigationViewRequest {
    pub async fn load(
        self,
        server: &crate::asset::system::AssetServer,
    ) -> Result<LoadedNavigationView> {
        let view = NavigationView::load(server, self.rows).await?;
        Ok(LoadedNavigationView {
            owner: self.owner,
            id: self.id,
            view,
        })
    }
}
pub trait Services: browser_controls::Services {
    /// Ordinary close invokes the retained parent callback with false at the
    /// start, then true after the slide reaches zero. The host owns its effects.
    fn returned(&mut self, complete: bool);
    fn question(&mut self, question: Question);
    fn question_cue(&mut self, cue: question::Cue);
    /// History configured with concrete rows can request its retained view.
    fn navigation_view(&mut self, _request: NavigationViewRequest) {}
    /// Original17490/275b0 schedules mode0 from the private GameData copy.
    fn reconstruct(&mut self, source: &ProgressMemory, mode: u32) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Initializer,
    Opening,
    CloseFeedback,
    JumpFeedback,
    Closing,
    Question(u64),
}
struct Task {
    kind: Kind,
    done: bool,
    question: Option<RetainedQuestion>,
    navigation_target: Option<u64>,
}
struct RetainedQuestion {
    model: question::Question,
    view: Option<View>,
}
impl Task {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            done: false,
            question: None,
            navigation_target: None,
        }
    }
}
struct InputTask {
    done: bool,
    navigation: Option<NavigationOwner>,
}
struct NavigationOwner {
    id: u64,
    model: Navigation,
    view: Option<NavigationView>,
}
fn close_navigation(inputs: &mut ModeStack<InputTask>, id: u64) {
    for task in inputs.iter_mut() {
        if let Some(owner) = &mut task.navigation {
            if owner.id == id && !task.done {
                owner.model.begin_closing();
                break;
            }
        }
    }
}
pub struct BrowserLifecycle {
    owner: u64,
    controls: BrowserControls,
    source: ProgressMemory,
    boundaries: Vec<u32>,
    tasks: ModeStack<Task>,
    inputs: ModeStack<InputTask>,
    slide: f32,
    complete: bool,
    jumped: bool,
    next_question: u64,
    next_navigation: u64,
    navigation_history: Option<NavigationHistory>,
    navigation_rows: Option<Arc<[super::browser_history::Row]>>,
    atlas_status: u32,
    worker_active: bool,
}
struct TopContext<'a, 'p, S> {
    owner: u64,
    controls: &'a mut BrowserControls,
    source: &'a mut ProgressMemory,
    boundaries: &'a [u32],
    inputs: &'a mut ModeStack<InputTask>,
    slide: &'a mut f32,
    complete: &'a mut bool,
    jumped: &'a mut bool,
    next_question: &'a mut u64,
    atlas_status: u32,
    worker_active: bool,
    pressed: u32,
    repeated: u32,
    frame: &'a PointerFrame<'p>,
    services: &'a mut S,
}
impl<S: Services> Mode<TopContext<'_, '_, S>> for Task {
    fn completion(&mut self) -> u32 {
        u32::from(self.done)
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        c: &mut TopContext<'_, '_, S>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self.kind {
            Kind::Initializer => {
                let v = c.controls.viewport();
                if c.atlas_status == 2 && (!c.worker_active || v.end - v.first >= 100) {
                    let r = c.controls.selected_metadata();
                    let top = r.top + 800.0;
                    let target = if top < 0.0 {
                        r.top
                    } else if top + r.height > 818.0 {
                        (r.top + r.height - 818.0).min(v.extent - 818.0)
                    } else {
                        -800.0
                    };
                    c.controls.set_scroll(target.max(v.minimum));
                    c.controls.set_blocked(false);
                    pushes.push(Task::new(Kind::Opening));
                    self.done = true;
                }
            }
            Kind::Opening => {
                *c.slide += delta.as_f32() * f32::from_bits(0x3dcc_cccd);
                if *c.slide >= 1.0 {
                    *c.slide = 1.0;
                    c.inputs.push(InputTask {
                        done: false,
                        navigation: None,
                    });
                    self.done = true;
                }
            }
            Kind::CloseFeedback => {
                if !c.controls.close_flash_waiting() {
                    c.controls.set_blocked(true);
                    if !*c.jumped {
                        c.services.returned(false);
                    }
                    pushes.push(Task::new(Kind::Closing));
                    self.done = true;
                }
            }
            Kind::JumpFeedback => {
                let waiting = c
                    .controls
                    .flashes(c.controls.selected())
                    .is_some_and(|f| f.jump.is_waiting());
                if !waiting {
                    let id = *c.next_question;
                    *c.next_question += 1;
                    let mut task = Task::new(Kind::Question(id));
                    task.navigation_target = self.navigation_target;
                    pushes.push(task);
                    c.services.question(Question {
                        owner: c.owner,
                        id,
                        text: JUMP_TEXT,
                        buttons: JUMP_BUTTONS,
                        selected: 1,
                    });
                    self.done = true;
                }
            }
            Kind::Closing => {
                *c.slide += delta.as_f32() * f32::from_bits(0xbdcc_cccd);
                if *c.slide <= 0.0 {
                    *c.slide = 0.0;
                    if *c.jumped {
                        c.services.reconstruct(c.source, 0)?;
                    } else {
                        c.services.returned(true);
                        *c.complete = true;
                    }
                    self.done = true;
                }
            }
            Kind::Question(_) => {
                if let Some(owner) = &mut self.question {
                    let mut services = QuestionServices {
                        services: c.services,
                        callbacks: Vec::new(),
                    };
                    owner.model.poll(
                        delta,
                        u32::from(permission),
                        c.pressed,
                        c.repeated,
                        c.frame,
                        false,
                        &mut services,
                    );
                    for callback in services.callbacks {
                        if accept_question(
                            callback.stage,
                            callback.button,
                            c.controls,
                            c.source,
                            c.boundaries,
                            c.jumped,
                        ) {
                            if let Some(id) = self.navigation_target {
                                close_navigation(c.inputs, id);
                            }
                            pushes.push(Task::new(Kind::Closing));
                        }
                    }
                    self.done = owner.model.complete();
                }
            }
        }
        // Original generic timed tasks block input even on their final poll.
        Ok(0)
    }
}
struct QuestionServices<'a, S> {
    services: &'a mut S,
    callbacks: Vec<question::Callback>,
}
impl<S: Services> question::Services for QuestionServices<'_, S> {
    fn cue(&mut self, cue: question::Cue) {
        self.services.question_cue(cue);
    }
    fn callback(&mut self, callback: question::Callback) {
        self.callbacks.push(callback);
    }
}
fn accept_question(
    stage: u32,
    button: i32,
    controls: &mut BrowserControls,
    source: &mut ProgressMemory,
    boundaries: &[u32],
    jumped: &mut bool,
) -> bool {
    if stage != 1 || button != 0 {
        return false;
    }
    *jumped = true;
    let active = source.progress().active_index();
    let entry = &mut source.progress_mut().entries[active];
    entry.replay_target = entry.save_position;
    entry.save_position = boundaries[controls.selected()];
    controls.set_blocked(true);
    true
}
struct InputContext<'a, 'p, S> {
    owner: u64,
    controls: &'a mut BrowserControls,
    tasks: &'a mut ModeStack<Task>,
    pressed: u32,
    repeated: u32,
    frame: &'a PointerFrame<'p>,
    mouse_release: Option<crate::adv::input_area::MouseRelease>,
    services: &'a mut S,
    navigation_history: Option<&'a NavigationHistory>,
    navigation_rows: Option<&'a Arc<[super::browser_history::Row]>>,
    next_navigation: &'a mut u64,
}
struct ControlServices<'a, S> {
    tasks: &'a mut ModeStack<Task>,
    services: &'a mut S,
    navigation_target: Option<u64>,
    open_navigation: bool,
}
impl<S: Services> browser_controls::Services for ControlServices<'_, S> {
    fn event(&mut self, e: Event) {
        match e {
            Event::Close => self.tasks.push(Task::new(Kind::CloseFeedback)),
            Event::Jump(_) => {
                let mut task = Task::new(Kind::JumpFeedback);
                task.navigation_target = self.navigation_target;
                self.tasks.push(task);
            }
            Event::Navigation => self.open_navigation = true,
            e => self.services.event(e),
        }
    }
    fn voice_is_playing(&self) -> bool {
        self.services.voice_is_playing()
    }
}
impl<S: Services> Mode<InputContext<'_, '_, S>> for InputTask {
    fn completion(&mut self) -> u32 {
        u32::from(self.done)
    }
    fn retire(&mut self, c: &mut InputContext<'_, '_, S>) {
        if let Some(owner) = &mut self.navigation {
            owner.model.retire(c.controls);
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        c: &mut InputContext<'_, '_, S>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        let mut services = ControlServices {
            tasks: c.tasks,
            services: c.services,
            navigation_target: self.navigation.as_ref().map(|owner| owner.id),
            open_navigation: false,
        };
        if let Some(owner) = &mut self.navigation {
            owner.model.poll_with_mouse_release(
                delta,
                u32::from(permission),
                c.pressed,
                c.repeated,
                c.frame,
                c.controls,
                &mut services,
                c.mouse_release,
            );
            self.done = owner.model.complete();
            return Ok(0);
        }
        self.done = !c.controls.input_with_mouse_release(
            u8::from(permission),
            c.pressed,
            c.repeated,
            c.frame,
            &mut services,
            c.mouse_release,
        );
        if services.open_navigation {
            if let Some(history) = c.navigation_history {
                let model = Navigation::new(
                    &history.row_indices,
                    &history.markers,
                    history.current,
                    c.controls,
                )?;
                let id = *c.next_navigation;
                *c.next_navigation += 1;
                pushes.push(InputTask {
                    done: false,
                    navigation: Some(NavigationOwner {
                        id,
                        model,
                        view: None,
                    }),
                });
                if let Some(rows) = c.navigation_rows {
                    services.services.navigation_view(NavigationViewRequest {
                        owner: c.owner,
                        id,
                        rows: rows.clone(),
                    });
                }
            } else {
                services.services.event(Event::Navigation);
            }
        }
        Ok(0)
    }
}
impl BrowserLifecycle {
    /// Metadata and row boundaries must come from the same retained history.
    /// Original initializer waits for atlas status2 and100 measured rows while
    /// the worker is active; a finished worker may publish a shorter history.
    pub fn new(
        controls: BrowserControls,
        boundaries: Vec<u32>,
        source: ProgressMemory,
    ) -> Result<Self> {
        ensure!(
            boundaries.len() >= controls.viewport().end,
            "Missing browser row boundaries"
        );
        let mut tasks = ModeStack::default();
        tasks.push(Task::new(Kind::Initializer));
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| anyhow::anyhow!("Browser owner IDs exhausted"))?;
        Ok(Self {
            owner,
            controls,
            source,
            boundaries,
            tasks,
            inputs: ModeStack::default(),
            slide: 0.0,
            complete: false,
            jumped: false,
            next_question: 1,
            next_navigation: 1,
            navigation_history: None,
            navigation_rows: None,
            atlas_status: 0,
            worker_active: true,
        })
    }
    pub fn controls(&self) -> &BrowserControls {
        &self.controls
    }
    #[cfg(test)]
    pub(crate) fn reference_input_ready(&self) -> bool {
        self.input_ready()
    }
    pub(crate) fn input_ready(&self) -> bool {
        let mut borrowed_input = false;
        self.inputs.draw_ref(|task| {
            if !task.done && task.navigation.is_none() {
                borrowed_input = true;
            }
        });
        borrowed_input
            && self.inputs.iter().all(|task| task.done || task.navigation.is_none())
            && self.tasks.iter().all(|task| task.done)
            && self.slide == 1.0
            && !self.complete
            && !self.controls.blocked()
            && !self.controls.scroll_waiting()
    }
    pub fn controls_mut(&mut self) -> &mut BrowserControls {
        &mut self.controls
    }
    pub fn cancel_input(&mut self) {
        struct Cancellation;
        impl super::browser_controls::Services for Cancellation {
            fn event(&mut self, _: super::browser_controls::Event) {
                unreachable!("Contact cancellation has no browser action")
            }
            fn voice_is_playing(&self) -> bool { false }
        }
        self.controls.cancel_input(&mut Cancellation);
        for task in self.inputs.iter_mut() {
            if let Some(navigation) = &mut task.navigation {
                navigation.model.cancel_input(&mut self.controls, &mut Cancellation);
            }
        }
        for task in self.tasks.iter_mut() {
            if let Some(question) = &mut task.question {
                question.model.cancel_input(&mut question::CancelOnly);
            }
        }
    }
    pub fn private_source(&self) -> &ProgressMemory {
        &self.source
    }
    pub fn slide(&self) -> f32 {
        self.slide
    }
    pub fn completion_word(&self) -> u32 {
        u32::from(self.complete)
    }
    pub fn jumped(&self) -> bool {
        self.jumped
    }
    pub fn publish_readiness(&mut self, atlas_status: u32, worker_active: bool) {
        self.atlas_status = atlas_status;
        self.worker_active = worker_active;
    }
    pub fn configure_navigation(&mut self, history: NavigationHistory) -> Result<()> {
        ensure!(
            history.row_indices.len() == self.controls.row_count(),
            "Navigation history disagrees with browser metadata"
        );
        ensure!(
            self.controls.navigation() == 0,
            "Navigation child already active"
        );
        self.navigation_history = Some(history);
        self.navigation_rows = None;
        Ok(())
    }
    pub fn configure_navigation_rendering(
        &mut self,
        rows: Arc<[super::browser_history::Row]>,
        markers: Vec<u32>,
        current: usize,
    ) -> Result<()> {
        let indices = rows.iter().map(|row| row.index).collect();
        self.configure_navigation(NavigationHistory::new(indices, markers, current)?)?;
        self.navigation_rows = Some(rows);
        Ok(())
    }
    pub(crate) fn wants_navigation_view(&self, id: u64) -> bool {
        self.inputs.iter().any(|task| {
            !task.done
                && task
                    .navigation
                    .as_ref()
                    .is_some_and(|n| n.id == id && n.view.is_none())
        })
    }
    pub(crate) fn wants_question_view(&self, id: u64) -> bool {
        self.tasks
            .iter()
            .any(|task| !task.done && task.kind == Kind::Question(id) && task.question.is_none())
    }
    pub fn publish_navigation_view(&mut self, loaded: LoadedNavigationView) -> Result<()> {
        ensure!(
            loaded.owner == self.owner,
            "Navigation view belongs to another browser"
        );
        let owner = self
            .inputs
            .iter_mut()
            .filter(|task| !task.done)
            .filter_map(|task| task.navigation.as_mut())
            .find(|owner| owner.id == loaded.id)
            .ok_or_else(|| anyhow::anyhow!("Expired navigation view"))?;
        ensure!(owner.view.is_none(), "Navigation view already published");
        owner.view = Some(loaded.view);
        Ok(())
    }
    pub fn prepare_navigation_views(
        &mut self,
        assets: &crate::asset::system::AssetServer,
    ) -> Result<()> {
        let mut result = Ok(());
        self.inputs.draw(|task| {
            if result.is_ok() {
                if let Some(owner) = &mut task.navigation {
                    if let Some(view) = &mut owner.view {
                        result = view.prepare(assets, &owner.model);
                    }
                }
            }
        });
        result
    }
    pub fn render_navigation_views(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        let controls = &self.controls;
        self.inputs.draw(|task| {
            if let Some(owner) = &task.navigation {
                if let Some(view) = &owner.view {
                    view.render(pass, &owner.model, controls);
                }
            }
        });
    }
    pub fn draw_navigation(&mut self, mut draw: impl FnMut(&Navigation)) {
        self.inputs.draw(|task| {
            if let Some(owner) = &task.navigation {
                draw(&owner.model);
            }
        });
    }
    pub fn publish_question(&mut self, loaded: LoadedQuestion) -> Result<()> {
        ensure!(
            loaded.owner == self.owner,
            "Question belongs to another browser"
        );
        self.attach_question(loaded.id, loaded.model, Some(loaded.view))
    }
    fn attach_question(
        &mut self,
        id: u64,
        model: question::Question,
        view: Option<View>,
    ) -> Result<()> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.kind == Kind::Question(id) && !t.done)
            .ok_or_else(|| anyhow::anyhow!("Expired browser question"))?;
        ensure!(
            task.question.is_none(),
            "Browser question already published"
        );
        task.question = Some(RetainedQuestion { model, view });
        Ok(())
    }
    pub fn prepare_questions(&mut self, context: &mut crate::render::PreRenderContext) {
        self.tasks.draw(|task| {
            if let Some(RetainedQuestion {
                model,
                view: Some(view),
            }) = &task.question
            {
                view.prepare(context, model);
            }
        });
    }
    pub fn render_questions(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        self.tasks.draw(|task| {
            if let Some(RetainedQuestion {
                model,
                view: Some(view),
            }) = &task.question
            {
                view.render(pass, model);
            }
        });
    }
    /// Original23644 emits stage0 when dialog closing begins, then stage1
    /// after its tweens and five-tick tail finish. Button0 is Yes,1 is No,
    /// and-1 is cancellation. Only stage1/Yes mutates the private source.
    /// The callback reads the *current* browser row and preserves complete
    /// response vectors, settings and the inactive entry. Its UI retires next.
    pub fn question_callback(&mut self, id: u64, stage: u32, button: i32) -> Result<()> {
        ensure!(
            self.tasks
                .iter()
                .any(|t| t.kind == Kind::Question(id) && !t.done && t.question.is_none()),
            "Expired browser question"
        );
        let navigation = self
            .tasks
            .iter()
            .find(|task| task.kind == Kind::Question(id))
            .and_then(|task| task.navigation_target);
        if accept_question(
            stage,
            button,
            &mut self.controls,
            &mut self.source,
            &self.boundaries,
            &mut self.jumped,
        ) {
            if let Some(id) = navigation {
                close_navigation(&mut self.inputs, id);
            }
            self.tasks.push(Task::new(Kind::Closing));
        }
        Ok(())
    }
    pub fn finish_question(&mut self, id: u64) -> Result<()> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.kind == Kind::Question(id) && !t.done && t.question.is_none());
        let task = task.ok_or_else(|| anyhow::anyhow!("Expired browser question"))?;
        task.done = true;
        Ok(())
    }
    /// Includes children completing in the last poll; excludes newly pushed
    /// children until their first poll, matching the native borrowed draw list.
    pub fn draw_questions(&mut self, mut draw: impl FnMut(u64)) {
        self.tasks.draw(|t| {
            if let Kind::Question(id) = t.kind {
                draw(id);
            }
        });
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        duck_music: bool,
        services: &mut impl Services,
    ) -> Result<u32> {
        self.poll_with_mouse_release(
            delta, permission, pressed, repeated, frame, duck_music, services, None,
        )
    }
    pub(crate) fn poll_with_mouse_release(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        duck_music: bool,
        services: &mut impl Services,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> Result<u32> {
        let allowed = self.tasks.poll(
            delta,
            permission,
            &mut TopContext {
                owner: self.owner,
                controls: &mut self.controls,
                source: &mut self.source,
                boundaries: &self.boundaries,
                inputs: &mut self.inputs,
                slide: &mut self.slide,
                complete: &mut self.complete,
                jumped: &mut self.jumped,
                next_question: &mut self.next_question,
                atlas_status: self.atlas_status,
                worker_active: self.worker_active,
                pressed,
                repeated,
                frame,
                services,
            },
        )?;
        self.inputs.poll(
            delta,
            u32::from(allowed),
            &mut InputContext {
                owner: self.owner,
                controls: &mut self.controls,
                tasks: &mut self.tasks,
                pressed,
                repeated,
                frame,
                mouse_release,
                services,
                navigation_history: self.navigation_history.as_ref(),
                navigation_rows: self.navigation_rows.as_ref(),
                next_navigation: &mut self.next_navigation,
            },
        )?;
        self.controls.tick(delta, duck_music, services);
        Ok(0)
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod desktop_tests;
