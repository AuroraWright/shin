//! Updated616a0 fragment catalogs,unlock snapshot,page selection and details.
//! Startup/resources/transition/render ownership is added around this model.
use super::runtime_game_data::RuntimeGameData;
use shin_core::format::save::{PersistData, higurashi::GameData};

mod catalog;
pub(super) mod motion;
pub(super) mod opening;
pub(super) mod view;

#[derive(Debug)]
pub(super) struct Fragment {
    pub variable: i32,
    pub follows: [i32; 5],
    pub title: &'static str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Detail {
    pub fragment: usize,
    pub title: &'static str,
    pub grey: bool,
}

pub(in crate::app::higurashi) struct Model {
    /// The separate menu owns the factory's complete GameData copy.
    pub data: RuntimeGameData,
    pub alternate: bool,
    pub statuses: Vec<i32>,
    pub pages: Vec<Vec<usize>>,
    pub page: usize,
    pub selected: usize,
    pub bgm_id: i32,
}
impl Model {
    pub fn new(
        data: impl Into<RuntimeGameData>,
        selected_id: i32,
        persistent: &PersistData,
    ) -> Self {
        let data = data.into();
        let alternate = data.progress.active_entry().scenario_id > 9999;
        let descriptors = if alternate {
            &catalog::ALTERNATE[..]
        } else {
            &catalog::NORMAL[..]
        };
        let complete = alternate && persistent.get(176) > 1;
        let mut previous_status = 0;
        let statuses: Vec<i32> = descriptors
            .iter()
            .map(|row| {
                let value = if alternate {
                    if row.variable < 0 {
                        if complete { 2 } else { previous_status }
                    } else {
                        i32::from(
                            data.byte_variables
                                .get(row.variable as usize)
                                .copied()
                                .unwrap_or(0),
                        )
                    }
                } else {
                    persistent.get(row.variable as usize)
                };
                let status = if row.follows[0] == -1 && value == 0 {
                    1
                } else {
                    value
                };
                previous_status = status;
                status
            })
            .collect();
        let source_pages = if alternate {
            &catalog::ALTERNATE_PAGES[..]
        } else {
            &catalog::NORMAL_PAGES[..]
        };
        let mut page = 0;
        let mut selected = 0;
        let mut unlocked = 0;
        let pages = source_pages
            .iter()
            .enumerate()
            .map(|(p, rows)| {
                rows.iter()
                    .enumerate()
                    .filter_map(|(s, &id)| {
                        if matches!((alternate, id), (false, 87) | (true, 50) | (true, 51))
                            && statuses[id] == 0
                        {
                            return None;
                        }
                        if statuses[id] != 0 {
                            unlocked += 1;
                        }
                        if id as i32 == selected_id {
                            page = p;
                            selected = s;
                        }
                        Some(id)
                    })
                    .collect()
            })
            .collect();
        let bgm_id = if alternate || unlocked < 40 {
            86
        } else if unlocked <= 54 {
            130
        } else if unlocked <= 69 {
            128
        } else if unlocked <= 84 {
            83
        } else {
            129
        };
        Self {
            data,
            alternate,
            statuses,
            pages,
            page,
            selected,
            bgm_id,
        }
    }
    pub fn catalog(&self) -> &'static [Fragment] {
        if self.alternate {
            &catalog::ALTERNATE
        } else {
            &catalog::NORMAL
        }
    }
    pub fn selected_fragment(&self) -> usize {
        self.pages[self.page][self.selected]
    }
    ///62d08 emits at most four dependent fragment labels in descriptor order.
    pub fn details(&self) -> Vec<Detail> {
        self.catalog()[self.selected_fragment()].follows[..4]
            .iter()
            .take_while(|&&id| id >= 0)
            .map(|&id| {
                let fragment = id as usize;
                Detail {
                    fragment,
                    title: self.catalog()[fragment].title,
                    grey: self.statuses[fragment] > 1,
                }
            })
            .collect()
    }
    ///64890 wraps a three-column,two-row page and skips absent cells.
    /// Up/left win opposite repeat bits;vertical movement precedes horizontal.
    pub fn navigate(&mut self, repeat: u32) -> bool {
        let dx = if repeat & (1 << 7) != 0 {
            -1
        } else {
            i32::from(repeat & (1 << 5) != 0)
        };
        let dy = if repeat & (1 << 4) != 0 {
            -1
        } else {
            i32::from(repeat & (1 << 6) != 0)
        };
        if dx == 0 && dy == 0 {
            return false;
        }
        let old = self.selected;
        let count = self.pages[self.page].len() as i32;
        let mut row = old as i32 / 3;
        let mut column = old as i32 % 3;
        if dy != 0 {
            loop {
                row = (row + dy).rem_euclid(2);
                if 3 * row + column < count {
                    break;
                }
            }
        }
        if dx != 0 {
            loop {
                column = (column + dx).rem_euclid(3);
                if 3 * row + column < count {
                    break;
                }
            }
        }
        self.selected = (3 * row + column) as usize;
        old != self.selected
    }
    ///64ac0 uses six authored432x244 rectangles in1920x1080 coordinates.
    pub fn cell_at(&self, x: f32, y: f32) -> Option<usize> {
        catalog::CELLS
            .iter()
            .take(self.pages[self.page].len())
            .position(|&(left, top)| {
                x >= left as f32
                    && x < (left + 432) as f32
                    && y >= top as f32
                    && y < (top + 244) as f32
            })
    }
}

#[cfg(test)]
mod tests;
