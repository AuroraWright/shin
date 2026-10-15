//! Actuald8708/98df0/60a34 notification input/delivery owners. The scene
//! advances the shared widget; this wrapper polls phase and its direct Choice.
use super::{
    choice_screen::{Model, view::Screen},
    input::{Buttons, OverlayInput},
    message_ui::HigurashiMessageTextures,
    touch::Overlay,
};
use crate::{
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::{
        higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
        user::higurashi::temporary::Owner,
    },
};
use anyhow::{Result, bail, ensure};
use glam::Vec2;
use parking_lot::Mutex;
use shin_core::time::Ticks;
use shin_render::{PassKind, render_pass::RenderPass};
use std::sync::Arc;

pub(crate) const TIPS_LABELS: [&str; 2] = ["本編を続ける", "TIPSを見る"];
pub(crate) const TIPS_FLAGS: u32 = 0x8003;

/// Notification13/14 call a void callback. Only Tips12 carries original0/1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    Notification,
    Tips(i32),
}

pub(crate) struct ChoiceResources {
    pub server: Arc<AssetServer>,
    pub font: Arc<GpuFontLazy>,
    pub textures: Arc<HigurashiMessageTextures>,
    /// Four fresh OS samples at child creation; never a global MT/shuffle call.
    pub seeds: Box<dyn FnMut() -> [u32; 4] + Send>,
}
enum Factory {
    Native(ChoiceResources),
    #[cfg(test)]
    Reference {
        seeds: Box<dyn FnMut() -> [u32; 4] + Send>,
        fail: bool,
    },
}
enum Child {
    Native(Screen),
    #[cfg(test)]
    Reference {
        model: Model,
        properties: HigurashiProperties,
    },
}
impl Factory {
    fn create(&mut self, route_navigation: bool) -> Result<Child> {
        match self {
            Self::Native(resources) => {
                let seeds = (resources.seeds)();
                let labels = TIPS_LABELS.map(String::from);
                let mut screen = Screen::new_ui(
                    &resources.server,
                    resources.font.clone(),
                    resources.textures.clone(),
                    &labels,
                    TIPS_FLAGS,
                    0,
                    seeds,
                    None,
                )?;
                screen.route_navigation = route_navigation;
                Ok(Child::Native(screen))
            }
            #[cfg(test)]
            Self::Reference { seeds, fail } => {
                let seeds = seeds();
                if *fail {
                    bail!("Checked reference Choice creation failure");
                }
                let specs = TIPS_LABELS
                    .into_iter()
                    .enumerate()
                    .map(|(original, label)| super::choice_screen::Spec {
                        original,
                        label: label.into(),
                        badges: 0,
                        seen: false,
                        height: 64.,
                    })
                    .collect();
                Ok(Child::Reference {
                    model: Model::new(specs, TIPS_FLAGS, None)?,
                    properties: HigurashiProperties::from_seeds(seeds),
                })
            }
        }
    }
}
impl Child {
    fn model(&self) -> &Model {
        match self {
            Self::Native(screen) => &screen.model,
            #[cfg(test)]
            Self::Reference { model, .. } => model,
        }
    }
    fn model_mut(&mut self) -> &mut Model {
        match self {
            Self::Native(screen) => &mut screen.model,
            #[cfg(test)]
            Self::Reference { model, .. } => model,
        }
    }
    fn advance(&mut self, delta: Ticks) {
        match self {
            Self::Native(screen) => screen.advance(delta),
            #[cfg(test)]
            Self::Reference { model, .. } => model.advance(delta),
        }
    }
    fn set_route_navigation(&mut self, value: bool) {
        match self {
            Self::Native(screen) => screen.route_navigation = value,
            #[cfg(test)]
            Self::Reference { .. } => {}
        }
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        match self {
            Self::Native(screen) => screen.render_with_parent(pass, parent, stencil, kind),
            #[cfg(test)]
            Self::Reference { .. } => bail!("CPU metrics Choice cannot supply a renderer"),
        }
    }
    #[cfg(test)]
    fn properties(&self) -> &HigurashiProperties {
        match self {
            Self::Native(screen) => &screen.properties,
            Self::Reference { properties, .. } => properties,
        }
    }
}
#[derive(Clone, Copy, Default)]
enum Contact {
    #[default]
    None,
    Child,
    Widget,
}
struct State {
    controller: Owner,
    widget_identity: usize,
    properties: HigurashiProperties,
    factory: Option<Factory>,
    child: Option<Child>,
    child_started: bool,
    child_resolved: bool,
    completion: Option<Completion>,
    fault: Option<String>,
    cues: Vec<&'static str>,
    contact: Contact,
    route_navigation: bool,
    parent: HigurashiTransform,
}
impl State {
    fn advance_native(&mut self, delta: Ticks) -> Result<()> {
        if let Some(fault) = &self.fault {
            bail!("Notification wrapper:{fault}");
        }
        self.controller.poll_phase();
        self.cues.extend(self.controller.take_cues());
        if !self.child_started && self.controller.question().is_some() {
            let (labels, flags) = self.controller.question().unwrap();
            ensure!(
                labels == TIPS_LABELS && flags == TIPS_FLAGS,
                "Unexpected native Tips child descriptor"
            );
            self.child_started = true; // Creation failure is retained,never retried/defaulted.
            let factory = self
                .factory
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("Tips wrapper has no prepared Choice resources"))?;
            self.child = Some(factory.create(self.route_navigation)?);
        }
        // Native d8cd0..d8d4c: freshly created child gets this SAME delta.
        if let Some(child) = &mut self.child {
            child.advance(delta);
            self.cues.extend(child.model_mut().take_cues());
            if !self.child_resolved {
                if let Some(result) = child.model().result {
                    self.child_resolved = true;
                    self.controller.resolve_question(result);
                    if result == 0 {
                        self.child = None;
                        self.contact = Contact::None;
                    }
                    // Nonzero retains child,phase4 and latched controller result.
                }
            }
        }
        if self.completion.is_none() {
            if let Some(result) = self.controller.result() {
                self.completion = Some(if self.controller.native_id() == 12 {
                    Completion::Tips(result)
                } else {
                    Completion::Notification
                });
            }
        }
        Ok(())
    }
    fn take_cues(&mut self) -> Vec<&'static str> {
        self.cues.extend(self.controller.take_cues());
        if let Some(child) = &mut self.child {
            self.cues.extend(child.model_mut().take_cues());
        }
        std::mem::take(&mut self.cues)
    }
}

