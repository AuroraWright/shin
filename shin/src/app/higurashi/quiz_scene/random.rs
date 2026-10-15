//! Native9fcb0/a0020 use minstd_rand and libc++ independent-bit rejection.
#[derive(Debug, Clone, Copy)]
pub(super) struct Random {
    state: u32,
}
const MODULUS: u64 = 2_147_483_647;
impl Random {
    pub fn new(seed: u32) -> Self {
        let state = (u64::from(seed) % MODULUS) as u32;
        Self {
            state: if state == 0 { 1 } else { state },
        }
    }
    pub fn state(&self) -> u32 {
        self.state
    }
    fn next(&mut self) -> u32 {
        self.state = (u64::from(self.state) * 48_271 % MODULUS) as u32;
        self.state
    }
    /// Quiz's original bounds require at most6 bits. Preserve libc++'s
    /// engine-range rejection before its requested-range rejection;modulo
    /// sampling would consume different values and choose different questions.
    pub fn uniform(&mut self, exclusive: u32) -> u32 {
        assert!((1..=60).contains(&exclusive));
        if exclusive == 1 {
            return 0;
        }
        let bits = 32 - (exclusive - 1).leading_zeros();
        let mask = (1 << bits) - 1;
        let accepted = (2_147_483_646u32 >> bits) << bits;
        loop {
            let value = self.next() - 1;
            if value < accepted {
                let result = value & mask;
                if result < exclusive {
                    return result;
                }
            }
        }
    }
}
