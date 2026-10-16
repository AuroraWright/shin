//! Updated616a0 fragment catalogs,unlock snapshot,page selection and details.
//! Startup/resources/transition/render ownership is added around this model.
use super::runtime_game_data::RuntimeGameData;
use shin_core::format::save::PersistData;

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
    artwork: Option<Vec<Vec<bool>>>,
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
            artwork: None,
        }
    }
    fn apply_artwork(&mut self, atlas: &crate::adv::character_view::CharacterTextures) {
        if self.artwork.is_some() { return; }
        let suffix = if self.alternate { "_org" } else { "" };
        self.artwork = Some(self.pages.iter().enumerate().map(|(page, rows)| {
            let title = atlas.contains(&format!("k_title{:02}{suffix}", page + 1));
            rows.iter().map(|id| title && atlas.contains(&format!("{id:02}{suffix}"))).collect()
        }).collect());
        if !self.cell_available(self.page, self.selected) {
            if let Some((page, cell)) = self.pages.iter().enumerate().find_map(|(page, rows)| {
                (0..rows.len()).find(|&cell| self.cell_available(page, cell)).map(|cell| (page, cell))
            }) { self.page = page; self.selected = cell; }
        }
    }
    pub(super) fn cell_available(&self, page: usize, cell: usize) -> bool {
        self.artwork.as_ref().is_none_or(|pages| pages[page][cell])
    }
    pub(super) fn any_artwork(&self) -> bool {
        self.artwork.as_ref().is_none_or(|pages| pages.iter().flatten().any(|&yes| yes))
    }
    pub(super) fn can_exit(&self) -> bool { !self.alternate || !self.any_artwork() }
    pub(super) fn turn_page(&mut self, direction: i32) {
        for step in 1..=self.pages.len() {
            let page = (self.page as i32 + direction * step as i32).rem_euclid(self.pages.len() as i32) as usize;
            if let Some(cell) = (0..self.pages[page].len()).find(|&cell| self.cell_available(page, cell)) {
                self.page = page;
                self.selected = self.selected.min(self.pages[page].len() - 1);
                if !self.cell_available(page, self.selected) { self.selected = cell; }
                return;
            }
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
        if !self.cell_available(self.page, self.selected) { return Vec::new(); }
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
        if !self.any_artwork() { return false; }
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
        let candidate = (3 * row + column) as usize;
        self.selected = if self.cell_available(self.page, candidate) { candidate } else {
            let step = if dy != 0 { dy * 3 } else { dx };
            (1..=count).map(|i| (candidate as i32 + step * i).rem_euclid(count) as usize)
                .find(|&cell| self.cell_available(self.page, cell)).unwrap_or(old)
        };
        old != self.selected
    }
    ///64ac0 uses six authored432x244 rectangles in1920x1080 coordinates.
    pub fn cell_at(&self, x: f32, y: f32) -> Option<usize> {
        catalog::CELLS
            .iter()
            .take(self.pages[self.page].len())
            .enumerate()
            .position(|(cell, &(left, top))| {
                self.cell_available(self.page, cell) && x >= left as f32
                    && x < (left + 432) as f32
                    && y >= top as f32
                    && y < (top + 244) as f32
            })
    }
}

#[cfg(test)]
mod tests;
