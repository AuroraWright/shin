//! Live SHOWCHARS screen and the native flash-before-action coroutines.
use std::sync::Arc;

use glam::Vec2;
use shin_core::{
    format::scenario::Scenario,
    time::{Easing, Ticks, Tween, Tweener},
};
use shin_render::render_pass::RenderPass;

use crate::adv::{
    characters::text::CharacterText,
    characters::view::{CharacterScreenTransition, CharacterTextures},
    characters::{CharacterButton, CharacterButtonState, CharacterEdit, CharacterModel},
};
use crate::asset::system::AssetServer;

#[cfg(all(test, target_os = "macos"))]
mod failure_reference;

/// Native text viewport callbacks0x54cd0/0x54d50 and inertia0x50d1c..0x50dc4.
#[derive(Debug, Default)]
pub struct CharacterTextScroll {
    pub position: f32,
    velocity: f32,
    maximum: f32,
}
impl CharacterTextScroll {
    pub fn set_maximum(&mut self, maximum: f32) {
        self.maximum = maximum.max(0.0);
        self.position = self.position.clamp(0.0, self.maximum);
    }
    pub fn reset(&mut self) {
        self.position = 0.0;
        self.velocity = 0.0;
    }
    pub fn axis(&mut self, amount: f32) {
        self.position = (self.position + amount * 16.0).clamp(0.0, self.maximum);
        self.velocity = 0.0;
    }
    pub fn drag(&mut self, delta_y: f32) {
        self.position = (self.position - delta_y).clamp(0.0, self.maximum);
        self.velocity = 0.0;
    }
    /// Input manager provides the pointer's release velocity in pixels per tick.
    pub fn release(&mut self, velocity_y: f32) {
        self.velocity = velocity_y;
    }
    pub fn update(&mut self, delta: Ticks) {
        if self.velocity == 0.0 {
            return;
        }
        let next = (self.position - self.velocity * delta.as_f32()).clamp(0.0, self.maximum);
        if (self.position - next).abs() < f32::EPSILON {
            self.velocity = 0.0;
            return;
        }
        self.position = next;
        self.velocity *= 0.95f32.powf(delta.as_f32());
        if self.velocity.abs() < 0.1 {
            self.velocity = 0.0;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerTarget {
    Close,
    Cell(usize),
    Button(CharacterButton),
    Text,
    None,
}

/// Single-pointer specialization of native InputArea0x6b110 (canvas coordinates).
#[derive(Debug, Default)]
struct CharacterPointer {
    held: bool,
    origin: Vec2,
    previous: Vec2,
    previous_delta: Vec2,
    dragging: bool,
    target: Option<PointerTarget>,
    consumed: bool,
}
fn move_flash(tweener: &mut Tweener, target: f32) {
    let current = tweener.value();
    tweener.fast_forward_to(current);
    tweener.enqueue(
        target,
        Tween::linear(Ticks::from_f32((target - current).abs() / 0.2)),
    );
}

#[derive(Debug)]
enum PendingAction {
    Page(u8),
    Edit(CharacterEdit),
}
#[derive(Debug)]
struct Pending {
    action: PendingAction,
    // Native tasks retain their target even when selection changes before completion.
    button_waits: Vec<usize>,
    cell_wait: Option<usize>,
}

#[derive(Debug)]
pub struct CharacterScreenModel {
    pub characters: CharacterModel,
    pub transition: CharacterScreenTransition,
    page_scroll: Tweener,
    button_flashes: [Tweener; 5],
    close_flash: Tweener,
    cell_flashes: Vec<Tweener>,
    selections: Vec<Tweener>,
    page_colors: [Tweener; 3],
    pending: Vec<Pending>,
    closing_requested: bool,
    pub hovered: Option<usize>,
    pub text_scroll: CharacterTextScroll,
    text_revision: u64,
    sprite_revision: u64,
    pending_sounds: Vec<&'static str>,
    frame_prepared: bool,
    advance_transition: bool,
}

fn flash(tweener: &mut Tweener) {
    let current = tweener.value();
    tweener.fast_forward_to(current);
    // Native flag0x40 means speed, not duration (0xbb94c..0xbb980).
    tweener.enqueue(
        0.7,
        Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
    );
    tweener.enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
}
impl CharacterScreenModel {
    pub fn new(characters: CharacterModel) -> Self {
        let selections = characters
            .cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                Tweener::new(if characters.selection_on_page(c.page) == Some(i) {
                    1.0
                } else {
                    0.0
                })
            })
            .collect();
        let palette = super::view::character_page_palette(characters.page);
        Self {
            selections,
            page_colors: palette.map(Tweener::new),
            page_scroll: Tweener::new(characters.page_scroll()),
            cell_flashes: vec![Tweener::new(0.0); characters.cells.len()],
            characters,
            transition: Default::default(),
            button_flashes: std::array::from_fn(|_| Tweener::new(0.0)),
            close_flash: Tweener::new(0.0),
            pending: Vec::new(),
            closing_requested: false,
            hovered: None,
            text_scroll: Default::default(),
            text_revision: 0,
            sprite_revision: 0,
            pending_sounds: Vec::new(),
            frame_prepared: false,
            advance_transition: true,
        }
    }
    pub fn is_closed(&self) -> bool {
        self.transition.is_closed()
    }
    fn accepts_input(&self) -> bool {
        self.transition.is_open() && !self.closing_requested
    }
    fn details_changed(&mut self) {
        self.text_revision += 1;
        self.sprite_revision += 1;
        self.text_scroll.reset();
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.pending_sounds)
    }
    pub fn request_close(&mut self) {
        self.close_with_sound("cancel");
    }
    fn close_with_sound(&mut self, sound: &'static str) {
        if !self.accepts_input() {
            return;
        }
        self.pending_sounds.push(sound);
        self.closing_requested = true;
        self.hovered = None;
        flash(&mut self.close_flash);
    }
    pub fn request_button(&mut self, button: CharacterButton) {
        if !self.accepts_input()
            || self.characters.button_states()[button as usize] != CharacterButtonState::Enabled
        {
            return;
        }
        match button {
            CharacterButton::PreviousPage => self.request_page(-1),
            CharacterButton::NextPage => self.request_page(1),
            CharacterButton::Execute => self.request_state(1),
            CharacterButton::Resurrect => self.request_state(-1),
            CharacterButton::Variant => self.request_variant(),
        }
    }
    pub fn request_page(&mut self, direction: i32) {
        if !self.accepts_input() || !self.page_scroll.is_idle() {
            return;
        }
        let Some(page) = self.characters.adjacent_page(direction) else {
            return;
        };
        self.pending_sounds.push("move");
        let button = usize::from(direction > 0);
        flash(&mut self.button_flashes[button]);
        self.pending.push(Pending {
            action: PendingAction::Page(page),
            button_waits: vec![0, 1],
            cell_wait: None,
        });
    }
    pub fn request_state(&mut self, direction: i32) {
        if !self.accepts_input() {
            return;
        }
        let Some(edit) = self.characters.plan_state_change(direction) else {
            return;
        };
        let CharacterEdit::State {
            cell,
            definition,
            target,
        } = edit
        else {
            unreachable!()
        };
        let current = self.characters.cells[cell].definitions[definition]
            .state()
            .unwrap()
            .index;
        let next = self.characters.cells[cell].definitions[definition].states[target].index;
        let button = if next > current { 2 } else { 3 };
        self.pending_sounds
            .push(if button == 2 { "execute" } else { "resurrect" });
        flash(&mut self.button_flashes[button]);
        self.pending.push(Pending {
            action: PendingAction::Edit(edit),
            button_waits: vec![2, 3],
            cell_wait: None,
        });
    }
    pub fn request_variant(&mut self) {
        if !self.accepts_input() {
            return;
        }
        let Some(edit) = self.characters.plan_variant_cycle() else {
            return;
        };
        self.pending_sounds.push("decide");
        flash(&mut self.button_flashes[4]);
        self.pending.push(Pending {
            action: PendingAction::Edit(edit),
            button_waits: vec![4],
            cell_wait: None,
        });
    }
    pub fn request_character_cycle(&mut self) {
        if !self.accepts_input() {
            return;
        }
        let Some(edit) = self.characters.plan_character_cycle() else {
            return;
        };
        let CharacterEdit::Definition { cell, .. } = edit else {
            unreachable!()
        };
        self.pending_sounds.push("decide");
        flash(&mut self.cell_flashes[cell]);
        self.pending.push(Pending {
            action: PendingAction::Edit(edit),
            button_waits: Vec::new(),
            cell_wait: Some(cell),
        });
    }
    fn change_selection_indicator(&mut self, previous: Option<usize>, next: usize) {
        for (index, target, speed) in previous
            .map(|i| (i, 0.0, 0.1))
            .into_iter()
            .chain(std::iter::once((next, 1.0, 0.25)))
        {
            let tweener = &mut self.selections[index];
            let current = tweener.value();
            tweener.fast_forward_to(current);
            tweener.enqueue(
                target,
                Tween::linear(Ticks::from_f32((target - current).abs() / speed)),
            );
        }
    }
    pub fn grid_effects(&self) -> ([f32; 3], Vec<f32>, Vec<f32>) {
        (
            std::array::from_fn(|i| self.page_colors[i].value()),
            self.selections.iter().map(Tweener::value).collect(),
            self.cell_flashes.iter().map(Tweener::value).collect(),
        )
    }
    pub fn navigate(&mut self, direction: Vec2) {
        let previous = self.characters.selected_index();
        if self.accepts_input() && self.page_scroll.is_idle() && self.characters.navigate(direction)
        {
            self.change_selection_indicator(previous, self.characters.selected_index().unwrap());
            self.pending_sounds.push("move");
            self.details_changed();
            self.hovered = None;
        }
    }
    /// Native0x53bf4..0x53f88 handles only the first matching action each frame.
    pub fn handle_controller_input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        scroll_axis: f32,
    ) {
        self.prepare_frame();
        use crate::app::AppAction as A;
        if !self.accepts_input() {
            return;
        }
        let page_direction = if state[A::CharacterPageUp].is_clicked_or_repeated {
            -1
        } else if state[A::CharacterPageDown].is_clicked_or_repeated {
            1
        } else {
            0
        };
        if page_direction != 0 && self.characters.adjacent_page(page_direction).is_some() {
            self.request_page(page_direction);
            return;
        }
        let x = if state[A::Left].is_clicked_or_repeated {
            -1.0
        } else if state[A::Right].is_clicked_or_repeated {
            1.0
        } else {
            0.0
        };
        let y = if state[A::Up].is_clicked_or_repeated {
            -1.0
        } else if state[A::AnyDown].is_clicked_or_repeated {
            1.0
        } else {
            0.0
        };
        if x != 0.0 || y != 0.0 {
            self.navigate(glam::vec2(x, y));
        } else if state[A::CharacterVariant].is_clicked {
            self.request_variant();
        } else if state[A::CharacterState].is_clicked {
            self.request_state(1);
        } else if state[A::Enter].is_clicked {
            self.request_character_cycle();
        } else if state[A::Cancel].is_clicked {
            self.request_close();
        } else {
            let axis = if state[A::TextScrollUp].is_clicked_or_repeated {
                -1.0
            } else if state[A::TextScrollDown].is_clicked_or_repeated {
                1.0
            } else {
                scroll_axis
            };
            if axis != 0.0 {
                self.text_scroll.axis(axis);
            }
        }
    }
    pub fn pointer(&mut self, position: Vec2, clicked: bool) {
        if !self.accepts_input() {
            return;
        }
        if clicked && (1764.0..1848.0).contains(&position.x) && (32.0..116.0).contains(&position.y)
        {
            self.close_with_sound("decide");
            return;
        }
        self.hovered = self.characters.cell_at(
            position,
            self.page_scroll.value(),
            !self.page_scroll.is_idle(),
        );
        if !clicked {
            return;
        }
        if let Some(cell) = self.hovered {
            let previous = self.characters.selected_index();
            if self.characters.selected_index() == Some(cell) {
                self.request_character_cycle();
            } else if self.characters.select(cell) {
                flash(&mut self.cell_flashes[cell]);
                self.change_selection_indicator(previous, cell);
                self.pending_sounds.push("decide");
                self.details_changed();
            }
        } else if let Some(button) = self
            .characters
            .button_at(position, !self.page_scroll.is_idle())
        {
            self.request_button(button);
        }
    }
    fn prepare_frame(&mut self) {
        if self.frame_prepared {
            return;
        }
        self.frame_prepared = true;
        //50d18 polls task gates before50e24 advances feedback clocks.
        self.advance_transition = !self.transition.is_open();
        let mut waiting = Vec::new();
        //100300 walks its captured task list from the end (newest first).
        for pending in std::mem::take(&mut self.pending).into_iter().rev() {
            if pending
                .button_waits
                .iter()
                .any(|&i| !self.button_flashes[i].is_idle())
                || pending
                    .cell_wait
                    .is_some_and(|i| !self.cell_flashes[i].is_idle())
            {
                waiting.push(pending);
                continue;
            }
            match pending.action {
                PendingAction::Page(page) => {
                    self.characters.page = page;
                    let palette = super::view::character_page_palette(page);
                    for (tweener, target) in self.page_colors.iter_mut().zip(palette) {
                        tweener.enqueue(target, Tween::linear(Ticks::from_f32(10.0)));
                    }
                    self.page_scroll.enqueue(
                        self.characters.page_scroll(),
                        Tween {
                            duration: Ticks::from_f32(10.0),
                            easing: Easing::SineOut,
                        },
                    );
                    self.details_changed();
                    self.hovered = None;
                }
                PendingAction::Edit(edit) => {
                    self.characters.apply_edit(edit);
                    if matches!(edit, CharacterEdit::Variant { .. }) {
                        self.sprite_revision += 1;
                    } else {
                        self.details_changed();
                    }
                }
            }
        }
        waiting.reverse();
        self.pending = waiting;
        if self.closing_requested && self.close_flash.is_idle() && self.transition.is_open() {
            self.transition.begin_close();
        }
    }
    pub fn update(&mut self, delta: Ticks) {
        self.prepare_frame();
        self.text_scroll.update(delta);
        //53a40 inserts a closing task; it first runs on the next group poll.
        if self.advance_transition {
            self.transition.update(delta);
        }
        self.page_scroll.update(delta);
        self.close_flash.update(delta);
        for flash in self
            .button_flashes
            .iter_mut()
            .chain(self.cell_flashes.iter_mut())
            .chain(self.selections.iter_mut())
            .chain(self.page_colors.iter_mut())
        {
            flash.update(delta);
        }
        self.frame_prepared = false;
    }
}

