//! Updated ADV100 mode ownership: d1070 push, d11c8 prune/poll and d147c draw.
//! New modes join the owner immediately, but await the next poll's snapshot.
use anyhow::Result;
use shin_core::time::Ticks;

pub trait Mode<S>: Sized {
    /// Native virtual10 returns a word whose low bit indicates completion.
    fn completion(&mut self) -> u32;
    /// Owner-side effects of a native deleting destructor run while frame
    /// services still exist, before the Rust owner/resources are dropped.
    fn retire(&mut self, _services: &mut S) {}
    /// Native virtual18 receives the permission from the mode above it.
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut S,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32>;
}

pub struct ModeStack<M> {
    active: Vec<Option<M>>,
    snapshot_len: usize,
}
impl<M> Default for ModeStack<M> {
    fn default() -> Self {
        Self { active: Vec::new(), snapshot_len: 0 }
    }
}

/// Append-only access while a mode is being polled. Indices in the current
/// snapshot survive vector growth, and the currently polled owner stays alive.
pub struct ModePush<'a, M> {
    active: &'a mut Vec<Option<M>>,
}
impl<M> ModePush<'_, M> {
    pub fn push(&mut self, mode: M) {
        self.active.push(Some(mode));
    }
    pub fn len(&self) -> usize {
        self.active.len()
    }
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

impl<M> ModeStack<M> {
    /// Transfers ownership without cloning or retaining the mode.
    pub fn push(&mut self, mode: M) {
        self.active.push(Some(mode));
    }
    /// Native d11b8 reports emptiness, including modes completed this frame.
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
    pub fn len(&self) -> usize {
        self.active.len()
    }
    pub fn iter(&self) -> impl Iterator<Item = &M> {
        self.active.iter().map(|mode| mode.as_ref().unwrap())
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut M> {
        self.active.iter_mut().map(|mode| mode.as_mut().unwrap())
    }
    /// Native d147c draws the last poll's borrowed snapshot in forward order.
    /// Modes pushed during polling are excluded, and modes completing during
    /// polling remain in this snapshot until the next poll prunes them.
    pub fn draw(&mut self, mut draw: impl FnMut(&mut M)) {
        for mode in &mut self.active[..self.snapshot_len] {
            draw(mode.as_mut().unwrap());
        }
    }
    /// Read-only rendering uses the same retained native snapshot.
    pub fn draw_ref(&self, mut draw: impl FnMut(&M)) {
        for mode in &self.active[..self.snapshot_len] {
            draw(mode.as_ref().unwrap());
        }
    }
    pub fn poll<S>(&mut self, delta: Ticks, initial_permission: u32, services: &mut S) -> Result<bool>
    where
        M: Mode<S>,
    {
        // std::retain would drop completed modes too early. Native remove_if
        // queries in forward order, drops a displaced hole when a survivor
        // moves into it, then destroys the remaining tail in reverse order.
        let old_len = self.active.len();
        let mut first_hole = old_len;
        for index in 0..old_len {
            if self.active[index].as_mut().unwrap().completion() & 1 != 0 {
                first_hole = index;
                break;
            }
        }
        let mut write = first_hole;
        for read in first_hole.saturating_add(1)..old_len {
            if self.active[read].as_mut().unwrap().completion() & 1 == 0 {
                let survivor = self.active[read].take();
                let displaced = std::mem::replace(&mut self.active[write], survivor);
                if let Some(mut mode) = displaced {
                    mode.retire(services);
                    drop(mode);
                }
                write += 1;
            }
        }
        while self.active.len() > write {
            if let Some(mut mode) = self.active.pop().flatten() {
                mode.retire(services);
                drop(mode);
            }
        }
        self.snapshot_len = self.active.len();
        let mut permission = initial_permission & 1 != 0;
        for index in (0..self.snapshot_len).rev() {
            let mut mode = self.active[index].take().unwrap();
            let result = mode.poll(delta, permission, services, &mut ModePush { active: &mut self.active });
            self.active[index] = Some(mode);
            permission = result? & 1 != 0;
        }
        Ok(permission)
    }
    /// Native owner destruction retires modes in reverse order while their
    /// ADV/message/UI services remain available. Drop subsequently sees empty.
    pub fn clear_with<S>(&mut self, services: &mut S)
    where M: Mode<S>,
    {
        self.snapshot_len = 0;
        while let Some(mut mode) = self.active.pop().flatten() {
            mode.retire(services);
            drop(mode);
        }
    }
}
impl<M> Drop for ModeStack<M> {
    fn drop(&mut self) {
        // d0f40 destroys the owning vector from end to beginning.
        while let Some(mode) = self.active.pop() {
            drop(mode);
        }
    }
}

#[cfg(test)]
mod tests;
