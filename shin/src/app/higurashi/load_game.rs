//! Updatedac8dc LoadGame child and acc08/ad204 storage producers.
//! Desktop paths replace the Switch mount/trial-token services.
use crate::{
    adv::{
        higurashi::{
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
            question::{self, view::View},
        },
        input_area::PointerFrame,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use shin_core::{
    format::save::{UnsupportedSaveVersion, higurashi::SaveData},
    time::Ticks,
    vm::command::types::Volume,
};
use std::{path::PathBuf, sync::Arc};

const MISSING: u32 = 0x202;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum Source {
    Trial = 0,
    Current = 1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Read(Source),
    TrialExists,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuestionKind {
    Recovery(u32),
    Inherit,
    ReadFailure,
    AutoSleep,
}
impl QuestionKind {
    fn prompt(self) -> &'static str {
        match self {
            Self::Recovery(0) => "エラーはありません。ゲームを開始します。",
            Self::Recovery(1) => {
                "セーブデータが壊れているため、ロードできません。@rはじめからゲームを開始します。"
            }
            Self::Recovery(2) => {
                "このセーブデータを使用するためには、ソフトをアップデートする必要があります。@rこのままゲームをはじめることもできますが、その場合は現在のセーブデータは削除されます。"
            }
            Self::Recovery(_) => "不明なエラーです。",
            Self::Inherit => "体験版のセーブデータが見つかりました。@r引き継ぎますか？",
            Self::ReadFailure => {
                "セーブデータを読み込めませんでした。@r保存先をご確認のうえ、再試行してください。"
            }
            Self::AutoSleep => {
                "本体の設定で自動スリープを有効にしている場合、オートモード進行中は一時的に自動スリープが抑制されます。"
            }
        }
    }
    fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Inherit => &["はい", "いいえ"],
            Self::ReadFailure => &["再試行"],
            _ => &["確認"],
        }
    }
    fn selected(self) -> i32 {
        if self == Self::Inherit { 1 } else { 0 }
    }
}

struct Model {
    phase: i32,
    source: Source,
    trial_token: bool,
    decode_status: u32,
    result: u32,
}
trait Services {
    fn start(&mut self, operation: Operation) -> Result<()>;
    fn status(&mut self, model: &mut Model) -> Result<u32>;
    fn question(&mut self, kind: QuestionKind) -> Result<()>;
    fn loaded(&mut self) -> Result<()>;
}
impl Model {
    fn new(trial_token: bool) -> Self {
        Self {
            phase: 0,
            source: Source::Current,
            trial_token,
            decode_status: 0,
            result: 0,
        }
    }
    fn complete(&self, children_empty: bool) -> bool {
        self.phase < 0 && children_empty
    }
    fn finish(&mut self, services: &mut impl Services) -> Result<()> {
        services.loaded()?;
        self.phase = -1;
        Ok(())
    }
    fn poll(&mut self, services: &mut impl Services) -> Result<()> {
        if self.phase == 0 {
            services.start(Operation::Read(self.source))?;
            self.phase = 1;
        }
        if self.phase == 1 && services.status(self)? == 2 {
            self.phase = 2;
        }
        match self.phase {
            2 => {
                if self.result & 0x3fffff == MISSING {
                    if self.source == Source::Current && self.trial_token {
                        self.source = Source::Trial;
                        self.phase = 10;
                    } else {
                        self.finish(services)?;
                    }
                } else if self.decode_status != 0 {
                    services.question(QuestionKind::Recovery(self.decode_status))?;
                    self.phase = i32::MAX;
                } else {
                    self.finish(services)?;
                }
            }
            10 => {
                services.start(Operation::TrialExists)?;
                self.phase = 11;
                self.poll_trial(services)?;
            }
            11 => self.poll_trial(services)?,
            _ => {}
        }
        Ok(())
    }
    fn poll_trial(&mut self, services: &mut impl Services) -> Result<()> {
        if services.status(self)? == 2 {
            if self.result == 0 {
                services.question(QuestionKind::Inherit)?;
                self.phase = i32::MAX;
            } else {
                self.finish(services)?;
            }
        }
        Ok(())
    }
    fn callback(
        &mut self,
        kind: QuestionKind,
        event: question::Callback,
        services: &mut impl Services,
    ) -> Result<()> {
        if event.stage != 1 {
            return Ok(());
        }
        match kind {
            QuestionKind::Inherit if event.button == 0 => self.phase = 0,
            QuestionKind::ReadFailure => {} // Desktop retry handled by its owner.
            _ => self.finish(services)?,
        }
        Ok(())
    }
}

