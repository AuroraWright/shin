//! Ger3C570 BustupMode: five remembered selectors and shared-scene Viewer.
//! Real resource/group ownership lives in the app. This model never fabricates ready art.
#[path = "bustup_gallery/artwork.rs"]
pub mod artwork;
#[path = "bustup_gallery/catalog.rs"]
pub mod catalog;
#[path = "bustup_gallery/gerokasu.rs"]
pub mod gerokasu;
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
/// Gerokasu atlas: sheet icons replace charNN/bgNNN, plus the bottom band.
pub const GEROKASU_TEXTURES: &[&str] = &[
    "arrow",
    "bgicon",
    "bottom",
    "button_shadow",
    "caption",
    "charicon",
    "close",
    "dressicon",
    "help_bupmode",
    "hidden",
    "icon",
    "key",
    "num",
    "sizeicon",
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
    /// Gerokasu: `[entry, dress parameter, expression]` at one of four sizes.
    LoadGerokasuBustup {
        request: [i32; 3],
        size: usize,
    },
    LoadGerokasuBackground {
        index: usize,
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
    /// Gerokasu only: the global size row (0x244).
    pub size: Option<usize>,
    pub arrows: [Motion; 12],
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
            size: None,
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
    /// Gerokasu ctor 0x36b60: hard-coded catalog and a size row on 中.
    pub fn new_gerokasu() -> Self {
        let mut this = Self::new(gerokasu::catalog());
        this.size = Some(gerokasu::INITIAL_SIZE);
        this
    }
    pub fn is_gerokasu(&self) -> bool {
        self.size.is_some()
    }
    /// `(index, count)` per row in native row order: Gerokasu inserts size
    /// after the character row.
    pub fn rows(&self) -> Vec<[usize; 2]> {
        let i = self.indices();
        let n = self.counts();
        let mut rows: Vec<_> = (0..5).map(|r| [i[r], n[r]]).collect();
        if let Some(size) = self.size {
            rows.insert(1, [size, gerokasu::SIZES]);
        }
        rows
    }
    /// The Gerokasu2 selection row behind a native row, or None for size.
    fn shared_row(&self, row: usize) -> Option<usize> {
        match (self.is_gerokasu(), row) {
            (false, row) => Some(row),
            (true, 0) => Some(0),
            (true, 1) => None,
            (true, row) => Some(row - 1),
        }
    }
    pub fn gerokasu_request(&self) -> Option<[i32; 3]> {
        let (c, o, p, e) = self.selection.indices();
        self.size.map(|size| gerokasu::request(c, size, o, p, e))
    }
    fn load_action(&self, background: bool) -> Action {
        match (self.size, background) {
            (Some(_), true) => Action::LoadGerokasuBackground {
                index: self.selection.background_index(),
            },
            (Some(size), false) => Action::LoadGerokasuBustup {
                request: self.gerokasu_request().unwrap(),
                size,
            },
            (None, true) => {
                let (primary, secondary) = self.selected_background();
                Action::LoadBackground { primary, secondary }
            }
            (None, false) => Action::LoadBustup {
                id: self.selected_bustup(),
            },
        }
    }
    fn bgm(&self) -> i32 {
        if self.is_gerokasu() { gerokasu::BGM } else { 8 }
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
        let [index, count] = self.rows()[self.focus];
        let next = (index as i64 + i64::from(delta)).clamp(0, count as i64 - 1) as usize;
        if next == index {
            return false;
        }
        let Some(row) = self.shared_row(self.focus) else {
            self.size = Some(next);
            self.sounds.push("move");
            self.loading = true;
            self.actions.push(self.load_action(false));
            return true;
        };
        match row {
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
        self.actions.push(self.load_action(row == 4));
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
                if self.focus + 1 < self.rows().len() {
                    self.focus += 1;
                    self.sounds.push("move");
                }
            } else {
                let d = if input.left {
                    -1
                } else if input.right {
                    1
                } else if input.previous && !self.is_gerokasu() {
                    -10
                } else if input.next && !self.is_gerokasu() {
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
                // Gerokasu 0x3bc00 steps by 4 and scales the stick by 12.
                let (step, stick) = if self.is_gerokasu() {
                    (4., 12.)
                } else {
                    (10., 20.)
                };
                let x = if input.left {
                    -step
                } else if input.right {
                    step
                } else {
                    input.axes[0] * stick
                };
                let y = if input.up {
                    -step
                } else if input.down {
                    step
                } else {
                    input.axes[1] * stick
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
        let [x, y] = if self.is_gerokasu() {
            [80., 70.]
        } else {
            [420., 130.]
        };
        self.viewer_position[0] = self.viewer_position[0].clamp(-x, x);
        self.viewer_position[1] = self.viewer_position[1].clamp(-y, y);
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
                self.actions.push(Action::PlayBgm {
                    id: self.bgm(),
                    fade: 0,
                });
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
        self.actions.push(Action::PlayBgm {
            id: self.bgm(),
            fade: 0,
        });
        self.reset();
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowGeometry {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub arrows: [f32; 2],
    pub arrow_y: f32,
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
        // Labels win first, then enabled left/right arrows, in authored row order.
        for (row, [index, count]) in self.rows().into_iter().enumerate() {
            let geometry = self.row_geometry(row);
            if rect(point, geometry.x, geometry.y, geometry.size) {
                return row as i32 * 3 + 2;
            }
            if index > 0 && rect(point, geometry.arrows[0], geometry.arrow_y, 80.0) {
                return row as i32 * 3;
            }
            if index + 1 < count && rect(point, geometry.arrows[1], geometry.arrow_y, 80.0) {
                return row as i32 * 3 + 1;
            }
        }
        -1
    }
    /// Label square and arrow placement (Gerokasu2 3D380, Gerokasu 0x3a0b0).
    pub fn row_geometry(&self, row: usize) -> RowGeometry {
        let rows = self.rows().len();
        let large = row == 0 || row + 1 == rows;
        let y = if self.is_gerokasu() {
            [216.0, 360.0, 472.0, 584.0, 696.0, 808.0][row]
        } else {
            [216.0, 360.0, 472.0, 584.0, 696.0][row]
        };
        RowGeometry {
            x: if large { 1504.0 } else { 1520.0 },
            y,
            size: if large { 144.0 } else { 112.0 },
            arrows: if large {
                [1424.0, 1648.0]
            } else {
                [1440.0, 1632.0]
            },
            arrow_y: y + if large { 32.0 } else { 16.0 },
        }
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
