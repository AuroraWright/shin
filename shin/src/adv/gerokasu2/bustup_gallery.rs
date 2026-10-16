//! Ger3C570 BustupMode: five remembered selectors and shared-scene Viewer.
//! Real resource/group ownership lives in the app. This model never fabricates ready art.
#[path = "bustup_gallery/artwork.rs"]
pub mod artwork;
#[path = "bustup_gallery/catalog.rs"]
pub mod catalog;
#[path = "cg_gallery/motion.rs"]
pub mod motion;
#[cfg(test)]
#[path = "bustup_gallery/tests.rs"]
mod tests;
use catalog::{Catalog, Selection};
use motion::{Motion, flash};
/// Native43CC0 overrides on freshly loaded shared drawables. Other properties
/// retain the shared layer defaults; raw BustupInfo placement is not applied.
pub const BUSTUP_SLOT: u8 = 10;
pub const REPLACEMENT_TICKS: u32 = 10;
pub const BUSTUP_PROPERTIES: &[(u8, f32)] = &[(5, 1000.), (2, 1000.)];
pub fn background_properties(slot: u8) -> [(u8, f32); 4] {
    [
        (5, 1000. + f32::from(slot)),
        (2, 2000.),
        (12, 2000.),
        (13, 2000.),
    ]
}
pub const BUSTUP_SIZE_PARAMETER: i32 = 0; // Native BD26C chooses /bustup/M/.
pub const ATLAS: &str = "/bupmode.txa";
pub const MASK: &str = "/change.msk";
pub const TEXTURES: &[&str] = &[
    "arrow",
    "button_shadow",
    "caption",
    "close",
    "help_bupmode",
    "hidden",
    "icon",
    "key",
    "num",
    "text",
    "top",
];
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub cancel: bool,
    pub confirm: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub previous: bool,
    pub next: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewerInput {
    pub cancel: bool,
    pub confirm: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub axes: [f32; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    LoadBustup {
        id: u16,
    },
    LoadBackground {
        primary: u16,
        secondary: Option<u16>,
    },
    StopBgm(u32),
    PlayBgm {
        id: i32,
        fade: u32,
    },
    ReturnTitle {
        requested_id: i32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Opening,
    InstallingInput,
    Active,
    CloseFlash,
    ViewerFlash,
    ViewerOpening,
    ViewerInstallingInput,
    ViewerActive,
    ViewerReturning,
    Closed,
}
#[derive(Clone, Debug)]
pub struct Controller {
    pub catalog: Catalog,
    pub selection: Selection,
    pub focus: usize,
    pub phase: Phase,
    pub contact: i32,
    pub selector: i32,
    pub elapsed: f32,
    pub capture_elapsed: f32,
    pub ready: bool,
    pub controls_visibility: Motion,
    pub close_flash: Motion,
    pub confirm_flash: Motion,
    pub panel_offset: Motion,
    pub arrows: [Motion; 10],
    pub scene_x: Motion,
    pub camera: [Motion; 2],
    pub viewer_position: [f32; 2],
    pub viewer_velocity: [f32; 2],
    loading: bool,
    input_installed: bool,
    viewer_input_installed: bool,
    actions: Vec<Action>,
    sounds: Vec<&'static str>,
    pub disabled: u32,
}
impl Controller {
    pub fn new(catalog: Catalog) -> Self {
        let selection = Selection::new(&catalog);
        Self {
            catalog,
            selection,
            focus: 0,
            phase: Phase::Loading,
            contact: -1,
            selector: -1,
            elapsed: 0.,
            capture_elapsed: 0.,
            ready: false,
            controls_visibility: Motion::new(1.),
            close_flash: Motion::new(0.),
            confirm_flash: Motion::new(0.),
            panel_offset: Motion::new(0.),
            arrows: std::array::from_fn(|_| Motion::new(0.)),
            scene_x: Motion::new(-200.),
            camera: std::array::from_fn(|_| Motion::new(0.)),
            viewer_position: [0.; 2],
            viewer_velocity: [0.; 2],
            loading: false,
            input_installed: false,
            viewer_input_installed: false,
            actions: vec![],
            sounds: vec![],
            disabled: 0,
        }
    }
    pub fn assets_ready(&mut self) {
        self.ready = true;
    }
    pub fn replacement_ready(&mut self) {
        self.loading = false;
    }
    pub fn is_loading(&self) -> bool {
        self.loading
    }
    pub fn accepts_input(&self) -> bool {
        matches!(self.phase, Phase::Active | Phase::ViewerActive) && !self.loading
    }
    pub fn in_viewer(&self) -> bool {
        matches!(
            self.phase,
            Phase::ViewerOpening
                | Phase::ViewerInstallingInput
                | Phase::ViewerActive
                | Phase::ViewerReturning
        )
    }
    pub fn indices(&self) -> [usize; 5] {
        let (c, o, p, e) = self.selection.indices();
        [c, o, p, e, self.selection.background_index()]
    }
    pub fn counts(&self) -> [usize; 5] {
        let (c, o, p, _) = self.selection.indices();
        [
            self.catalog.characters.len(),
            self.catalog.characters[c].outfits.len(),
            self.catalog.characters[c].outfits[o].poses.len(),
            self.catalog.characters[c].outfits[o].poses[p]
                .expressions
                .len(),
            self.catalog.backgrounds.len(),
        ]
    }
    pub fn selected_bustup(&self) -> u16 {
        self.selection.bustup(&self.catalog)
    }
    pub fn selected_background(&self) -> (u16, Option<u16>) {
        let b = self.selection.background(&self.catalog);
        (b.primary, b.secondary)
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn change(&mut self, delta: i32) -> bool {
        let i = self.indices();
        let n = self.counts();
        let next =
            (i[self.focus] as i64 + i64::from(delta)).clamp(0, n[self.focus] as i64 - 1) as usize;
        if next == i[self.focus] {
            return false;
        }
        match self.focus {
            0 => {
                self.selection.select_character(next);
            }
            1 => {
                self.selection.select_outfit(next);
            }
            2 => {
                self.selection.select_pose(next);
            }
            3 => {
                self.selection.select_expression(&self.catalog, next);
            }
            4 => {
                self.selection.select_background(&self.catalog, next);
            }
            _ => return false,
        };
        self.sounds.push("move");
        self.loading = true;
        self.actions.push(if self.focus == 4 {
            let (primary, secondary) = self.selected_background();
            Action::LoadBackground { primary, secondary }
        } else {
            Action::LoadBustup {
                id: self.selected_bustup(),
            }
        });
        true
    }
    fn close(&mut self) {
        flash(&mut self.close_flash, true);
        self.phase = Phase::CloseFlash;
    }
    fn hidden(&mut self) {
        flash(&mut self.confirm_flash, true);
        self.panel_offset.target(500., 30., 2, false);
        self.phase = Phase::ViewerFlash;
    }
    pub fn controls(&mut self, permission: bool, captured: bool, input: Input) -> bool {
        if !permission {
            self.disabled += 1;
        } else if !captured {
            if input.cancel {
                self.sounds.push("cancel");
                self.close();
            } else if input.confirm {
                self.sounds.push("decide");
                self.hidden();
            } else if input.up {
                if self.focus > 0 {
                    self.focus -= 1;
                    self.sounds.push("move");
                }
            } else if input.down {
                if self.focus < 4 {
                    self.focus += 1;
                    self.sounds.push("move");
                }
            } else {
                let d = if input.left {
                    -1
                } else if input.right {
                    1
                } else if input.previous {
                    -10
                } else if input.next {
                    10
                } else {
                    0
                };
                if self.change(d) {
                    flash(&mut self.arrows[self.focus * 2 + usize::from(d > 0)], true);
                }
            }
        }
        matches!(
            self.phase,
            Phase::Loading | Phase::Opening | Phase::InstallingInput | Phase::Active
        )
    }
    fn begin_viewer(&mut self) {
        self.scene_x.target(0., 30., 2, false);
        self.controls_visibility.target(0., 10., 0, false);
        self.viewer_position = [0.; 2];
        self.viewer_velocity = [0.; 2];
        self.phase = Phase::ViewerOpening;
    }
    fn return_viewer(&mut self) {
        self.scene_x.target(-200., 30., 2, false);
        for c in &mut self.camera {
            c.target(0., 30., 2, false);
        }
        self.panel_offset.target(0., 30., 2, false);
        self.controls_visibility.target(1., 30., 0, false);
        self.phase = Phase::ViewerReturning;
    }
    pub fn viewer_controls(
        &mut self,
        permission: bool,
        captured: bool,
        input: ViewerInput,
    ) -> bool {
        if !permission {
            self.disabled += 1;
        } else if !captured {
            if input.cancel || input.confirm {
                self.sounds.push("cancel");
                self.return_viewer();
            } else {
                let x = if input.left {
                    -10.
                } else if input.right {
                    10.
                } else {
                    input.axes[0] * 20.
                };
                let y = if input.up {
                    -10.
                } else if input.down {
                    10.
                } else {
                    input.axes[1] * 20.
                };
                if x != 0. || y != 0. {
                    self.viewer_position[0] += x;
                    self.viewer_position[1] += y;
                    self.viewer_velocity = [0.; 2];
                }
                self.clamp_viewer();
            }
        }
        self.phase != Phase::ViewerReturning
    }
    fn clamp_viewer(&mut self) {
        self.viewer_position[0] = self.viewer_position[0].clamp(-420., 420.);
        self.viewer_position[1] = self.viewer_position[1].clamp(-130., 130.);
    }
    pub fn viewer_probe(&mut self) {
        self.viewer_velocity = [0.; 2];
    }
    pub fn viewer_release(&mut self) {
        self.sounds.push("decide");
        self.return_viewer();
    }
    pub fn viewer_drag(&mut self, old: [f32; 2], new: [f32; 2]) {
        self.viewer_position[0] -= new[0] - old[0];
        self.viewer_position[1] = (new[1] - old[1]).mul_add(-0.5625, self.viewer_position[1]);
        self.clamp_viewer();
    }
    pub fn viewer_gesture(&mut self, velocity: [f32; 2]) {
        self.viewer_velocity = [velocity[0], velocity[1] * 0.5625];
    }
    pub fn advance_viewer(&mut self, dt: f32) {
        if self.viewer_velocity != [0.; 2] {
            self.viewer_position[0] -= self.viewer_velocity[0];
            self.viewer_position[1] -= self.viewer_velocity[1];
            let decay = f32::from_bits(0x3f733333).powf(dt);
            self.viewer_velocity[0] *= decay;
            self.viewer_velocity[1] *= decay;
            self.clamp_viewer();
        }
        if self.phase != Phase::ViewerReturning {
            for i in 0..2 {
                self.camera[i].snap(self.viewer_position[i]);
            }
        }
    }
    pub fn update(
        &mut self,
        dt: f32,
        permission: bool,
        captured: bool,
        input: Input,
        viewer: ViewerInput,
    ) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        let phase = self.phase;
        if self.input_installed && matches!(phase, Phase::CloseFlash | Phase::ViewerFlash) {
            self.disabled += 1;
            self.input_installed = false;
        }
        if self.viewer_input_installed && phase == Phase::ViewerReturning {
            self.disabled += 1;
            self.viewer_input_installed = false;
        }
        match phase {
            Phase::Loading if self.ready => {
                self.actions.push(Action::StopBgm(60));
                self.capture_elapsed = 0.;
                self.phase = Phase::Opening;
            }
            Phase::Opening if self.capture_elapsed >= 60. => {
                self.actions.push(Action::PlayBgm { id: 8, fade: 0 });
                self.phase = Phase::InstallingInput;
                self.input_installed = true;
            }
            Phase::InstallingInput => {
                self.phase = Phase::Active;
                self.controls(permission && !self.loading, captured, input);
                if self.phase != Phase::Active {
                    self.input_installed = false;
                }
            }
            Phase::Active => {
                self.controls(permission && !self.loading, captured, input);
                if self.phase != Phase::Active {
                    self.input_installed = false;
                }
            }
            Phase::CloseFlash if !self.close_flash.busy() => {
                self.actions.push(Action::StopBgm(30));
                self.actions.push(Action::ReturnTitle { requested_id: 10 });
                self.phase = Phase::Closed;
            }
            Phase::ViewerFlash if !self.confirm_flash.busy() => self.begin_viewer(),
            Phase::ViewerOpening if !self.scene_x.busy() => {
                self.phase = Phase::ViewerInstallingInput;
                self.viewer_input_installed = true;
            }
            Phase::ViewerInstallingInput => {
                self.phase = Phase::ViewerActive;
                self.viewer_controls(permission && !self.loading, captured, viewer);
                if self.phase != Phase::ViewerActive {
                    self.viewer_input_installed = false;
                }
            }
            Phase::ViewerActive => {
                self.viewer_controls(permission && !self.loading, captured, viewer);
                if self.phase != Phase::ViewerActive {
                    self.viewer_input_installed = false;
                }
            }
            Phase::ViewerReturning if !self.scene_x.busy() && !self.panel_offset.busy() => {
                self.phase = Phase::InstallingInput;
                self.input_installed = true;
                self.reset();
            }
            _ => {}
        }
        if self.in_viewer() {
            self.advance_viewer(dt);
        }
        for c in [
            &mut self.controls_visibility,
            &mut self.close_flash,
            &mut self.confirm_flash,
            &mut self.panel_offset,
            &mut self.scene_x,
        ] {
            c.update(dt);
        }
        for c in &mut self.arrows {
            c.update(dt);
        }
        for c in &mut self.camera {
            c.update(dt);
        }
        if self.phase == Phase::Opening {
            self.capture_elapsed = (self.capture_elapsed + dt).min(60.);
        }
        self.elapsed += dt;
    }
    pub fn recover_return(&mut self) {
        self.phase = Phase::Active;
        self.input_installed = true;
        self.loading = false;
        self.close_flash.snap(0.);
        self.actions.clear();
        self.actions.push(Action::PlayBgm { id: 8, fade: 0 });
        self.reset();
    }
}
fn rect(point: [f32; 2], x: f32, y: f32, size: f32) -> bool {
    point[0] >= x && point[0] < x + size && point[1] >= y && point[1] < y + size
}
fn pointer_rate(clock: &mut Motion, value: f32) {
    clock.target(value, 0.2, 0, true);
}
impl Controller {
    /// The whole background captures contacts, including outside the menu band.
    pub fn probe(&mut self, point: [f32; 2]) -> bool {
        self.selector = -1;
        if point[1] >= 46.0 && point[1] < 154.0 {
            if point[0] >= 1766.0 && point[0] < 1874.0 {
                self.contact = 1;
                return true;
            }
            if point[0] >= 1636.0 && point[0] < 1744.0 {
                self.contact = 2;
                return true;
            }
        }
        self.contact = if point[1] >= 162.0 && point[1] < 980.0 {
            -1
        } else {
            0
        };
        if self.contact == -1 {
            self.selector = self.selector_hit(point);
        }
        true
    }
    pub fn selector_hit(&self, point: [f32; 2]) -> i32 {
        let (c, o, p, e) = self.selection.indices();
        let indices = [c, o, p, e, self.selection.background_index()];
        let counts = [
            self.catalog.characters.len(),
            self.catalog.characters[c].outfits.len(),
            self.catalog.characters[c].outfits[o].poses.len(),
            self.catalog.characters[c].outfits[o].poses[p]
                .expressions
                .len(),
            self.catalog.backgrounds.len(),
        ];
        // Labels win first, then enabled left/right arrows, in authored row order.
        for row in 0..5 {
            let (x, y, size) = match row {
                0 => (1504.0, 216.0, 144.0),
                4 => (1504.0, 696.0, 144.0),
                _ => (1520.0, 248.0 + row as f32 * 112.0, 112.0),
            };
            if rect(point, x, y, size) {
                return row as i32 * 3 + 2;
            }
            let x = if row == 0 || row == 4 { 1424.0 } else { 1440.0 };
            let y = if row == 4 {
                728.0
            } else {
                248.0 + row as f32 * 112.0 + if row == 0 { 0.0 } else { 16.0 }
            };
            if indices[row] > 0 && rect(point, x, y, 80.0) {
                return row as i32 * 3;
            }
            let x = if row == 0 || row == 4 { 1648.0 } else { 1632.0 };
            if indices[row] + 1 < counts[row] && rect(point, x, y, 80.0) {
                return row as i32 * 3 + 1;
            }
        }
        -1
    }
    pub fn reset(&mut self) {
        self.contact = -1;
        self.selector = -1;
    }
    pub fn press(&mut self) {
        match self.contact {
            1 => pointer_rate(&mut self.close_flash, 0.7),
            2 => pointer_rate(&mut self.confirm_flash, 0.7),
            target if target < 0 && self.selector >= 0 => {
                let row = self.selector as usize / 3;
                let part = self.selector as usize % 3;
                self.focus = row;
                if part < 2 {
                    pointer_rate(&mut self.arrows[row * 2 + part], 0.7);
                }
            }
            _ => {}
        }
    }
    pub fn release(&mut self) {
        match self.contact {
            1 => {
                self.sounds.push("decide");
                self.close();
            }
            2 => {
                self.sounds.push("decide");
                self.hidden();
            }
            target if target < 0 && self.selector >= 0 => {
                let row = self.selector as usize / 3;
                let part = self.selector as usize % 3;
                self.focus = row;
                if part < 2 && self.change(if part == 0 { -1 } else { 1 }) {
                    flash(&mut self.arrows[row * 2 + part], true);
                }
            }
            _ => {}
        }
    }
    /// Drag/cancel fade highlights and invalidate the captured target. Gesture
    /// and long-press callbacks are no-ops; release coordinates are ignored.
    pub fn cancel(&mut self) {
        if self.contact >= 0 {
            pointer_rate(&mut self.close_flash, 0.0);
            pointer_rate(&mut self.confirm_flash, 0.0);
            self.contact = 0;
        } else {
            if self.selector >= 0 {
                let part = self.selector as usize % 3;
                if part < 2 {
                    pointer_rate(&mut self.arrows[self.focus * 2 + part], 0.0);
                }
            }
            self.selector = -1;
        }
    }
}