/// Explicit optional trial path corresponds to the native trial-token binding.
/// Existing error/corrupt files remain untouched until a later save boundary.
pub(super) trait Storage: Send + Sync {
    fn trial_token(&self) -> bool;
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>>;
    fn trial_exists(&self) -> Result<bool>;
    fn write(&self, _bytes: &[u8]) -> Result<()> {
        bail!("Higurashi storage does not support writes")
    }
    fn recover_write_errors(&self) -> bool {
        false
    }
}
/// Runtime opts desktop IO into recovery; native/component owners retain their
/// original failure behavior. Errors before write() are never marked as IO.
pub(super) struct DesktopStorage(pub Arc<dyn Storage>);
impl Storage for DesktopStorage {
    fn trial_token(&self) -> bool {
        self.0.trial_token()
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        self.0.read(source)
    }
    fn trial_exists(&self) -> Result<bool> {
        self.0.trial_exists()
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        self.0.write(bytes).map_err(|error| {
            if error.chain().any(|cause| cause.is::<std::io::Error>()) {
                crate::adv::higurashi::save_game::WriteIoFailure::new(error).into()
            } else {
                error
            }
        })
    }
    fn recover_write_errors(&self) -> bool {
        true
    }
}
pub(super) struct Paths {
    pub current: Option<PathBuf>,
    pub trial: Option<PathBuf>,
}
impl Storage for Paths {
    fn write(&self, bytes: &[u8]) -> Result<()> {
        let path = self
            .current
            .as_ref()
            .context("Higurashi progress has no writable path")?;
        crate::adv::persistence::write_atomic(path, bytes)
    }
    fn trial_token(&self) -> bool {
        self.trial.is_some()
    }
    fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
        let path = match source {
            Source::Current => &self.current,
            Source::Trial => &self.trial,
        };
        let Some(path) = path else {
            return Ok(None);
        };
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("Reading Higurashi progress {}", path.display()))
            }
        }
    }
    fn trial_exists(&self) -> Result<bool> {
        let Some(path) = &self.trial else {
            return Ok(false);
        };
        match std::fs::metadata(path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error)
                .with_context(|| format!("Inspecting Higurashi trial progress {}", path.display())),
        }
    }
}
struct Output {
    result: u32,
    decode_status: u32,
    data: Option<SaveData>,
}
fn run(storage: &dyn Storage, operation: Operation) -> Result<Output> {
    let (result, decode_status, data) = match operation {
        Operation::TrialExists => (if storage.trial_exists()? { 0 } else { MISSING }, 0, None),
        Operation::Read(source) => match storage.read(source)? {
            None => (MISSING, 0, None),
            Some(bytes) => match SaveData::decode(&bytes) {
                Ok(data) => (0, 0, Some(data)),
                Err(error) => (
                    0,
                    if error.is::<UnsupportedSaveVersion>() {
                        2
                    } else {
                        1
                    },
                    None,
                ),
            },
        },
    };
    Ok(Output {
        result,
        decode_status,
        data,
    })
}
struct Worker {
    task: Option<shin_tasks::LoaderComputeTask<Result<Output>>>,
    retirement: shin_tasks::LoaderRetirement,
    ready: bool,
}
impl Worker {
    fn start(storage: Arc<dyn Storage>, operation: Operation) -> Self {
        let task = shin_tasks::compute::spawn_loader(move || run(storage.as_ref(), operation));
        let retirement = task.retirement_handle();
        Self {
            task: Some(task),
            retirement,
            ready: false,
        }
    }
    fn poll(&mut self) -> Option<Result<Output>> {
        let output = self.task.as_mut()?.poll_naive()?;
        self.task = None;
        self.ready = true;
        Some(output)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
struct QuestionJob {
    task: Option<shin_tasks::AsyncTask<Result<(question::Question, View)>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for QuestionJob {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(task) = self.task.take() {
                task.cancel_and_retire();
            }
            self.retirement.clone().retire(());
        }
    }
}
pub(super) struct QuestionMode {
    kind: QuestionKind,
    job: QuestionJob,
    ready: Option<(question::Question, View)>,
}
impl QuestionMode {
    pub(super) fn new(server: Arc<AssetServer>, kind: QuestionKind) -> Self {
        let task = shin_tasks::async_io::spawn_loader(async move {
            View::load(
                &server,
                kind.prompt(),
                kind.labels(),
                kind.selected(),
                !matches!(kind, QuestionKind::ReadFailure | QuestionKind::AutoSleep),
            )
            .await
        });
        let retirement = task.retirement_handle().unwrap();
        Self {
            kind,
            job: QuestionJob {
                task: Some(task),
                retirement,
            },
            ready: None,
        }
    }
    pub(super) fn complete(&self) -> bool {
        self.ready.as_ref().is_some_and(|(q, _)| q.complete())
    }
    /// Observe the actual loaded notice, without polling its loader or clocks.
    pub(super) fn touch_diagnostic_notice_ready(&self) -> bool {
        self.kind == QuestionKind::AutoSleep
            && self.ready.as_ref().is_some_and(|(q, _)| q.phase() == 2)
    }
    pub(super) fn cancel_input(&mut self) {
        if let Some((model, _)) = &mut self.ready {
            model.cancel_input(&mut question::CancelOnly);
        }
    }
    pub(super) fn poll_frame(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
        sounds: &Arc<Mutex<SystemSoundPlayer>>,
        server: &AssetServer,
    ) -> Result<Vec<question::Callback>> {
        let mut events = Vec::new();
        self.update(
            delta,
            permission,
            &mut QuestionServices {
                buttons,
                pointer,
                events: &mut events,
                sounds,
                server,
            },
        )?;
        Ok(events.into_iter().map(|(_, event)| event).collect())
    }
    pub(super) fn prepare(&self, context: &mut PreRenderContext) {
        if let Some((model, view)) = &self.ready {
            view.prepare(context, model);
        }
    }
    pub(super) fn render(&self, pass: &mut shin_render::render_pass::RenderPass) {
        if let Some((model, view)) = &self.ready {
            view.render(pass, model);
        }
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_state(&self) -> Option<(QuestionKind, i32, i32)> {
        self.ready
            .as_ref()
            .map(|(q, _)| (self.kind, q.phase(), q.selected()))
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_button_center(&self, index: usize) -> Option<glam::Vec2> {
        let (model, _) = self.ready.as_ref()?;
        let rect = model.buttons().get(index)?.rect;
        Some(
            model.origin()
                + glam::vec2(30., 46.)
                + rect.truncate().truncate()
                + glam::vec2(rect.z, rect.w) * 0.5,
        )
    }
    #[cfg(test)]
    pub(super) fn reference_retirement(&self) -> shin_tasks::LoaderRetirement {
        self.job.retirement.clone()
    }
}
struct QuestionServices<'a, 'b> {
    buttons: Buttons,
    pointer: &'a PointerFrame<'b>,
    events: &'a mut Vec<(QuestionKind, question::Callback)>,
    sounds: &'a Arc<Mutex<SystemSoundPlayer>>,
    server: &'a AssetServer,
}
struct Callbacks<'a, 'b, 'c> {
    kind: QuestionKind,
    services: &'a mut QuestionServices<'b, 'c>,
}
impl question::Services for Callbacks<'_, '_, '_> {
    fn cue(&mut self, cue: question::Cue) {
        let name = match cue {
            question::Cue::Move => "move",
            question::Cue::Cancel => "cancel",
            question::Cue::Confirm => "decide",
            question::Cue::NoSelection => "error",
        };
        self.services
            .sounds
            .lock()
            .play(self.services.server, name, Volume(1.0));
    }
    fn callback(&mut self, event: question::Callback) {
        self.services.events.push((self.kind, event));
    }
}
impl<'a, 'b> Mode<QuestionServices<'a, 'b>> for QuestionMode {
    fn completion(&mut self) -> u32 {
        self.complete() as u32
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut QuestionServices<'a, 'b>,
        _pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        self.update(delta, permission, services)?;
        Ok(0)
    }
}
impl QuestionMode {
    fn update(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut QuestionServices<'_, '_>,
    ) -> Result<()> {
        if let Some(job) = &mut self.job.task {
            if let Some(result) = job.poll_naive() {
                self.job.task = None;
                self.ready = Some(result?);
            }
        }
        if let Some((model, _)) = &mut self.ready {
            let buttons = services.buttons;
            let pointer = services.pointer;
            model.poll(
                delta,
                permission as u32,
                buttons.pressed,
                buttons.repeat,
                pointer,
                false,
                &mut Callbacks {
                    kind: self.kind,
                    services,
                },
            );
        }
        Ok(())
    }
}
struct Bridge<'a> {
    storage: &'a Arc<dyn Storage>,
    worker: &'a mut Option<Worker>,
    data: &'a mut SaveData,
    loaded: &'a mut Option<SaveData>,
    children: &'a mut ModeStack<QuestionMode>,
    server: &'a Arc<AssetServer>,
}
impl Services for Bridge<'_> {
    fn start(&mut self, operation: Operation) -> Result<()> {
        // Retire the old producer before replacing its future/data binding.
        self.worker.take();
        *self.worker = Some(Worker::start(self.storage.clone(), operation));
        Ok(())
    }
    fn status(&mut self, model: &mut Model) -> Result<u32> {
        let worker = self.worker.as_mut().context("LoadGame has no producer")?;
        if let Some(output) = worker.poll() {
            let output = output?;
            model.result = output.result;
            model.decode_status = output.decode_status;
            if let Some(data) = output.data {
                *self.data = data;
            }
        }
        Ok(if worker.ready { 2 } else { 1 })
    }
    fn question(&mut self, kind: QuestionKind) -> Result<()> {
        self.children
            .push(QuestionMode::new(self.server.clone(), kind));
        Ok(())
    }
    fn loaded(&mut self) -> Result<()> {
        ensure!(
            self.loaded.is_none(),
            "LoadGame callback was invoked twice before consumption"
        );
        *self.loaded = Some(self.data.clone());
        Ok(())
    }
}

