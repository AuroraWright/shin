//! Native39114/3915c/391d8/391e4 route primitives,sharing the engine bit buffer.
use shin_core::primitives::bit_utils::BitBuffer;
fn grow(data: &mut Vec<u8>, position: usize, bits: usize) {
    let needed = (position + bits).div_ceil(8);
    if needed >= data.len() {
        data.resize(needed.next_multiple_of(1024), 0);
    }
}
fn with(data: &mut Vec<u8>, position: &mut usize, bits: usize, f: impl FnOnce(&mut BitBuffer)) {
    grow(data, *position, bits);
    let mut buffer = BitBuffer::with_data(std::mem::take(data), *position);
    f(&mut buffer);
    (*data, *position) = buffer.into_data();
}
fn write_bool(data: &mut Vec<u8>, position: &mut usize, value: bool) {
    with(data, position, 1, |b| b.write_bool(value));
    // Native38e24 ORs a true bit but masks the remaining byte for false.
    let partial = *position % 8;
    if !value && partial != 0 {
        data[*position / 8] &= 0xff << (8 - partial);
    }
}
pub fn write_bits(data: &mut Vec<u8>, position: &mut usize, bits: u8, value: u32) {
    let mask = u32::MAX >> (32 - bits);
    with(data, position, bits as usize, |b| {
        b.write_int(bits, value & mask)
    });
    // Native38cb0 preserves the leading prefix and clears unused trailing bits.
    let partial = *position % 8;
    if partial != 0 {
        data[*position / 8] &= 0xff << (8 - partial);
    }
}
pub fn write_persistent(data: &mut Vec<u8>, position: &mut usize, value: i32) {
    let compact = matches!(value, 0 | 1);
    write_bool(data, position, compact);
    if compact {
        write_bool(data, position, value != 0);
    } else {
        write_bits(data, position, 16, value as u32);
    }
}
fn read_bool(data: &mut Vec<u8>, position: &mut usize) -> bool {
    let mut result = false;
    with(data, position, 1, |b| result = b.read_bool().unwrap());
    result
}
pub fn read_persistent(data: &mut Vec<u8>, position: &mut usize) -> i32 {
    if read_bool(data, position) {
        i32::from(read_bool(data, position))
    } else {
        let mut result = 0i16;
        with(data, position, 16, |b| result = b.read_int(16).unwrap());
        i32::from(result)
    }
}
pub fn read_quiz(data: &mut Vec<u8>, position: &mut usize) -> i32 {
    let mut result = 0u8;
    with(data, position, 6, |b| result = b.read_int(6).unwrap());
    i32::from(result)
}
pub fn read_selection(data: &mut Vec<u8>, position: &mut usize) -> i32 {
    let mut result = 0u8;
    with(data, position, 4, |b| result = b.read_int(4).unwrap());
    i32::from(result)
}