pub struct CharacterScreen {
    pub model: CharacterScreenModel,
    atlas: Arc<CharacterTextures>,
    system: Arc<CharacterTextures>,
    text: Option<CharacterText>,
    full_sprite: Option<Arc<CharacterTextures>>,
    pending_full_sprite: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<CharacterTextures>>>>,
    text_revision: u64,
    sprite_revision: u64,
    pointer: CharacterPointer,
}

#[cfg(test)]
pub(crate) fn assert_reference_character_scroll<'a>(
    server: &'a Arc<AssetServer>,
    scenario: &Scenario,
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    target: &wgpu::Texture,
) {
    use crate::app::AppAction as A;
    use shin_input::{Action, ActionState, RawInputState};
    use winit::keyboard::KeyCode;
    let mut screen = CharacterScreen::new(server, scenario, 1, 0).unwrap();
    let (cell, definition, state) = screen
        .model
        .characters
        .cells
        .iter()
        .enumerate()
        .find_map(|(cell, c)| {
            c.definitions
                .iter()
                .enumerate()
                .find_map(|(definition, d)| {
                    d.states
                        .iter()
                        .position(|s| s.description.contains("気に入られた不遇の青年"))
                        .map(|state| (cell, definition, state))
                })
        })
        .expect("Episode2 must contain the reported Battler description");
    let reported = (cell, definition, state);
    screen.model.characters.page = screen.model.characters.cells[cell].page;
    screen.model.characters.select(cell);
    screen
        .model
        .characters
        .apply_edit(CharacterEdit::Definition {
            cell,
            target: definition,
        });
    screen.model.characters.apply_edit(CharacterEdit::State {
        cell,
        definition,
        target: state,
    });
    screen.model.details_changed();
    screen.update(Ticks::ZERO, server);
    let height = screen.text.as_ref().unwrap().layout.height;
    assert!(
        height > 730.0,
        "Reported description must naturally overflow, height={height}"
    );
    fn key(screen: &mut CharacterScreen, code: KeyCode, repeat: bool) {
        let mut raw = RawInputState::new();
        raw.keyboard.insert(code);
        let lowered = A::lower(&raw);
        let input = enum_map::EnumMap::from_fn(|action| {
            if lowered[action] {
                ActionState {
                    is_held: true,
                    is_clicked: !repeat,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                }
            } else {
                ActionState::default()
            }
        });
        screen
            .model
            .handle_controller_input(&input, crate::app::text_scroll_axis(&raw));
    }
    key(&mut screen, KeyCode::KeyL, false);
    assert_eq!(
        screen.model.text_scroll.position, 0.0,
        "Opening remains input gated"
    );
    screen.update(Ticks::from_f32(10.0), server);
    screen.update(Ticks::ZERO, server);
    screen.wait_reference_full_sprite(server);
    let page = screen.model.characters.page;
    let selected = screen.model.characters.selected_index();
    let description = screen
        .model
        .characters
        .selected_cell()
        .unwrap()
        .definition()
        .state()
        .unwrap()
        .description
        .clone();
    // Integer positions and ceil content range, recovered from both supplied
    // executables. Scroller drawing follows the text's stencil release.
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let root = std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    let exe = std::fs::read(root.with_file_name(if updated {
        "uncompressed_executable_update"
    } else {
        "uncompressed_executable"
    }))
    .unwrap();
    let word =
        |address: usize| u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
    let shift = if updated { 0xa0 } else { 0 };
    for (address, opcode) in [
        (0x50de8, 0x52805b48),
        (0x50df4, 0x1e280008),
        (0x52048, 0x4eaa1d40),
        (0x5204c, 0x4ea81d01),
        (0x5206c, 0x9107c260),
    ] {
        assert_eq!(
            word(address - shift),
            opcode,
            "Native character scrollbar anchor{address:x}"
        );
    }
    let constants = if updated {
        [0x15a6bc, 0x15a364, 0x15a5f8]
    } else {
        [0x1596dc, 0x159384, 0x159618]
    };
    for (address, value) in constants.into_iter().zip([1520.0f32, 188.0, 730.0]) {
        assert_eq!(word(address), value.to_bits());
    }
    fn draw(
        screen: &CharacterScreen,
        context: &mut crate::render::PreRenderContext,
        target: &wgpu::Texture,
    ) -> Vec<u8> {
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let view = target.create_view(&Default::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Reported Battler description scrollbar",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            screen.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(context, target)
    }
    let top = draw(&screen, context, target);
    // Negative render control omits only the range-driven scrollbar. Actual
    // authored height remains in use for all input/geometry checks.
    screen.text.as_mut().unwrap().layout.height = 730.0;
    let without = draw(&screen, context, target);
    screen.text.as_mut().unwrap().layout.height = height;
    let changed = (188..918)
        .flat_map(|y| (1520..1536).map(move |x| (y * 1920 + x) * 4))
        .filter(|&at| top[at..at + 4] != without[at..at + 4])
        .count();
    assert!(
        changed > 100,
        "Character scrollbar must visibly render: {changed}"
    );
    key(&mut screen, KeyCode::KeyL, false);
    assert_eq!(
        screen.model.text_scroll.position,
        16.0_f32.min(height - 730.0)
    );
    key(&mut screen, KeyCode::KeyL, true);
    for _ in 0..1000 {
        key(&mut screen, KeyCode::KeyL, true);
    }
    assert_eq!(screen.model.text_scroll.position, height - 730.0);
    assert_eq!(
        screen.model.characters.page, page,
        "K/L cannot switch pages"
    );
    assert_eq!(screen.model.characters.selected_index(), selected);
    assert_eq!(
        screen
            .model
            .characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .description,
        description
    );
    let bottom = draw(&screen, context, target);
    let moved = (188..918)
        .flat_map(|y| (1520..1536).map(move |x| (y * 1920 + x) * 4))
        .filter(|&at| top[at..at + 4] != bottom[at..at + 4])
        .count();
    assert!(moved > 30, "Scrollbar thumb must move with text: {moved}");
    for _ in 0..1000 {
        key(&mut screen, KeyCode::KeyK, true);
    }
    assert_eq!(screen.model.text_scroll.position, 0.0);
    // A short authored description still clamps scrolling and hides the bar.
    let font: Arc<crate::asset::font::GpuFontLazy> = server.load_sync("/system.fnt").unwrap();
    let short = screen
        .model
        .characters
        .cells
        .iter()
        .enumerate()
        .find_map(|(row, c)| {
            (c.page == page && c.is_selectable())
                .then(|| {
                    c.definitions
                        .iter()
                        .enumerate()
                        .find_map(|(definition, d)| {
                            d.states.iter().enumerate().find_map(|(state, s)| {
                                (super::text::CharacterTextLayout::new(
                                    &s.name,
                                    &s.description,
                                    font.as_ref(),
                                )
                                .height
                                    <= 730.0)
                                    .then_some((row, definition, state))
                            })
                        })
                })
                .flatten()
        })
        .unwrap();
    let (row, definition, state) = short;
    screen.model.characters.select(row);
    screen
        .model
        .characters
        .apply_edit(CharacterEdit::Definition {
            cell: row,
            target: definition,
        });
    screen.model.characters.apply_edit(CharacterEdit::State {
        cell: row,
        definition,
        target: state,
    });
    screen.model.details_changed();
    screen.update(Ticks::ZERO, server);
    key(&mut screen, KeyCode::KeyL, false);
    assert_eq!(screen.model.text_scroll.position, 0.0);
    assert!(
        crate::adv::tips::view::scrollbar_slices(screen.text.as_ref().unwrap().layout.height, 0.0)
            .is_empty()
    );
    // Start from a supplied grid's first page rather than Battler's last page.
    let tables = scenario.info_tables();
    let (grid, initial_page, next_page) = tables
        .chars_grid_info
        .iter()
        .enumerate()
        .find_map(|(grid, info)| {
            let model = CharacterModel::new(info, &tables.chars_sprite_info, grid, 0);
            model.adjacent_page(1).map(|next| (grid, model.page, next))
        })
        .expect("Supplied scenario must contain a multi-page character grid");
    let mut pages =
        CharacterScreen::new(server, scenario, grid as i32, initial_page as i32).unwrap();
    pages.update(Ticks::from_f32(10.0), server);
    pages.update(Ticks::ZERO, server);
    key(&mut pages, KeyCode::KeyJ, false);
    for _ in 0..30 {
        pages.update(Ticks::from_f32(1.0), server);
    }
    assert_eq!(
        pages.model.characters.page, next_page,
        "J switches character pages"
    );
    key(&mut pages, KeyCode::KeyH, false);
    for _ in 0..30 {
        pages.update(Ticks::from_f32(1.0), server);
    }
    assert_eq!(pages.model.characters.page, initial_page, "H switches back");
    assert_eq!(pages.model.text_scroll.position, 0.0);
    eprintln!(
        "Episode2 Battler {reported:?}, height{height}: K/L scroll/repeat/bounds, H/J supplied grid{grid} pages{initial_page}/{next_page}, short text; visible bar{changed}, moved thumb{moved}"
    );
}
impl CharacterScreen {
    pub fn new(
        server: &Arc<AssetServer>,
        scenario: &Scenario,
        grid: i32,
        initial_page: i32,
    ) -> anyhow::Result<Self> {
        Self::with_atlas(
            server,
            scenario,
            grid,
            initial_page,
            server.load_sync("/chars.txa")?,
        )
    }
    pub(crate) fn with_atlas(
        server: &Arc<AssetServer>,
        scenario: &Scenario,
        grid: i32,
        initial_page: i32,
        atlas: Arc<CharacterTextures>,
    ) -> anyhow::Result<Self> {
        let tables = scenario.info_tables();
        let id = usize::try_from(grid)?;
        let grid = tables
            .chars_grid_info
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Character grid {id} is unavailable"))?;
        let characters = CharacterModel::new(grid, &tables.chars_sprite_info, id, initial_page);
        atlas.validate_character_screen(&characters)?;
        let system: Arc<CharacterTextures> = server.load_sync("/systex.txa")?;
        system.require_names("/systex.txa", &["scroll"])?;
        // Root ADV/title assets retain this font during later text changes.
        // Keep construction fallible even when called without that root owner.
        let _font: Arc<crate::asset::font::GpuFontLazy> = server.load_sync("/system.fnt")?;
        let mut screen = Self {
            model: CharacterScreenModel::new(characters),
            atlas,
            system,
            text: None,
            full_sprite: None,
            pending_full_sprite: None,
            text_revision: u64::MAX,
            sprite_revision: u64::MAX,
            pointer: Default::default(),
        };
        screen.refresh(server);
        Ok(screen)
    }
    fn refresh(&mut self, server: &Arc<AssetServer>) {
        let state = self
            .model
            .characters
            .selected_cell()
            .and_then(|c| c.definition().state());
        if self.text_revision != self.model.text_revision {
            self.text = state.map(|s| CharacterText::load(server, s));
            self.model.text_scroll.set_maximum(
                self.text
                    .as_ref()
                    .map_or(0.0, |text| text.layout.max_scroll()),
            );
            self.text_revision = self.model.text_revision;
        }
        if self.sprite_revision != self.model.sprite_revision {
            let path = state
                .and_then(|s| s.variant())
                .filter(|v| !v.full_sprite.is_empty())
                .map(|v| format!("/chars/{}.txa", v.full_sprite.to_ascii_lowercase()));
            // Native0x52e64 replaces the pending request;0x50ec0..0x50f98
            // retains the displayed sprite until the latest request is ready.
            self.pending_full_sprite = path.map(|path| {
                let server = server.clone();
                shin_tasks::async_io::spawn(async move {
                    let atlas: Arc<CharacterTextures> = server.load(&path).await?;
                    atlas.require_first_texture(&path)?;
                    Ok(atlas)
                })
            });
            if self.pending_full_sprite.is_none() {
                self.full_sprite = None;
            }
            self.sprite_revision = self.model.sprite_revision;
        }
    }
    fn poll_full_sprite(&mut self) {
        if let Some(result) = self
            .pending_full_sprite
            .as_mut()
            .and_then(|task| task.poll_naive())
        {
            self.pending_full_sprite = None;
            match result {
                Ok(sprite) => self.full_sprite = Some(sprite),
                Err(error) => tracing::warn!(%error, "Loading character full sprite failed"),
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn wait_reference_full_sprite(&mut self, server: &Arc<AssetServer>) {
        if let Some(task) = self.pending_full_sprite.take() {
            self.full_sprite = Some(shin_tasks::block_on(task).unwrap());
        }
        let variant = self
            .model
            .characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .variant()
            .unwrap();
        let expected: Arc<CharacterTextures> = server
            .load_sync(format!(
                "/chars/{}.txa",
                variant.full_sprite.to_ascii_lowercase()
            ))
            .unwrap();
        assert!(
            Arc::ptr_eq(self.full_sprite.as_ref().unwrap(), &expected),
            "Displayed sprite differs from latest selection"
        );
    }
    pub fn update(&mut self, delta: Ticks, server: &Arc<AssetServer>) {
        self.model.update(delta);
        self.refresh(server);
        self.poll_full_sprite();
    }
    pub fn render(&self, pass: &mut RenderPass) {
        let transform = crate::render::top_left_projection_matrix();
        let progress = self.model.transition.progress();
        self.atlas.render_background(pass, progress, transform);
        let body = CharacterTextures::body_transform(progress, transform);
        self.atlas.render_window(pass, body);
        let (palette, selected, clicked) = self.model.grid_effects();
        self.atlas.render_grid_with_effects(
            pass,
            &self.model.characters,
            self.model.page_scroll.value(),
            palette,
            &selected,
            &clicked,
            body,
        );
        self.atlas.render_controls(
            pass,
            &self.model.characters,
            std::array::from_fn(|i| self.model.button_flashes[i].value()),
            body,
        );
        if let Some(text) = &self.text {
            text.render_divider(pass, &self.atlas, self.model.text_scroll.position, body);
        }
        if let Some(sprite) = &self.full_sprite {
            sprite.render_full_sprite(pass, body);
        }
        if let Some(text) = &self.text {
            text.render(pass, self.model.text_scroll.position, body);
            // Native52048..52074 translates the common730px scroller to
            // (1520,188), immediately after the text stencil is released.
            for (rect, uv) in crate::adv::tips::view::scrollbar_slices(
                text.layout.height,
                self.model.text_scroll.position,
            ) {
                let rect = rect - glam::vec4(318.0, 0.0, 318.0, 0.0);
                shin_render::quad_vertices::QuadVertices::new()
                    .with_box(rect.x, rect.y, rect.z, rect.w)
                    .with_tex_box(uv.x, uv.y, uv.z, uv.w)
                    .with_color(shin_core::primitives::color::UnormColor::WHITE)
                    .render_sprite(
                        pass,
                        shin_render::RenderRequestBuilder::new()
                            .color_blend_type(shin_render::ColorBlendType::Layer1),
                        self.system.get("scroll").as_source(),
                        body,
                    );
            }
        }
        self.atlas.render_header_with_flash(
            pass,
            progress,
            self.model.close_flash.value(),
            transform,
        );
        self.atlas.render_footer(
            pass,
            &self.model.characters,
            self.model.text_scroll.maximum > 0.0,
            progress,
            transform,
        );
    }
    fn pointer_target(&self, position: Vec2) -> PointerTarget {
        if (1764.0..1848.0).contains(&position.x) && (32.0..116.0).contains(&position.y) {
            return PointerTarget::Close;
        }
        if let Some(cell) = self.model.characters.cell_at(
            position,
            self.model.page_scroll.value(),
            !self.model.page_scroll.is_idle(),
        ) {
            return PointerTarget::Cell(cell);
        }
        if let Some(button) = self
            .model
            .characters
            .button_at(position, !self.model.page_scroll.is_idle())
        {
            return PointerTarget::Button(button);
        }
        if (544.0..1520.0).contains(&position.x) && (188.0..918.0).contains(&position.y) {
            return PointerTarget::Text;
        }
        PointerTarget::None
    }
    fn pointer_flash(&mut self, target: PointerTarget, value: f32) {
        match target {
            PointerTarget::Close => move_flash(&mut self.model.close_flash, value),
            PointerTarget::Button(button) => {
                move_flash(&mut self.model.button_flashes[button as usize], value)
            }
            PointerTarget::Cell(cell) => move_flash(&mut self.model.cell_flashes[cell], value),
            _ => {}
        }
    }
    pub fn consumes_keyboard(&self) -> bool {
        self.pointer.consumed
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed_seconds: f64) {
        self.model.prepare_frame();
        self.pointer.consumed = held || self.pointer.held;
        if !self.model.accepts_input() {
            self.pointer.held = held;
            self.pointer.target = None;
            return;
        }
        self.model.pointer(position, false);
        if held && !self.pointer.held {
            let target = self.pointer_target(position);
            self.pointer.origin = position;
            self.pointer.previous = position;
            self.pointer.previous_delta = Vec2::ZERO;
            self.pointer.dragging = false;
            self.pointer.target = Some(target);
            self.pointer_flash(target, 0.7);
            if target == PointerTarget::Text {
                self.model.text_scroll.release(0.0);
            }
        } else if held {
            // Native pointer coordinates1280x720 scale by1.5 into this1920x1080 canvas.
            if !self.pointer.dragging && (position - self.pointer.origin).length() > 16.0 {
                self.pointer.dragging = true;
                if let Some(target) = self.pointer.target {
                    self.pointer_flash(target, 0.0);
                }
            }
            let delta = position - self.pointer.previous;
            if self.pointer.dragging && delta.length() > 1.5 {
                if self.pointer.target == Some(PointerTarget::Text) {
                    self.drag_text(delta.y);
                }
                self.pointer.previous = position;
            }
            // The native release callback uses the last displacement and the time
            // since that sample; stationary held frames intentionally zero velocity.
            self.pointer.previous_delta = delta;
        } else if self.pointer.held {
            if self.pointer.dragging {
                if elapsed_seconds > 0.0 && self.pointer.target == Some(PointerTarget::Text) {
                    let velocity = self.pointer.previous_delta
                        * (0.016683333333333335 / elapsed_seconds) as f32;
                    if velocity.length() > 20.0 {
                        self.release_text_drag(velocity.y);
                    }
                }
            } else if self.pointer.target.is_some() {
                // Native click callbacks retain the press hit. Motion exceeding
                // the drag threshold cancels clicking; lifting alone does not re-hit-test.
                self.model.pointer(self.pointer.origin, true);
            }
            self.pointer.target = None;
        }
        self.pointer.held = held;
    }
    pub fn drag_text(&mut self, delta_y: f32) {
        if self.model.accepts_input() {
            self.model.text_scroll.drag(delta_y);
        }
    }
    pub fn release_text_drag(&mut self, velocity_y: f32) {
        if self.model.accepts_input() {
            self.model.text_scroll.release(velocity_y);
        }
    }
    pub fn scroll_text(&mut self, amount: f32) {
        if self.model.accepts_input() {
            self.model.text_scroll.axis(amount / 16.0);
        }
    }
}

#[cfg(test)]
mod tests;
