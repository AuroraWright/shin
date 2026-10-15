//! Updated d9d7c TIPS catalog and dbc04/dc1d8 input, with full scene payloads.
//! The separate browser uses a two-column list of pre-rendered TIPS atlases.
use super::runtime_game_data::RuntimeGameData;
use crate::{
    adv::higurashi::{
        autosave::{ProgressEntry, SaveProgress},
        input::Buttons,
    },
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use shin_core::{
    format::{
        save::higurashi::{SaveDate, SaveVectors},
        scenario::higurashi::{HigurashiScenario, HigurashiVariant},
    },
    time::Ticks,
};

pub(super) const ROW_HEIGHT: i32 = 92;
pub(super) const VIEW_HEIGHT: i32 = 818;
pub(super) mod factory;
pub(super) mod motion;
pub(super) mod opening;
pub(super) mod view;

pub(super) struct Row {
    /// Unlock bits and GameData's acquired list use the unfiltered SNR index.
    pub source_index: usize,
    pub atlas_id: u16,
    pub scenario_id: u16,
    pub texture_path: String,
    pub current: bool,
    pub status: u32,
    pub flash: HigurashiTweener,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    FadeMusic(u32),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SceneRequest {
    Story { data: RuntimeGameData, flags: u32 },
    Title { selected: i32, mode: u32 },
    Origin { selected: i32 },
}
#[derive(Clone, Copy)]
enum Pending {
    Start(usize),
    Retreat,
}
pub(in crate::app::higurashi) struct Model {
    pub data: Option<RuntimeGameData>,
    pub alternate: bool,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub content_height: i32,
    pub completion_text: String,
    pub scroll: HigurashiTweener,
    pub feedback: HigurashiTweener,
    pub background_phase: f32,
    pub closing: bool,
    pub hit: i32,
    pub hovered: i32,
    pub pressed: i32,
    pub speed: f32,
    pending: Option<Pending>,
    actions: Vec<Action>,
}
fn bit(words: &[u32], at: usize) -> bool {
    words
        .get(at / 32)
        .is_some_and(|w| w & (1 << (at % 32)) != 0)
}
fn control(clock: &mut HigurashiTweener, value: f32, ticks: f32, flags: u32) {
    clock
        .control(value, Ticks::from_f32(ticks), flags)
        .expect("Verified native TIPS tween");
}
fn flash(clock: &mut HigurashiTweener, value: f32, release: bool) {
    clock.fast_forward_to(clock.value());
    control(clock, value, f32::from_bits(0x3e4ccccd), 0x40);
    if release {
        control(clock, 0.0, f32::from_bits(0x3e4ccccd), 0x40);
    }
}
impl Model {
    pub fn new(
        scenario: &HigurashiScenario,
        data: Option<RuntimeGameData>,
        selected: i32,
        alternate_flag: u32,
        vectors: &SaveVectors,
    ) -> Result<Self> {
        ensure!(
            scenario.variant == HigurashiVariant::Updated,
            "TIPS requires the updated Higurashi scenario"
        );
        let alternate = data.as_ref().map_or(alternate_flag & 1 != 0, |d| {
            d.progress.active_entry().scenario_id > 9999
        });
        let mut selected_row = 0;
        let mut unlocked = 0;
        let mut rows = Vec::new();
        for (source_index, tip) in scenario.tips_info.iter().enumerate() {
            let scenario_id = tip.updated_field.expect("Updated TIPS scenario ID");
            if (scenario_id > 9999) != alternate {
                continue;
            }
            let current = data
                .as_ref()
                .is_some_and(|d| d.trailing_values.contains(&(source_index as u16)));
            let status = if bit(&vectors.extra_words_0, 2 * source_index) {
                unlocked += 1;
                1 + u32::from(bit(&vectors.extra_words_0, 2 * source_index + 1))
            } else {
                0
            };
            if source_index as i32 == selected {
                selected_row = rows.len();
            }
            rows.push(Row {
                source_index,
                atlas_id: tip.id,
                scenario_id,
                texture_path: tip.texture_path.as_ref().to_owned(),
                current,
                status,
                flash: HigurashiTweener::new(0.0),
            });
        }
        ensure!(!rows.is_empty(), "TIPS catalog is empty");
        let percentage = unlocked * 1000 / rows.len();
        let completion_text = if percentage >= 1000 {
            "達成率:100％".to_owned()
        } else {
            format!("達成率:{}.{}％", percentage / 10, percentage % 10)
        };
        let content_height = ROW_HEIGHT * rows.len().div_ceil(2) as i32;
        let row_top = ROW_HEIGHT * (selected_row / 2) as i32;
        let scroll = (row_top - 363).clamp(0, (content_height - VIEW_HEIGHT).max(0)) as f32;
        Ok(Self {
            data,
            alternate,
            rows,
            selected: selected_row,
            content_height,
            completion_text,
            scroll: HigurashiTweener::new(scroll),
            feedback: HigurashiTweener::new(0.0),
            background_phase: 0.0,
            closing: false,
            hit: -1,
            hovered: -1,
            pressed: -1,
            speed: 0.0,
            pending: None,
            actions: vec![],
        })
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    fn reveal_selected(&mut self) {
        let top = (ROW_HEIGHT * (self.selected / 2) as i32) as f32;
        let scroll = self.scroll.value();
        let next = if top < scroll {
            top.max(0.0)
        } else if top - scroll + ROW_HEIGHT as f32 > VIEW_HEIGHT as f32 {
            (top + (ROW_HEIGHT - VIEW_HEIGHT) as f32).min(self.content_height as f32)
        } else {
            scroll
        };
        control(&mut self.scroll, next, 4.0, 0);
    }
    pub fn poll_controls(&mut self, permission: bool, captured: bool, buttons: Buttons) -> bool {
        if permission && !captured && self.scroll.is_idle() {
            if buttons.pressed & (1 << 25) != 0 {
                self.actions.push(Action::Cue("cancel"));
                self.retreat();
            } else if buttons.pressed & (1 << 24) != 0 {
                self.confirm(self.selected);
            } else {
                let dx = if buttons.repeat & (1 << 7) != 0 {
                    -1
                } else {
                    i32::from(buttons.repeat & (1 << 5) != 0)
                };
                let dy = if buttons.repeat & (1 << 4) != 0 {
                    -1
                } else if buttons.repeat & (1 << 6) != 0 {
                    1
                } else if buttons.repeat & (1 << 10) != 0 {
                    -8
                } else if buttons.repeat & (1 << 11) != 0 {
                    8
                } else {
                    0
                };
                if dx != 0 || dy != 0 {
                    let row = (self.selected as i32 / 2 + dy)
                        .clamp(0, self.rows.len().div_ceil(2) as i32 - 1);
                    let column = (self.selected as i32 % 2 + dx).clamp(0, 1);
                    let selected = (row as usize * 2 + column as usize).min(self.rows.len() - 1);
                    if selected != self.selected {
                        self.actions.push(Action::Cue("move"));
                        self.speed = 0.0;
                        self.selected = selected;
                        self.reveal_selected();
                    }
                }
            }
        }
        !self.closing
    }
    fn keep_selection_visible(&mut self, scroll: f32) {
        let bottom = scroll + VIEW_HEIGHT as f32;
        loop {
            let top = (ROW_HEIGHT * (self.selected / 2) as i32) as f32;
            if scroll > top {
                self.selected += 2;
            } else if bottom < top + ROW_HEIGHT as f32 {
                self.selected -= 2;
            } else {
                break;
            }
        }
        self.scroll.fast_forward_to(scroll);
    }
    pub fn advance(&mut self, delta: Ticks) {
        self.advance_with_metrics(delta, None);
    }
    pub(super) fn advance_with_metrics(
        &mut self,
        delta: Ticks,
        metrics: Option<&mut crate::adv::higurashi::browser_controls::Scrollbar>,
    ) {
        if self.speed != 0.0 {
            let old = self.scroll.value();
            let next = (old - self.speed)
                .min((self.content_height - VIEW_HEIGHT) as f32)
                .max(0.0);
            if next != old {
                self.keep_selection_visible(next);
            }
            self.speed *= f32::from_bits(0x3f733333).powf(delta.as_f32());
            if self.speed.abs() < f32::from_bits(0x3dcccccd) {
                self.speed = 0.0;
            }
        }
        if let Some(metrics) = metrics {
            *metrics = crate::adv::higurashi::browser_controls::Scrollbar {
                position: self.scroll.value() as i32,
                visible: VIEW_HEIGHT,
                first: 0,
                end: self.content_height,
            };
        }
        self.scroll.update(delta);
        for row in &mut self.rows {
            row.flash.update(delta);
        }
        self.feedback.update(delta);
        self.background_phase += delta.as_f32() * f32::from_bits(0x3c888889);
    }
    ///dc1d8 returns true even for empty list gutters;hit1 selects the back button.
    pub fn contact(&mut self, x: f32, y: f32) -> bool {
        self.hovered = -1;
        self.speed = 0.0;
        if (1764.0..1848.0).contains(&x) && (30.0..114.0).contains(&y) {
            self.hit = 1;
        } else {
            self.hit = if (162.0..980.0).contains(&y) { -1 } else { 0 };
            if self.hit == -1 {
                let column = if (150.0..950.0).contains(&x) {
                    Some(0)
                } else if (970.0..1770.0).contains(&x) {
                    Some(1)
                } else {
                    None
                };
                if let Some(column) = column {
                    let vertical = (self.scroll.value() + y - 162.0) as i32;
                    let row = vertical / ROW_HEIGHT;
                    let remainder = vertical % ROW_HEIGHT;
                    let at = row * 2 + column;
                    if (2..90).contains(&remainder) && at >= 0 && (at as usize) < self.rows.len() {
                        self.hovered = at;
                    }
                }
            }
        }
        true
    }
    pub fn press_contact(&mut self) {
        if self.hit == 1 {
            flash(&mut self.feedback, f32::from_bits(0x3f333333), false);
        } else if self.hit < 0 && self.hovered >= 0 && (self.hovered as usize) < self.rows.len() {
            self.selected = self.hovered as usize;
            self.scroll.fast_forward_to(self.scroll.value());
            self.reveal_selected();
        }
    }
    pub fn confirm_contact(&mut self) {
        if self.hit == 1 {
            self.actions.push(Action::Cue("decide"));
            self.retreat();
        } else if self.hit < 0 && self.hovered >= 0 {
            if (self.hovered as usize) < self.rows.len() {
                self.selected = self.hovered as usize;
                self.scroll.fast_forward_to(self.scroll.value());
                self.reveal_selected();
            }
            self.confirm(self.selected);
        }
    }
    pub fn drag(&mut self, from_y: f32, to_y: f32) {
        if self.hit >= 0 {
            self.release_contact();
        } else if self.hovered >= 0 {
            let next = (self.scroll.value() - (to_y - from_y))
                .min((self.content_height - VIEW_HEIGHT) as f32)
                .max(0.0);
            self.keep_selection_visible(next);
        } else {
            self.hovered = -1;
            self.pressed = -1;
        }
    }
    pub fn inertia(&mut self, speed: f32) {
        if self.hit < 0 && self.hovered >= 0 {
            self.speed = speed;
        }
    }
    pub fn release_contact(&mut self) {
        if self.hit >= 0 {
            flash(&mut self.feedback, 0.0, false);
            self.hit = 0;
        }
    }
    fn confirm(&mut self, selected: usize) {
        flash(
            &mut self.rows[selected].flash,
            f32::from_bits(0x3f333333),
            true,
        );
        if self.rows[selected].status == 0 {
            self.actions.push(Action::Cue("error"));
        } else {
            self.actions.push(Action::Cue("decide"));
            self.actions.push(Action::FadeMusic(30));
            //dbe3c repeats the reset/flash pair before attaching its callback.
            flash(
                &mut self.rows[selected].flash,
                f32::from_bits(0x3f333333),
                true,
            );
            self.pending = Some(Pending::Start(selected));
            self.closing = true;
            self.speed = 0.0;
        }
    }
    fn retreat(&mut self) {
        self.closing = true;
        flash(&mut self.feedback, f32::from_bits(0x3f333333), true);
        self.speed = 0.0;
        self.pending = Some(Pending::Retreat);
    }
    ///dd1f4/dd51c wait for the owned flash before requesting a scene. Fresh
    ///native records leave date bytes unwritten;desktop supplies an explicit
    ///clock here instead of treating those indeterminate bytes as a date.
    pub fn poll_request(
        &mut self,
        mut fresh_date: impl FnMut() -> Result<SaveDate>,
        mut seed: impl FnMut() -> Result<u32>,
    ) -> Result<Option<SceneRequest>> {
        let Some(pending) = self.pending else {
            return Ok(None);
        };
        let request = match pending {
            Pending::Start(at) => {
                if self.rows[at].flash.is_waiting() {
                    return Ok(None);
                }
                let mut data = if let Some(data) = &self.data {
                    let mut data = data.clone();
                    data.progress.set_active_index(1)?;
                    data
                } else {
                    let entry = || ProgressEntry {
                        scenario_id: 0,
                        random_seed: 0,
                        save_position: 0,
                        replay_target: 0,
                        response_data: vec![],
                    };
                    RuntimeGameData::from_progress(
                        SaveProgress::new(0, 0, [entry(), entry()])?,
                        fresh_date()?,
                        vec![],
                        vec![],
                    )
                };
                let active = data.progress.active_index();
                let entry = &mut data.progress.entries[active];
                entry.scenario_id = i32::from(self.rows[at].scenario_id);
                entry.random_seed = seed()?;
                entry.save_position = 0;
                entry.replay_target = 0;
                SceneRequest::Story { data, flags: 0 }
            }
            Pending::Retreat => {
                if self.feedback.is_waiting() {
                    return Ok(None);
                }
                if let Some(data) = &self.data {
                    SceneRequest::Story {
                        data: data.clone(),
                        flags: 1,
                    }
                } else if self.alternate {
                    SceneRequest::Origin { selected: 9 }
                } else {
                    SceneRequest::Title {
                        selected: 6,
                        mode: u32::MAX,
                    }
                }
            }
        };
        self.pending = None;
        Ok(Some(request))
    }
}

#[cfg(test)]
pub(in crate::app::higurashi) mod tests;