/// Clone retains the SAME wrapper/controller/callback owner for the app Job.
/// Native virtual38 scene copying is separate and always returns NULL.
#[derive(Clone)]
pub(crate) struct Shared(Arc<Mutex<State>>);
impl Shared {
    #[cfg(test)]
    pub(crate) fn reference_child(
        &self,
    ) -> Option<(
        u32,
        i32,
        f32,
        bool,
        [f32; 65],
        [u32; 4],
        Vec<(usize, String, f32, f32, f32, f32)>,
    )> {
        let state = self.0.lock();
        let child = state.child.as_ref()?;
        let model = child.model();
        let properties = child.properties();
        Some((
            model.phase,
            model.selected,
            model.opacity,
            model.hidden,
            properties.values(),
            properties.oscillators.map(|oscillator| oscillator.seed),
            model
                .rows
                .iter()
                .map(|row| {
                    (
                        row.spec.original,
                        row.spec.label.clone(),
                        row.y,
                        row.spec.height,
                        row.x.value(),
                        row.alpha.value(),
                    )
                })
                .collect(),
        ))
    }
    pub fn new(
        widget: &Owner,
        seeds: [u32; 4],
        choice: Option<ChoiceResources>,
        route_navigation: bool,
    ) -> Result<Self> {
        Self::from_factory(widget, seeds, choice.map(Factory::Native), route_navigation)
    }
    fn from_factory(
        widget: &Owner,
        seeds: [u32; 4],
        factory: Option<Factory>,
        route_navigation: bool,
    ) -> Result<Self> {
        ensure!(
            (12..=14).contains(&widget.native_id()),
            "Notification wrapper requires kind12..14"
        );
        ensure!(
            widget.native_id() != 12 || factory.is_some(),
            "Tips wrapper requires actual prepared Choice resources"
        );
        let mut properties = HigurashiProperties::from_seeds(seeds);
        properties.control(5, 1., Ticks::ZERO, 0)?;
        Ok(Self(Arc::new(Mutex::new(State {
            controller: widget.retain(),
            widget_identity: widget.widget_identity(),
            properties,
            factory,
            child: None,
            child_started: false,
            child_resolved: false,
            completion: None,
            fault: None,
            cues: Vec::new(),
            contact: Contact::None,
            route_navigation,
            parent: Default::default(),
        }))))
    }
    pub fn widget_identity(&self) -> usize {
        self.0.lock().widget_identity
    }
    pub fn completion(&self) -> Result<Option<Completion>> {
        let state = self.0.lock();
        if let Some(fault) = &state.fault {
            bail!("Notification wrapper:{fault}");
        }
        Ok(state.completion)
    }
    pub fn fault(&self) -> Option<String> {
        self.0.lock().fault.clone()
    }
    pub fn set_route_navigation(&self, value: bool) {
        let mut state = self.0.lock();
        state.route_navigation = value;
        if let Some(child) = &mut state.child {
            child.set_route_navigation(value);
        }
    }
    /// Supplied root optional-child parent; notification drawable transform
    /// is independent and must not be multiplied into the direct Choice.
    pub fn set_parent(&self, parent: HigurashiTransform) {
        self.0.lock().parent = parent;
    }
    pub fn native_copy(&self) -> Option<Self> {
        None
    }
    pub fn advance_checked(&self, delta: Ticks) -> Result<()> {
        let mut state = self.0.lock();
        if let Err(error) = state.advance_native(delta) {
            if state.fault.is_none() {
                state.fault = Some(format!("{error:#}"));
            }
            return Err(error);
        }
        Ok(())
    }
    #[cfg(test)]
    fn reference(
        widget: &Owner,
        seeds: [u32; 4],
        child_seeds: Box<dyn FnMut() -> [u32; 4] + Send>,
        fail: bool,
    ) -> Self {
        Self::from_factory(
            widget,
            seeds,
            Some(Factory::Reference {
                seeds: child_seeds,
                fail,
            }),
            true,
        )
        .unwrap()
    }
}
impl Overlay for Shared {
    fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
    fn input_permissions(&self) -> OverlayInput {
        let state = self.0.lock();
        if state.fault.is_some() {
            return OverlayInput {
                menus: false,
                hide_window: false,
            };
        }
        state.child.as_ref().map_or_else(
            || state.controller.input_permissions(),
            |child| child.model().permissions(),
        )
    }
    fn probe(&mut self, position: Vec2) -> bool {
        let mut state = self.0.lock();
        state.contact = Contact::None;
        if state.fault.is_some() {
            return false;
        }
        if state
            .child
            .as_ref()
            .is_some_and(|child| child.model().probe(position))
        {
            state.contact = Contact::Child;
            true
        } else if state.controller.probe(position) {
            state.contact = Contact::Widget;
            true
        } else {
            false
        }
    }
    fn press(&mut self, position: Vec2) {
        let mut state = self.0.lock();
        if state.fault.is_some() {
            return;
        }
        if state.controller.native_id() != 12 {
            state.controller.press(position);
            return;
        }
        if matches!(state.contact, Contact::Child) {
            if let Some(child) = &mut state.child {
                child.model_mut().press(position);
            }
        }
    }
    fn release(&mut self, position: Vec2) {
        let mut state = self.0.lock();
        if state.fault.is_some() {
            return;
        }
        if state.controller.native_id() != 12 {
            state.controller.release(position);
            return;
        }
        match state.contact {
            Contact::Child => {
                if let Some(child) = &mut state.child {
                    child.model_mut().release(position);
                }
            }
            Contact::Widget => state.controller.release(position),
            Contact::None => {}
        }
    }
    fn cancel(&mut self) {
        self.0.lock().contact = Contact::None;
    }
    fn set_hidden(&mut self, hidden: bool) {
        if let Some(child) = &mut self.0.lock().child {
            child.model_mut().hidden = hidden;
        }
    }
    fn input(&mut self, permission: bool, buttons: Buttons) {
        let mut state = self.0.lock();
        if state.fault.is_some() {
            return;
        }
        if let Some(child) = &mut state.child {
            child.model_mut().input(permission, buttons);
        } else {
            state.controller.input(permission, buttons);
        }
    }
    fn advance(&mut self, delta: Ticks) {
        let _ = self.advance_checked(delta);
    }
    fn fast_forward(&mut self) {
        let mut state = self.0.lock();
        if state.controller.native_id() == 12 {
            if let Some(child) = &mut state.child {
                child.model_mut().fast_forward();
            }
        } else {
            state.properties.fast_forward();
        }
    }
    fn take_cues(&mut self) -> Vec<&'static str> {
        self.0.lock().take_cues()
    }
    fn priority(&self) -> f32 {
        self.0.lock().properties.values()[5]
    }
    fn render(&self, pass: &mut RenderPass, stencil: u8, kind: PassKind) -> Result<()> {
        let state = self.0.lock();
        if let Some(fault) = &state.fault {
            bail!("Notification wrapper:{fault}");
        }
        if let Some(child) = &state.child {
            child.render(pass, &state.parent, stencil, kind)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
