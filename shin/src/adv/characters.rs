//! Character-screen data and selection, recovered from native0x4e260..0x4fee8.
//! A cell may contain several characters. Script grids expose their authored
//! defaults; only the eight episode-menu grids expose state/variant controls.
pub mod screen;
pub mod text;
pub mod view;
use shin_core::format::scenario::info::{
    CharsGridConnectorColor as Color, CharsGridConnectorShape as Shape, CharsGridInfoItem,
    CharsGridSegment, CharsPortraitDisplayMode, CharsSpriteInfo, CharsSpriteSegment,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(usize)]
pub enum CharacterButton {
    PreviousPage,
    NextPage,
    Execute,
    Resurrect,
    Variant,
}
impl CharacterButton {
    pub const ALL: [Self; 5] = [
        Self::PreviousPage,
        Self::NextPage,
        Self::Execute,
        Self::Resurrect,
        Self::Variant,
    ];
    pub fn rectangle(self) -> glam::Vec4 {
        match self {
            Self::PreviousPage => glam::vec4(90.0, 724.0, 274.0, 780.0),
            Self::NextPage => glam::vec4(306.0, 724.0, 490.0, 780.0),
            Self::Execute => glam::vec4(102.0, 780.0, 350.0, 844.0),
            Self::Resurrect => glam::vec4(102.0, 844.0, 350.0, 908.0),
            Self::Variant => glam::vec4(354.0, 780.0, 478.0, 908.0),
        }
    }
    pub fn texture(self) -> &'static str {
        match self {
            Self::PreviousPage | Self::NextPage => "lr",
            _ => "button",
        }
    }
    pub fn uv(self) -> glam::Vec4 {
        match self {
            Self::PreviousPage => glam::vec4(0.0, 0.0, 0.5, 1.0),
            Self::NextPage => glam::vec4(0.5, 0.0, 1.0, 1.0),
            Self::Execute => glam::vec4(0.0, 0.0, 2.0 / 3.0, 0.5),
            Self::Resurrect => glam::vec4(0.0, 0.5, 2.0 / 3.0, 1.0),
            Self::Variant => glam::vec4(2.0 / 3.0, 0.0, 1.0, 1.0),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CharacterButtonState {
    Hidden,
    Disabled,
    Enabled,
}

/// Captured native edit target; selection may move while its flash is running.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CharacterEdit {
    Definition {
        cell: usize,
        target: usize,
    },
    State {
        cell: usize,
        definition: usize,
        target: usize,
    },
    Variant {
        cell: usize,
        definition: usize,
        target: usize,
    },
}

#[derive(Debug)]
pub struct CharacterVariant {
    pub index: u8,
    pub display_mode: u8,
    pub portrait: String,
    pub full_sprite: String,
}

#[derive(Debug)]
pub struct CharacterState {
    pub index: u8,
    pub name: String,
    pub description: String,
    pub variants: Vec<CharacterVariant>,
    selected_variant: usize,
}
impl CharacterState {
    pub fn variant(&self) -> Option<&CharacterVariant> {
        self.variants.get(self.selected_variant)
    }
}

#[derive(Debug)]
pub struct CharacterDefinition {
    pub character_id: usize,
    pub default_state: u8,
    pub default_variant: u8,
    pub states: Vec<CharacterState>,
    selected_state: usize,
}
impl CharacterDefinition {
    pub fn state(&self) -> Option<&CharacterState> {
        self.states.get(self.selected_state)
    }
}

#[derive(Debug)]
pub struct CharacterCell {
    pub page: u8,
    pub x: u8,
    pub y: u8,
    pub definitions: Vec<CharacterDefinition>,
    selected_definition: usize,
}
impl CharacterCell {
    pub fn definition_index(&self) -> usize {
        self.selected_definition
    }
    pub fn definition(&self) -> &CharacterDefinition {
        &self.definitions[self.selected_definition]
    }

    pub fn is_selectable(&self) -> bool {
        // Native0x54070 checks the original default of the active definition.
        self.definition().default_state != 0
    }
}

#[derive(Debug)]
pub struct CharacterConnector {
    pub page: u8,
    pub x: u8,
    pub y: u8,
    pub shape: u8,
    pub color: u8,
}

#[derive(Debug)]
pub struct CharacterModel {
    pub cells: Vec<CharacterCell>,
    pub connectors: Vec<CharacterConnector>,
    pub page: u8,
    pub page_mask: u8,
    pub editable: bool,
    selections: [Option<usize>; 4],
}
impl CharacterModel {
    pub(crate) fn plan_character_cycle(&self) -> Option<CharacterEdit> {
        let cell = self.selected_index()?;
        let c = &self.cells[cell];
        if c.definitions.len() < 2 {
            return None;
        }
        Some(CharacterEdit::Definition {
            cell,
            target: (c.selected_definition + 1) % c.definitions.len(),
        })
    }
    pub(crate) fn plan_state_change(&self, direction: i32) -> Option<CharacterEdit> {
        if !self.editable {
            return None;
        }
        let cell = self.selected_index()?;
        let c = &self.cells[cell];
        let definition = c.selected_definition;
        let d = &c.definitions[definition];
        if d.states.is_empty() {
            return None;
        }
        let target =
            (d.selected_state as i64 + direction as i64).rem_euclid(d.states.len() as i64) as usize;
        (target != d.selected_state).then_some(CharacterEdit::State {
            cell,
            definition,
            target,
        })
    }
    pub(crate) fn plan_variant_cycle(&self) -> Option<CharacterEdit> {
        if !self.editable {
            return None;
        }
        let cell = self.selected_index()?;
        let c = &self.cells[cell];
        let definition = c.selected_definition;
        let d = &c.definitions[definition];
        let state = d.selected_state;
        let v = d.states.get(state)?;
        if v.variants.len() < 2 {
            return None;
        }
        Some(CharacterEdit::Variant {
            cell,
            definition,
            target: (v.selected_variant + 1) % v.variants.len(),
        })
    }
    pub(crate) fn apply_edit(&mut self, edit: CharacterEdit) {
        match edit {
            CharacterEdit::Definition { cell, target } => {
                self.cells[cell].selected_definition = target
            }
            CharacterEdit::State {
                cell,
                definition,
                target,
            } => self.cells[cell].definitions[definition].selected_state = target,
            CharacterEdit::Variant {
                cell,
                definition,
                target,
            } => {
                // Native0x55c14 reads the definition's state at completion, after waiting.
                let d = &mut self.cells[cell].definitions[definition];
                d.states[d.selected_state].selected_variant = target;
            }
        }
    }

    /// Native0x50350: hide absent page arrows/script controls, dim state endpoints.
    pub fn button_states(&self) -> [CharacterButtonState; 5] {
        use CharacterButtonState::*;
        let mut states = [Disabled; 5];
        if self.page_mask.count_ones() < 2 {
            states[0] = Hidden;
            states[1] = Hidden;
        } else {
            states[0] = if self.adjacent_page(-1).is_some() {
                Enabled
            } else {
                Disabled
            };
            states[1] = if self.adjacent_page(1).is_some() {
                Enabled
            } else {
                Disabled
            };
        }
        if !self.editable {
            states[2..].fill(Hidden);
            return states;
        }
        if let Some(cell) = self.selected_cell() {
            let definition = cell.definition();
            states[2] = if definition.selected_state + 1 < definition.states.len() {
                Enabled
            } else {
                Disabled
            };
            states[3] = if definition.selected_state > 0 {
                Enabled
            } else {
                Disabled
            };
            states[4] = if definition.state().is_some_and(|s| s.variants.len() > 1) {
                Enabled
            } else {
                Disabled
            };
        }
        states
    }

    pub fn button_at(&self, position: glam::Vec2, transitioning: bool) -> Option<CharacterButton> {
        if !(86.0..494.0).contains(&position.x) || !(188.0..918.0).contains(&position.y) {
            return None;
        }
        let states = self.button_states();
        CharacterButton::ALL.into_iter().find(|button| {
            // Native0x54198 ignores page arrows while scroll tweener is busy.
            if transitioning && (*button as usize) < 2 {
                return false;
            }
            let r = button.rectangle();
            states[*button as usize] == CharacterButtonState::Enabled
                && position.x >= r.x
                && position.x < r.z
                && position.y >= r.y
                && position.y < r.w
        })
    }

    pub fn activate_button(&mut self, button: CharacterButton) -> bool {
        if self.button_states()[button as usize] != CharacterButtonState::Enabled {
            return false;
        }
        match button {
            CharacterButton::PreviousPage => self.change_page(-1),
            CharacterButton::NextPage => self.change_page(1),
            CharacterButton::Execute => self.change_state(1),
            CharacterButton::Resurrect => self.change_state(-1),
            CharacterButton::Variant => self.cycle_variant(),
        }
    }

    /// Native0x53cbc..0x53e00: choose distance/(direction dot)^4, in authored lattice units.
    pub fn navigate(&mut self, direction: glam::Vec2) -> bool {
        let Some(current) = self.selected_cell() else {
            return false;
        };
        if direction.length_squared() == 0.0 {
            return false;
        }
        let direction = direction.normalize();
        let origin = glam::vec2(f32::from(current.x), f32::from(current.y));
        let mut best = f32::MAX;
        let mut target = None;
        for (index, cell) in self.cells.iter().enumerate() {
            if Some(index) == self.selected_index()
                || cell.page != self.page
                || !cell.is_selectable()
            {
                continue;
            }
            let delta = glam::vec2(f32::from(cell.x), f32::from(cell.y)) - origin;
            let distance = delta.length();
            if distance == 0.0 {
                continue;
            }
            let dot = direction.dot(delta / distance);
            if dot <= f32::EPSILON {
                continue;
            }
            let score = distance / dot.powf(4.0);
            if score < best {
                best = score;
                target = Some(index);
            }
        }
        target.is_some_and(|index| self.select(index))
    }

    pub fn new(
        grid: &CharsGridInfoItem,
        sprites: &CharsSpriteInfo,
        grid_id: usize,
        initial_page: i32,
    ) -> Self {
        let editable = grid_id < 8;
        let mut cells: Vec<CharacterCell> = Vec::new();
        let mut connectors = Vec::new();
        for segment in &grid.segments {
            match segment {
                CharsGridSegment::Portrait {
                    page,
                    grid_x,
                    grid_y,
                    character_id,
                    default_state,
                    default_variant,
                } => {
                    let definition = build_definition(
                        sprites,
                        character_id.into_i32() as usize,
                        *default_state,
                        *default_variant,
                        editable,
                    );
                    if let Some(cell) = cells
                        .iter_mut()
                        .find(|c| (c.page, c.x, c.y) == (*page, *grid_x, *grid_y))
                    {
                        cell.definitions.push(definition);
                    } else {
                        cells.push(CharacterCell {
                            page: *page,
                            x: *grid_x,
                            y: *grid_y,
                            definitions: vec![definition],
                            selected_definition: 0,
                        });
                    }
                }
                CharsGridSegment::Connector {
                    page,
                    grid_x,
                    grid_y,
                    shape,
                    color,
                } => {
                    // BinRead enums are not Copy; discriminants are their on-disk values.
                    let shape = match shape {
                        Shape::None => 0,
                        Shape::LeftDeadEnd => 1,
                        Shape::TopDeadEnd => 2,
                        Shape::LeftTopElbow => 3,
                        Shape::RightDeadEnd => 4,
                        Shape::HorizontalLine => 5,
                        Shape::TopRightElbow => 6,
                        Shape::TopPointingT => 7,
                        Shape::BottomDeadEnd => 8,
                        Shape::BottomLeftElbow => 9,
                        Shape::VerticalLine => 10,
                        Shape::LeftPointingT => 11,
                        Shape::RightBottomElbow => 12,
                        Shape::BottomPointingT => 13,
                        Shape::RightPointingT => 14,
                        Shape::Cross => 15,
                        Shape::DarkVerticalLine => 16,
                    };
                    let color = match color {
                        Color::Red => 1,
                        Color::Blue => 2,
                        Color::Yellow => 3,
                    };
                    connectors.push(CharacterConnector {
                        page: *page,
                        x: *grid_x,
                        y: *grid_y,
                        shape,
                        color,
                    });
                }
            }
        }
        let mut page_mask = 0;
        let mut selections = [None; 4];
        for (index, cell) in cells.iter().enumerate() {
            assert!(
                cell.page < 4,
                "Character grid page exceeds native four-page table"
            );
            page_mask |= 1 << cell.page;
            if selections[cell.page as usize].is_none() && cell.is_selectable() {
                selections[cell.page as usize] = Some(index);
            }
        }
        // Native0x4edb0 masks even negative arguments before checking pages.
        let requested = initial_page as u8 & 3;
        let page = (0..4)
            .map(|i| requested.wrapping_add(i) & 3)
            .find(|p| page_mask & (1 << p) != 0)
            .unwrap_or(0);
        Self {
            cells,
            connectors,
            page,
            page_mask,
            editable,
            selections,
        }
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selections[self.page as usize]
    }

    pub fn selection_on_page(&self, page: u8) -> Option<usize> {
        self.selections[page as usize]
    }

    pub fn selected_cell(&self) -> Option<&CharacterCell> {
        self.selected_index().map(|i| &self.cells[i])
    }

    /// Pages occupy consecutive400-pixel slots, including gaps in page IDs.
    pub fn page_scroll(&self) -> f32 {
        (self.page_mask & ((1 << self.page) - 1)).count_ones() as f32 * 400.0
    }

    /// Native0x53f90: authored coordinates are on a24x26-pixel lattice.
    /// During a page transition the pointer cannot select a portrait.
    pub fn cell_at(&self, position: glam::Vec2, scroll: f32, transitioning: bool) -> Option<usize> {
        if transitioning
            || !(86.0..494.0).contains(&position.x)
            || !(188.0..918.0).contains(&position.y)
        {
            return None;
        }
        self.cells.iter().enumerate().find_map(|(index, cell)| {
            if cell.page != self.page || !cell.is_selectable() {
                return None;
            }
            let left = 84.0 - scroll + self.page_scroll() + f32::from(cell.x) * 24.0;
            let top = 184.0 + f32::from(cell.y) * 26.0;
            ((left..left + 76.0).contains(&position.x) && (top..top + 86.0).contains(&position.y))
                .then_some(index)
        })
    }

    pub fn select(&mut self, index: usize) -> bool {
        let Some(cell) = self.cells.get(index) else {
            return false;
        };
        if cell.page != self.page || !cell.is_selectable() || self.selected_index() == Some(index) {
            return false;
        }
        self.selections[self.page as usize] = Some(index);
        true
    }

    /// Page arrows stop at the end, skipping absent pages (native0x50350).
    pub fn adjacent_page(&self, direction: i32) -> Option<u8> {
        if direction == 0 {
            return None;
        }
        let mut page = self.page as i32 + direction.signum();
        while (0..4).contains(&page) {
            if self.page_mask & (1 << page) != 0 {
                return Some(page as u8);
            }
            page += direction.signum();
        }
        None
    }

    pub fn change_page(&mut self, direction: i32) -> bool {
        let Some(page) = self.adjacent_page(direction) else {
            return false;
        };
        self.page = page;
        true
    }

    pub fn cycle_character(&mut self) -> bool {
        let Some(index) = self.selected_index() else {
            return false;
        };
        let cell = &mut self.cells[index];
        if cell.definitions.len() < 2 {
            return false;
        }
        cell.selected_definition = (cell.selected_definition + 1) % cell.definitions.len();
        true
    }

    /// Execute/resurrect change the state; each state retains its own variant.
    pub fn change_state(&mut self, direction: i32) -> bool {
        if !self.editable {
            return false;
        }
        let Some(index) = self.selected_index() else {
            return false;
        };
        let cell = &mut self.cells[index];
        let def = &mut cell.definitions[cell.selected_definition];
        if def.states.is_empty() {
            return false;
        }
        let next = (def.selected_state as i64 + direction as i64)
            .rem_euclid(def.states.len() as i64) as usize;
        if next == def.selected_state {
            return false;
        }
        def.selected_state = next;
        true
    }

    pub fn cycle_variant(&mut self) -> bool {
        if !self.editable {
            return false;
        }
        let Some(index) = self.selected_index() else {
            return false;
        };
        let cell = &mut self.cells[index];
        let def = &mut cell.definitions[cell.selected_definition];
        let Some(state) = def.states.get_mut(def.selected_state) else {
            return false;
        };
        if state.variants.len() < 2 {
            return false;
        }
        state.selected_variant = (state.selected_variant + 1) % state.variants.len();
        true
    }
}

fn build_definition(
    sprites: &CharsSpriteInfo,
    character_id: usize,
    default_state: u8,
    default_variant: u8,
    editable: bool,
) -> CharacterDefinition {
    let mut states: Vec<CharacterState> = Vec::new();
    for segment in &sprites[character_id].segments {
        match segment {
            CharsSpriteSegment::BeginState { index } => states.push(CharacterState {
                index: *index,
                name: String::new(),
                description: String::new(),
                variants: Vec::new(),
                selected_variant: 0,
            }),
            CharsSpriteSegment::Texts { name, description } => {
                let state = states
                    .last_mut()
                    .expect("Character text precedes its state");
                state.name = name.as_str().into();
                state.description = description.as_str().into();
            }
            CharsSpriteSegment::SpriteVariant {
                variant_index,
                portrait_display_mode,
                portrait_texture_name,
                full_texture_name,
            } => {
                let mode = match portrait_display_mode {
                    CharsPortraitDisplayMode::Alive => 0,
                    CharsPortraitDisplayMode::Dead => 1,
                    CharsPortraitDisplayMode::Missing => 2,
                    CharsPortraitDisplayMode::HalfDead => 3,
                };
                states
                    .last_mut()
                    .expect("Character variant precedes its state")
                    .variants
                    .push(CharacterVariant {
                        index: *variant_index,
                        display_mode: mode,
                        portrait: portrait_texture_name.as_str().into(),
                        full_sprite: full_texture_name.as_str().into(),
                    });
            }
        }
    }
    if editable {
        // Native0x4f7dc..0x4fb8c enumerates state bits1..3 and variant bits0..3.
        states.retain(|s| (1..=3).contains(&s.index));
    } else {
        states.retain(|s| s.index == default_state);
    }
    states.sort_by_key(|s| s.index);
    for state in &mut states {
        state.variants.retain(|v| {
            if editable {
                v.index < 4
            } else {
                v.index == default_variant
            }
        });
        state.variants.sort_by_key(|v| v.index);
        state.selected_variant = state
            .variants
            .iter()
            .position(|v| v.index == default_variant)
            .unwrap_or(0);
    }
    let selected_state = states
        .iter()
        .position(|s| s.index == default_state)
        .unwrap_or(0);
    CharacterDefinition {
        character_id,
        default_state,
        default_variant,
        states,
        selected_state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::{
        scenario::{
            info::{CharsSpriteId, CharsSpriteInfoItem},
            instruction_elements::FromNumber,
        },
        text::U16String,
    };

    fn portrait(page: u8, x: u8, character: u16, state: u8, variant: u8) -> CharsGridSegment {
        CharsGridSegment::Portrait {
            page,
            grid_x: x,
            grid_y: 1,
            character_id: CharsSpriteId::from_number(i32::from(character)),
            default_state: state,
            default_variant: variant,
        }
    }

    fn sprites() -> CharsSpriteInfo {
        (0..2)
            .map(|character| {
                let mut segments = Vec::new();
                for state in 1..=4 {
                    segments.push(CharsSpriteSegment::BeginState { index: state });
                    segments.push(CharsSpriteSegment::Texts {
                        name: U16String::new(format!("Character{character}/{state}")),
                        description: U16String::new(format!("Description{state}")),
                    });
                    for variant in [0, 2] {
                        segments.push(CharsSpriteSegment::SpriteVariant {
                            variant_index: variant,
                            portrait_display_mode: CharsPortraitDisplayMode::Alive,
                            portrait_texture_name: U16String::new("portrait"),
                            full_texture_name: U16String::new("full"),
                        });
                    }
                }
                CharsSpriteInfoItem {
                    episode: 0,
                    segments,
                }
            })
            .collect()
    }

    #[test]
    fn buttons_hide_dim_and_stop_at_native_state_endpoints() {
        use CharacterButton::*;
        use CharacterButtonState::*;
        let grid = CharsGridInfoItem {
            segments: vec![portrait(0, 0, 0, 1, 0), portrait(3, 0, 1, 2, 0)],
        };
        let mut model = CharacterModel::new(&grid, &sprites(), 0, 0);
        assert_eq!(
            model.button_states(),
            [Disabled, Enabled, Enabled, Disabled, Enabled]
        );
        assert_eq!(
            model.button_at(glam::vec2(306.0, 724.0), false),
            Some(NextPage)
        );
        assert_eq!(model.button_at(glam::vec2(306.0, 724.0), true), None);
        assert_eq!(
            model.button_at(glam::vec2(102.0, 780.0), true),
            Some(Execute)
        );
        assert_eq!(model.button_at(glam::vec2(350.0, 780.0), false), None);
        assert!(!model.activate_button(Resurrect));
        assert!(model.activate_button(Execute));
        assert!(model.activate_button(Execute));
        assert_eq!(model.button_states()[2], Disabled);
        assert!(!model.activate_button(Execute));
        assert_eq!(
            model
                .selected_cell()
                .unwrap()
                .definition()
                .state()
                .unwrap()
                .index,
            3
        );
        assert!(model.activate_button(NextPage));
        assert_eq!(model.page, 3);
        assert_eq!(model.button_states()[..2], [Enabled, Disabled]);
        let scripted = CharacterModel::new(&grid, &sprites(), 8, 0);
        assert_eq!(scripted.button_states()[2..], [Hidden; 3]);
        let single = CharsGridInfoItem {
            segments: vec![portrait(1, 0, 0, 1, 0)],
        };
        assert_eq!(
            CharacterModel::new(&single, &sprites(), 0, 0).button_states()[..2],
            [Hidden; 2]
        );
    }

    #[test]
    fn directional_selection_prefers_native_angle_score_and_retains_boundaries() {
        let mut near = portrait(0, 1, 1, 1, 0);
        if let CharsGridSegment::Portrait { grid_y, .. } = &mut near {
            *grid_y = 3;
        }
        let grid = CharsGridInfoItem {
            segments: vec![
                portrait(0, 0, 0, 1, 0),
                near,
                portrait(0, 8, 1, 1, 0),
                portrait(0, 3, 0, 0, 0),
                portrait(3, 2, 0, 1, 0),
            ],
        };
        let mut model = CharacterModel::new(&grid, &sprites(), 8, 0);
        assert!(!model.navigate(glam::Vec2::ZERO));
        assert!(!model.navigate(glam::vec2(-1.0, 0.0)));
        assert!(model.navigate(glam::vec2(1.0, 0.0)));
        assert_eq!(model.selected_index(), Some(2));
        assert!(!model.navigate(glam::vec2(1.0, 0.0)));
        assert!(model.navigate(glam::vec2(-1.0, 0.0)));
        assert_eq!(model.selected_index(), Some(0));
        assert!(model.navigate(glam::vec2(0.0, 1.0)));
        assert_eq!(model.selected_index(), Some(1));
    }

    #[test]
    fn groups_stacked_characters_and_retains_page_selections() {
        let grid = CharsGridInfoItem {
            segments: vec![
                portrait(1, 1, 0, 0, 0),
                portrait(1, 5, 0, 1, 0),
                portrait(3, 1, 0, 1, 0),
                portrait(1, 5, 1, 2, 2),
                portrait(1, 9, 0, 1, 0),
                CharsGridSegment::Connector {
                    page: 2,
                    grid_x: 0,
                    grid_y: 0,
                    shape: Shape::Cross,
                    color: Color::Yellow,
                },
            ],
        };
        let mut model = CharacterModel::new(&grid, &sprites(), 8, 2);
        assert_eq!(model.cells.len(), 4);
        assert_eq!(model.page_mask, 0b1010); // Connector-only page2 is absent.
        assert_eq!(model.connectors.len(), 1);
        assert_eq!(
            (
                model.connectors[0].page,
                model.connectors[0].shape,
                model.connectors[0].color
            ),
            (2, 15, 3)
        );
        assert_eq!(model.page, 3);
        assert_eq!(model.selected_index(), Some(2));
        assert_eq!(model.page_scroll(), 400.0);
        assert!(!model.change_page(1));
        assert!(model.change_page(-1));
        assert_eq!(model.page, 1);
        assert_eq!(model.selected_index(), Some(1));
        assert!(!model.select(0)); // Empty frame.
        assert!(!model.select(2)); // Different page.
        assert!(!model.select(100));
        assert!(model.cycle_character());
        assert_eq!(model.selected_cell().unwrap().definition().character_id, 1);
        assert_eq!(
            model
                .selected_cell()
                .unwrap()
                .definition()
                .state()
                .unwrap()
                .index,
            2
        );
        assert_eq!(
            model
                .selected_cell()
                .unwrap()
                .definition()
                .state()
                .unwrap()
                .variant()
                .unwrap()
                .index,
            2
        );
        assert!(!model.change_state(1)); // Script grids retain authored state/variant.
        assert!(!model.cycle_variant());
        assert!(model.select(3));
        assert!(model.change_page(1));
        assert!(model.change_page(-1));
        assert_eq!(model.selected_index(), Some(3));
        for initial in [0, 1, 2, 3, -1, -2, i32::MAX, i32::MIN] {
            let model = CharacterModel::new(&grid, &sprites(), 8, initial);
            assert_eq!(model.page, if initial & 3 <= 1 { 1 } else { 3 });
        }
    }

    #[test]
    fn episode_grids_expose_three_states_and_retain_each_variant() {
        let grid = CharsGridInfoItem {
            segments: vec![portrait(0, 1, 0, 2, 2)],
        };
        let mut model = CharacterModel::new(&grid, &sprites(), 0, 0);
        let current = |m: &CharacterModel| {
            let state = m.selected_cell().unwrap().definition().state().unwrap();
            (state.index, state.variant().unwrap().index)
        };
        assert_eq!(current(&model), (2, 2));
        assert!(model.cycle_variant());
        assert_eq!(current(&model), (2, 0));
        assert!(model.change_state(1));
        assert_eq!(current(&model), (3, 2));
        assert!(model.change_state(1));
        assert_eq!(current(&model), (1, 2));
        assert!(model.change_state(-2));
        assert_eq!(current(&model), (2, 0));
        assert!(!model.change_state(0));
        assert!(!model.cycle_character());
    }

    #[test]
    fn pointer_hit_testing_uses_native_lattice_clipping_and_scroll() {
        let grid = CharsGridInfoItem {
            segments: vec![portrait(1, 1, 0, 1, 0), portrait(3, 1, 1, 1, 0)],
        };
        let model = CharacterModel::new(&grid, &sprites(), 8, 3);
        assert_eq!(model.page_scroll(), 400.0);
        for (x, y, expected) in [
            (108.0, 210.0, Some(1)),
            (183.9, 295.9, Some(1)),
            (184.0, 210.0, None),
            (108.0, 296.0, None),
            (107.9, 210.0, None),
            (108.0, 209.9, None),
            (85.9, 210.0, None),
            (108.0, 187.9, None),
        ] {
            assert_eq!(model.cell_at(glam::vec2(x, y), 400.0, false), expected);
        }
        assert_eq!(model.cell_at(glam::vec2(108.0, 210.0), 400.0, true), None);
        assert_eq!(model.cell_at(glam::vec2(108.0, 210.0), 0.0, false), None);
        let empty = CharacterModel::new(
            &CharsGridInfoItem {
                segments: Vec::new(),
            },
            &sprites(),
            8,
            0,
        );
        assert_eq!(empty.page_mask, 0);
        assert_eq!(empty.selected_cell().map(|c| c.page), None);
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_character_model_matches_native_instructions_and_geometry() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap())
        };
        // Grouped-cell lookup, first nonempty selection, initial page masking,
        // script-grid control restriction, character/state/variant cycling and hit lattice.
        for (address, opcode) in [
            (0x4e61c, 0x54000121u32),
            (0x4e624, 0x394006eb),
            (0x4ec24, 0xf9405d0e),
            (0x4ec2c, 0x3400006e),
            (0x4edb8, 0x1200052a),
            (0x4edd4, 0x1200054a),
            (0x4ee00, 0x12000529),
            (0x4f764, 0x7100211f),
            (0x4fd60, 0xf9405f0b),
            (0x531b8, 0x71000aff),
            (0x53244, 0x6b17011f),
            (0x533f4, 0x1ac90eaa),
            (0x53644, 0x6b08015f),
            (0x54074, 0x394009ef),
            (0x54084, 0x531d71ef),
            (0x54090, 0x1b0c7def),
            // Native control visibility/endpoints and distance/dot^4 navigation.
            (0x50354, 0x7100091f),
            (0x50398, 0x71001d1f),
            (0x50434, 0x7100099f),
            (0x50444, 0x6b0d019f),
            (0x50458, 0x7100019f),
            (0x50494, 0x7100091f),
            (0x51d14, 0x370006a8),
            (0x51d74, 0x370800e8),
            (0x53d58, 0x39400929),
            (0x53d68, 0x6b09015f),
            (0x53d98, 0x1e2f09e0),
            (0x53da4, 0x1e21c009),
            (0x53dd0, 0x1e2e2000),
            (0x53dd8, 0x1e221001),
            (0x53de0, 0x1e201940),
            (0x53de4, 0x1e200920),
            (0x53de8, 0x1e2021a0),
        ] {
            assert_eq!(word(address), opcode, "native{address:x}");
        }
        for (address, value) in [
            (0x1594c8, 918.0f32),
            (0x159670, 86.0),
            (0x159998, 494.0),
            (0x159384, 188.0),
            (0x1599e0, 90.0),
            (0x1597b4, 198.0),
            (0x159904, 76.0),
        ] {
            assert_eq!(word(address), value.to_bits());
        }
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
    fn reference_all_character_grids_resolve_authored_states_and_assets() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
        };
        use std::{
            collections::{HashMap, HashSet},
            fs::File,
            io::Read,
        };
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let tables = scenario.info_tables();
        let mut assets = HashSet::new();
        let mut portrait_assets = HashSet::new();
        let mut stacked = 0;
        let mut portraits = 0;
        for (id, grid) in tables.chars_grid_info.iter().enumerate() {
            let model = CharacterModel::new(grid, &tables.chars_sprite_info, id, 0);
            let mut expected = HashMap::<(u8, u8, u8), Vec<(usize, u8, u8)>>::new();
            let mut first_selections = [None; 4];
            let mut cell_order = Vec::new();
            for segment in &grid.segments {
                if let CharsGridSegment::Portrait {
                    page,
                    grid_x,
                    grid_y,
                    character_id,
                    default_state,
                    default_variant,
                } = segment
                {
                    let key = (*page, *grid_x, *grid_y);
                    if !expected.contains_key(&key) {
                        cell_order.push(key);
                    }
                    expected.entry(key).or_default().push((
                        character_id.into_i32() as usize,
                        *default_state,
                        *default_variant,
                    ));
                    portraits += 1;
                }
            }
            assert_eq!(model.cells.len(), expected.len(), "grid{id}");
            for (cell_index, cell) in model.cells.iter().enumerate() {
                assert_eq!((cell.page, cell.x, cell.y), cell_order[cell_index]);
                let defs = &expected[&(cell.page, cell.x, cell.y)];
                assert_eq!(cell.definitions.len(), defs.len());
                if defs.len() > 1 {
                    stacked += 1;
                }
                if defs[0].1 != 0 && first_selections[cell.page as usize].is_none() {
                    first_selections[cell.page as usize] = Some(cell_index);
                }
                for (definition, &(character_id, state, variant)) in
                    cell.definitions.iter().zip(defs)
                {
                    assert_eq!(
                        (
                            definition.character_id,
                            definition.default_state,
                            definition.default_variant
                        ),
                        (character_id, state, variant)
                    );
                    if state != 0 {
                        let actual = definition.state().unwrap();
                        assert_eq!(actual.index, state, "grid{id},character{character_id}");
                        assert_eq!(
                            actual.variant().unwrap().index,
                            variant,
                            "grid{id},character{character_id}"
                        );
                    }
                    for state in &definition.states {
                        for variant in &state.variants {
                            portrait_assets.insert(variant.portrait.to_ascii_lowercase());
                            if !variant.full_sprite.is_empty() {
                                assets.insert(format!(
                                    "/chars/{}.txa",
                                    variant.full_sprite.to_ascii_lowercase()
                                ));
                            }
                        }
                    }
                }
            }
            assert_eq!(model.selections, first_selections);
            for argument in -4..8 {
                let m = CharacterModel::new(grid, &tables.chars_sprite_info, id, argument);
                let requested = argument & 3;
                let expected_page = (0..4)
                    .map(|offset| ((requested + offset) & 3) as u8)
                    .find(|p| m.cells.iter().any(|c| c.page == *p))
                    .unwrap_or(0);
                assert_eq!(m.page, expected_page);
            }
        }
        for path in &assets {
            assert!(
                rom.find_file(path).is_ok(),
                "Missing full character sprite{path}"
            );
        }
        let entry = rom.find_file("/chars.txa").unwrap();
        let mut atlas_bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut atlas_bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&atlas_bytes).unwrap();
        for name in &portrait_assets {
            assert!(
                atlas.name_to_index.contains_key(name),
                "Missing character portrait{name}"
            );
        }
        assert!(stacked > 0);
        eprintln!(
            "Validated{} grids,{} portraits,{} stacked cells,{} full-sprite archives,{} portrait textures",
            tables.chars_grid_info.len(),
            portraits,
            stacked,
            assets.len(),
            portrait_assets.len()
        );
    }
}
