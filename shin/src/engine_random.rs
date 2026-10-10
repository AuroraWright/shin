//! The native non-script random stream (fbac0), independent of the VM LCG.
use std::sync::OnceLock;

pub(crate) struct EngineRandom {
    words: [u32; 624],
    index: usize,
}

impl EngineRandom {
    fn new() -> Self {
        Self::from_seed(5489)
    }

    pub(crate) fn from_seed(seed: u32) -> Self {
        let mut words = [0u32; 624];
        words[0] = seed; // Native static initialization f8ee4..f8f38.
        for i in 1..624 {
            let previous = words[i - 1];
            words[i] = (previous ^ (previous >> 30))
                .wrapping_mul(1_812_433_253)
                .wrapping_add(i as u32);
        }
        Self { words, index: 0 }
    }

    pub(crate) fn next(&mut self) -> u32 {
        // Native twists one word per call, then tempers that word.
        let i = self.index;
        let next = (i + 1) % 624;
        let joined = (self.words[i] & 0x8000_0000) | (self.words[next] & 0x7fff_ffff);
        let mut value = self.words[(i + 397) % 624]
            ^ (joined >> 1)
            ^ if joined & 1 != 0 { 0x9908_b0df } else { 0 };
        self.words[i] = value;
        self.index = next;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^ (value >> 18)
    }
}

#[cfg(test)]
thread_local! { static SEED_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
thread_local! { static REFERENCE_STREAM: std::cell::RefCell<Option<EngineRandom>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(crate) fn with_reference_seed<R>(seed: u32, action: impl FnOnce() -> R) -> R {
    struct Restore(Option<EngineRandom>);
    impl Drop for Restore {
        fn drop(&mut self) {
            REFERENCE_STREAM.with(|stream| *stream.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(
        REFERENCE_STREAM.with(|stream| stream.replace(Some(EngineRandom::from_seed(seed)))),
    );
    action()
}
#[cfg(test)]
pub(crate) fn seed_call_count() -> usize {
    SEED_CALLS.with(|count| count.get())
}

pub(crate) fn next_seed() -> u32 {
    #[cfg(test)]
    SEED_CALLS.with(|count| count.set(count.get() + 1));
    #[cfg(test)]
    if let Some(value) =
        REFERENCE_STREAM.with(|stream| stream.borrow_mut().as_mut().map(EngineRandom::next))
    {
        return value;
    }
    static STREAM: OnceLock<parking_lot::Mutex<EngineRandom>> = OnceLock::new();
    STREAM
        .get_or_init(|| parking_lot::Mutex::new(EngineRandom::new()))
        .lock()
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_seed_scope_restores_the_outer_stream() {
        with_reference_seed(5489, || {
            assert_eq!(next_seed(), 3499211612);
            with_reference_seed(5489, || {
                assert_eq!(next_seed(), 3499211612);
            });
            assert_eq!(next_seed(), 581869302);
        });
    }

    #[test]
    fn native_mt_stream_matches_standard_sequence_across_wraps() {
        let mut rng = EngineRandom::new();
        for expected in [3499211612, 581869302, 3890346734, 3586334585, 545404204] {
            assert_eq!(rng.next(), expected);
        }
        // Values at twist boundaries, independently generated with std::mt19937.
        let mut targets = [
            (623, 4020325887),
            (624, 4178893912),
            (1247, 2538210759),
            (1248, 358555951),
            (1871, 2449624894),
            (1872, 4018933476),
        ]
        .into_iter()
        .peekable();
        for index in 5..=1872 {
            let value = rng.next();
            if targets.peek().is_some_and(|&(target, _)| target == index) {
                assert_eq!(value, targets.next().unwrap().1, "stream index {index}");
            }
        }
    }
}
