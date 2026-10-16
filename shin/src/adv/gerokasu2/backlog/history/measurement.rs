//! Atomic row publications from original7FC00's outward measurement loop.
use super::{Entry, Geometry, GeometryRow};

/// The native worker fixes the current row at y0, then measures preceding and
/// following rows alternately. Metadata exists for the whole log immediately;
/// pending rows keep zero voice count, height and position.
pub struct Measurement {
    geometry: Geometry,
    portrait_minimum: Vec<bool>,
    next_iteration: usize,
    published: usize,
}
impl Measurement {
    pub fn new(entries: &[Entry], current: Option<usize>) -> Option<Self> {
        if entries.is_empty() {
            return None;
        }
        let selected = current.unwrap_or(0).min(entries.len() - 1);
        Some(Self {
            geometry: Geometry {
                rows: entries
                    .iter()
                    .map(|entry| GeometryRow {
                        marker: entry.kind() != 0,
                        voice_count: 0,
                        top: 0.,
                        height: 0.,
                    })
                    .collect(),
                selected,
                allowed: selected..selected,
                initial_scroll: 0.,
                minimum: 0.,
                maximum: 0.,
                bottom: 0.,
            },
            portrait_minimum: entries
                .iter()
                .map(|entry| entry.kind() == 0 && entry.message_arg() >= 0)
                .collect(),
            next_iteration: 0,
            published: 0,
        })
    }
    fn next_candidate(&self) -> Option<(usize, usize)> {
        if self.complete() {
            return None;
        }
        let mut iteration = self.next_iteration;
        loop {
            let distance = iteration.div_ceil(2);
            let row = if iteration & 1 != 0 {
                self.geometry.selected.checked_sub(distance)
            } else {
                self.geometry.selected.checked_add(distance)
            };
            if let Some(row) = row.filter(|&row| row < self.geometry.rows.len()) {
                return Some((iteration, row));
            }
            iteration += 1;
        }
    }
    pub fn next_row(&self) -> Option<usize> {
        self.next_candidate().map(|(_, row)| row)
    }
    /// Publish one completed LogMessage layout. Invalid or out-of-order input
    /// leaves the entire snapshot unchanged. A cancelled host simply stops
    /// publishing; previously published coordinates never move.
    pub fn publish(
        &mut self,
        row: usize,
        text_height: f32,
        voice_count: u8,
    ) -> Result<(), &'static str> {
        let Some((iteration, expected)) = self.next_candidate() else {
            return Err("Ger backlog measurement is already complete");
        };
        if row != expected || !text_height.is_finite() || text_height < 0. {
            return Err("Ger backlog measurement disagrees with worker order");
        }
        let height = if self.portrait_minimum[row] && voice_count != 0 && text_height < 196. {
            196.
        } else {
            text_height
        } + 36.;
        let g = &mut self.geometry;
        let top = if row < g.selected {
            //7FFB0 and7FFC8 are separate scalar operations; retain both.
            g.minimum -= 8.;
            g.minimum -= height;
            g.allowed.start -= 1;
            g.minimum
        } else {
            if g.allowed.end != g.selected {
                g.bottom += 8.;
            }
            let top = g.bottom;
            g.bottom = height + g.bottom;
            g.allowed.end += 1;
            top
        };
        g.rows[row].top = top;
        g.rows[row].height = height;
        g.rows[row].voice_count = voice_count;
        g.maximum = (g.bottom - 832.).max(g.minimum);
        let selected = &g.rows[g.selected];
        g.initial_scroll = (selected.top + selected.height - 832.)
            .min(g.bottom - 832.)
            .max(g.minimum);
        self.next_iteration = iteration + 1;
        self.published += 1;
        Ok(())
    }
    pub fn geometry(&self) -> Geometry {
        self.geometry.clone()
    }
    pub fn geometry_ref(&self) -> &Geometry {
        &self.geometry
    }
    pub fn published_count(&self) -> usize {
        self.published
    }
    pub fn complete(&self) -> bool {
        self.published == self.geometry.rows.len()
    }
    /// Original84374 permits First when the worker ended or at least100 rows
    /// are measured. Asset readiness and cancellation ownership belong to the
    /// host; a cancelled private worker must not publish a new live browser.
    pub fn ready(&self, worker_done: bool) -> bool {
        worker_done || self.geometry.allowed.len() >= 100
    }
}

#[cfg(test)]
#[path = "measurement_tests.rs"]
mod tests;
