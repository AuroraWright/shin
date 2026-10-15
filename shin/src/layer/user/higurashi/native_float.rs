//! Scalar FP32 arithmetic at original Tile/leaf-clip call sites. Keep native
//! NaN payload priority and positive default NaNs independent of host ISA.
const CANONICAL_NAN_BITS: u32 = 0x7fc0_0000;

/// Original scalar FADD with FPCR.DN=0. Preserve its operand order and quiet
/// NaN priority explicitly: Rust does not guarantee arithmetic NaN payloads.
/// Signaling inputs take priority over quiet inputs; opposing infinities
/// produce the native positive canonical NaN. Finite arithmetic stays FP32.
pub(super) fn add(first: f32, second: f32) -> f32 {
    let words = [first.to_bits(), second.to_bits()];
    let is_nan = |word: u32| word & 0x7f80_0000 == 0x7f80_0000 && word & 0x007f_ffff != 0;
    for signaling in [true, false] {
        for word in words {
            if is_nan(word) && (!signaling || word & 0x0040_0000 == 0) {
                return f32::from_bits(word | 0x0040_0000);
            }
        }
    }
    if words[0] & 0x7fff_ffff == 0x7f80_0000
        && words[1] & 0x7fff_ffff == 0x7f80_0000
        && words[0] != words[1]
    {
        return f32::from_bits(CANONICAL_NAN_BITS);
    }
    first + second
}

/// Original696d0 FSUB uses the same NaN priority without negating the second
/// NaN's sign/payload. Equal signed infinities produce the default NaN.
pub(super) fn sub(first: f32, second: f32) -> f32 {
    let words = [first.to_bits(), second.to_bits()];
    let is_nan = |word: u32| word & 0x7f80_0000 == 0x7f80_0000 && word & 0x007f_ffff != 0;
    for signaling in [true, false] {
        for word in words {
            if is_nan(word) && (!signaling || word & 0x0040_0000 == 0) {
                return f32::from_bits(word | 0x0040_0000);
            }
        }
    }
    if words[0] & 0x7fff_ffff == 0x7f80_0000 && words[0] == words[1] {
        return f32::from_bits(CANONICAL_NAN_BITS);
    }
    first - second
}