pub(super) struct LoadGame {
    model: Model,
    // Drop joins producers before data, children, assets or callback storage.
    worker: Option<Worker>,
    data: SaveData,
    children: ModeStack<QuestionMode>,
    loaded: Option<SaveData>,
    storage: Arc<dyn Storage>,
    server: Arc<AssetServer>,
    sounds: Arc<Mutex<SystemSoundPlayer>>,
    retry_phase: Option<i32>,
    fault: Option<String>,
}
impl LoadGame {
    pub fn new(
        storage: Arc<dyn Storage>,
        server: Arc<AssetServer>,
        sounds: Arc<Mutex<SystemSoundPlayer>>,
    ) -> Self {
        sounds
            .lock()
            .set_settings_volume(SaveData::default().settings.common.v3_sysvol);
        Self {
            model: Model::new(storage.trial_token()),
            worker: None,
            data: SaveData::default(),
            children: ModeStack::default(),
            loaded: None,
            storage,
            server,
            sounds,
            retry_phase: None,
            fault: None,
        }
    }
    pub fn complete(&self) -> bool {
        self.model.complete(self.children.is_empty())
    }
    pub fn take_loaded(&mut self) -> Option<SaveData> {
        self.loaded.take()
    }
    pub fn cancel_input(&mut self) {
        for child in self.children.iter_mut() {
            child.cancel_input();
        }
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub(super) fn reference_question(&self) -> Option<(QuestionKind, i32, i32)> {
        self.children.iter().find_map(|child| {
            child
                .ready
                .as_ref()
                .map(|(q, _)| (child.kind, q.phase(), q.selected()))
        })
    }
    #[cfg(test)]
    pub(super) fn reference_retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        self.worker
            .iter()
            .map(|worker| worker.retirement.clone())
            .chain(
                self.children
                    .iter()
                    .map(|question| question.job.retirement.clone()),
            )
            .collect()
    }
    fn bridge(&mut self) -> Bridge<'_> {
        Bridge {
            storage: &self.storage,
            worker: &mut self.worker,
            data: &mut self.data,
            loaded: &mut self.loaded,
            children: &mut self.children,
            server: &self.server,
        }
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        buttons: Buttons,
        pointer: &PointerFrame<'_>,
    ) -> Result<u32> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.0,
            "Invalid LoadGame delta"
        );
        if let Some(fault) = &self.fault {
            bail!("Higurashi save-loading child failed: {fault}");
        }
        let result = (|| {
            if self.retry_phase.is_none() {
                let mut model = std::mem::replace(&mut self.model, Model::new(false));
                let result = model.poll(&mut self.bridge());
                self.model = model;
                if let Err(error) = result {
                    tracing::warn!(%error,"Higurashi progress storage failed");
                    self.retry_phase = Some(if self.model.phase == 11 { 10 } else { 0 });
                    self.children.push(QuestionMode::new(
                        self.server.clone(),
                        QuestionKind::ReadFailure,
                    ));
                }
            }
            let mut events = Vec::new();
            self.children.poll(
                delta,
                permission as u32,
                &mut QuestionServices {
                    buttons,
                    pointer,
                    events: &mut events,
                    sounds: &self.sounds,
                    server: &self.server,
                },
            )?;
            for (kind, event) in events {
                if kind == QuestionKind::ReadFailure && event.stage == 1 {
                    self.model.phase = self
                        .retry_phase
                        .take()
                        .context("Unexpected storage retry callback")?;
                } else {
                    let mut model = std::mem::replace(&mut self.model, Model::new(false));
                    let result = model.callback(kind, event, &mut self.bridge());
                    self.model = model;
                    result?;
                }
            }
            Ok(0) // Original always blocks input below this child.
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        self.children.draw(|child| {
            if let Some((model, view)) = &child.ready {
                view.prepare(context, model)
            }
        });
    }
    pub fn render(&mut self, pass: &mut shin_render::render_pass::RenderPass) {
        self.children.draw(|child| {
            if let Some((model, view)) = &child.ready {
                view.render(pass, model)
            }
        });
    }
}

#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod tests;
