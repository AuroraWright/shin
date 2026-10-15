//! Implement the LZ77 variant used in the game.
//!
//! LZ77 is a lossless compression algorithm that compressed data by replacing repeated sequences of bytes with references to the previous occurrences of the same sequence.
//!
//! This variant of LZ77 precedes a block of data with an 8-bit bitmap specifying whether decompressor should read a literal byte or a reference.
//!
//! The references are encoded as 16-bit big-endian (sic!) integers. It encodes offset and length of the reference,
//!     but the amount of bits spent on each part is dependent on the format (const generics are used to specify this).
//!
//! Offset is specified as amount to seek back from the current position.
//!
//! The minimum offset is 1, so the actual offset is offset + 1.
//! The minimum length is 3, so the actual length is length + 3.
//!
//! Here's an example of a data stream encoded with 12 bits offset (and, hence, 4 bits length):
//!
//! ```text
//! // first byte is the bitmap. bitmap is read from LSB to MSB
//! 0b11000000 // 6 literals, 2 references
//! // 6 literal bytes encoding the string "HELLO "
//! 0x48 0x45 0x4c 0x4c 0x4f 0x20
//! // first reference
//! 0x3005 // offset <- 1+5=6, length <- 3+3=6
//! // ^ this seeks back 6 bytes and copies 6 bytes from there
//! // repeating the string "HELLO " previously decoded
//! // second reference
//! 0x800b // offset <- 1+11=12, length <- 3+8=11
//! // ^ this seeks back 12 bytes and copies 11 bytes from there
//! // repeating the string "HELLO HELLO" previously decoded
//! // note that you CAN reference the data that was itself decoded by a reference
//! ```
//!
//! ```
//! let compressed = [0b11000000, 0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x20, 0x30, 0x05, 0x80, 0x0b];
//! let mut decompressed = Vec::new();
//! shin_core::format::lz77::decompress::<12>(&compressed, &mut decompressed);
//! assert_eq!(decompressed, b"HELLO HELLO HELLO HELLO");
//! ```
//!
//! Encoding is (to be) implemented using a sliding window and a greedy algorithm.
//! Theoretically the efficiency can be improved by using a bit of backtracking,
//!     but it seems this improves compression ratio only by several percent (not worth the time).

use std::io;

use bytes::Buf;

/// Decode a complete stream into an explicitly bounded output.
///
/// Unlike the legacy append API, malformed references, truncated tokens and
/// decoded lengths that differ from `expected_size` return errors. This is used
/// by the FNT6 reader without changing the legacy formats' error behavior.
pub fn decompress_checked<const OFFSET_BITS: u32>(
    input: &[u8],
    expected_size: usize,
) -> io::Result<Vec<u8>> {
    if !(1..16).contains(&OFFSET_BITS) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid LZ offset bits",
        ));
    }
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
    let mut output = Vec::new();
    output
        .try_reserve_exact(expected_size)
        .map_err(io::Error::other)?;
    let mut position = 0;
    while position < input.len() {
        let map = input[position];
        position += 1;
        for bit in 0..8 {
            if position == input.len() {
                break;
            }
            if map & (1 << bit) == 0 {
                if output.len() == expected_size {
                    return Err(invalid("LZ output exceeds expected size"));
                }
                output.push(input[position]);
                position += 1;
            } else {
                let token = input
                    .get(position..position + 2)
                    .ok_or_else(|| invalid("Truncated LZ reference"))?;
                position += 2;
                let word = u16::from_be_bytes(token.try_into().unwrap());
                let distance = usize::from(word & ((1 << OFFSET_BITS) - 1)) + 1;
                let length = usize::from(word >> OFFSET_BITS) + 3;
                if distance > output.len() {
                    return Err(invalid("LZ reference precedes output"));
                }
                if length > expected_size - output.len() {
                    return Err(invalid("LZ output exceeds expected size"));
                }
                for _ in 0..length {
                    output.push(output[output.len() - distance]);
                }
            }
        }
    }
    if output.len() != expected_size {
        return Err(invalid("LZ decoded size differs from expected size"));
    }
    Ok(output)
}

#[cfg(test)]
#[path = "lz77_reference.rs"]
pub(crate) mod reference;

pub fn decompress<const OFFSET_BITS: u32>(input: &[u8], output: &mut Vec<u8>) {
    #[cfg(test)]
    let encoded_input = input;
    #[cfg(test)]
    let output_begin = output.len();
    let mut input = io::Cursor::new(input);

    'tokens: while input.has_remaining() {
        let map = input.get_u8();
        for i in 0..8 {
            if !input.has_remaining() {
                break;
            }

            if ((map >> i) & 1) == 0 {
                /* literal value */
                output.push(input.get_u8());
            } else {
                /* back seek */
                // Native a03e4..a03e8 stops before reading a partial final
                // reference. Preserve the bytes already decoded.
                if input.remaining() < 2 {
                    break 'tokens;
                }
                let backseek_spec = input.get_u16(); // big endian Oo

                /*  MSB  XXXXXXXX          YYYYYYYY    LSB
                    val  len               backOffset
                    size (16-OFFSET_BITS)  OFFSET_BITS
                */

                let back_offset_mask = (1 << OFFSET_BITS) - 1; // magic to get the last OFFSET_BITS bits

                let len = (backseek_spec >> OFFSET_BITS) + 3;
                let back_offset = (backseek_spec & back_offset_mask) + 1;

                for _ in 0..len {
                    let last = output.len() - back_offset as usize;
                    // TODO: make this fallible?
                    // TODO: this might be optimized by stopping the bounds checking after we have enough data to guarantee that it's in bounds
                    output.push(output[last]);
                }
            }
        }
    }
    #[cfg(test)]
    reference::compare_native_lz77_if_enabled::<OFFSET_BITS>(
        encoded_input,
        &output[output_begin..],
    );
}
