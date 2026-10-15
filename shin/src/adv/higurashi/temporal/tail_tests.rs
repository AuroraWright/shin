//! Replay the independently captured original complete-owner events with the
//! same split negative-prefix/keyed-suffix storage used by live layer banks.
use super::*;

struct Group<T> {
    prefix: super::Group<T>,
    keyed: Vec<Entry<T>>,
}
impl<T> Group<T> {
    fn new(entries: Vec<Entry<T>>) -> Self {
        let (negative, keyed) = entries.into_iter().partition(|entry| entry.key < 0);
        Self {
            prefix: super::Group::new(negative),
            keyed,
        }
    }
    fn entries(&self) -> Vec<&Entry<T>> {
        self.prefix
            .entries()
            .iter()
            .chain(self.keyed.iter())
            .collect()
    }
    fn entries_mut(&mut self) -> Vec<&mut Entry<T>> {
        self.prefix
            .entries_mut()
            .iter_mut()
            .chain(self.keyed.iter_mut())
            .collect()
    }
    fn publish(&mut self, key: i32, owner: T) {
        if let Some(entry) = self.keyed.iter_mut().find(|entry| entry.key == key) {
            entry.owner = Some(owner);
        } else {
            let index = self.keyed.partition_point(|entry| entry.key < key);
            self.keyed.insert(
                index,
                Entry {
                    key,
                    remaining: f32::from_bits(1),
                    owner: Some(owner),
                },
            );
        }
    }
    fn unload(&mut self, key: i32, delay: f32) {
        let Some(index) = self.keyed.iter().position(|entry| entry.key == key) else {
            return;
        };
        let entry = self.keyed.remove(index);
        if delay > 0.0 {
            self.prefix.prepend_retiring(entry.owner.unwrap(), delay);
        }
    }
    fn advance(
        &mut self,
        delta: f32,
        mut advance: impl FnMut(&mut T, f32) -> Result<()>,
    ) -> Result<()> {
        let count = self.keyed.len();
        let keyed = &mut self.keyed;
        // Both callbacks share one observer while preserving prefix->suffix.
        let callback = std::cell::RefCell::new(&mut advance);
        self.prefix.advance_with_tail(
            delta,
            count,
            |owner, d| (callback.borrow_mut())(owner, d),
            || {
                for entry in keyed {
                    (callback.borrow_mut())(entry.owner.as_mut().unwrap(), delta)?;
                }
                Ok(())
            },
        )
    }
    fn fast_forward(&mut self, mut skip: impl FnMut(&mut T) -> Result<()>) -> Result<()> {
        let count = self.keyed.len();
        let keyed = &mut self.keyed;
        let callback = std::cell::RefCell::new(&mut skip);
        self.prefix.fast_forward_with_tail(
            count,
            |owner| (callback.borrow_mut())(owner),
            || {
                for entry in keyed {
                    (callback.borrow_mut())(entry.owner.as_mut().unwrap())?;
                }
                Ok(())
            },
        )
    }
    fn copy_retired_from(
        &mut self,
        source: &Self,
        copy: impl FnMut(&T) -> Result<Option<T>>,
    ) -> Result<()> {
        self.prefix
            .copy_retired_from_with_tail(&source.prefix, self.keyed.len(), copy)
    }
}

// Use the full immutable native action/event comparisons, including mixed
// expiry/destruction order and WIPE compaction with active keyed children.
#[path = "tests.rs"]
mod original_comparison;
