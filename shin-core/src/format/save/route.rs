use shin_primitives::bit_utils::{BitBuffer, BitReadError};

use super::RouteData;

/// The native route stream records values that cannot be reproduced from the
/// initial script registers and random seed: SGET, SELECT, and QUIZ responses.
#[derive(Debug, Default, Clone)]
pub struct RouteRecorder {
    data: RouteData,
    position: usize,
}

impl RouteRecorder {
    fn write(&mut self, f: impl FnOnce(&mut BitBuffer)) {
        let mut buffer = BitBuffer::with_data(std::mem::take(&mut self.data.0), self.position);
        f(&mut buffer);
        (self.data.0, self.position) = buffer.into_data();
    }

    /// Native 0x34940: zero and one use two bits; all other values use a
    /// zero tag followed by the low signed sixteen bits.
    pub fn record_persistent(&mut self, value: i32) {
        self.write(|buffer| {
            if matches!(value, 0 | 1) {
                buffer.write_bool(true);
                buffer.write_bool(value != 0);
            } else {
                buffer.write_bool(false);
                buffer.write_int(16, value as u16);
            }
        });
    }

    /// Native 0x349b0. Values are truncated like the original bit writer.
    pub fn record_choice(&mut self, value: i32) {
        self.write(|buffer| buffer.write_int(4, (value as u8) & 15));
    }

    /// Native 0x349d0.
    pub fn record_quiz(&mut self, value: i32) {
        self.write(|buffer| buffer.write_int(6, (value as u8) & 63));
    }

    pub fn snapshot(&self) -> RouteData {
        self.data.clone()
    }

    pub fn bit_position(&self) -> usize {
        self.position
    }
}

impl RouteData {
    pub fn reader(&self) -> RouteReader {
        RouteReader {
            buffer: BitBuffer::with_data(self.0.clone(), 0),
        }
    }
}

/// Reads the saved route in the same command order during reconstruction.
pub struct RouteReader {
    buffer: BitBuffer,
}

impl RouteReader {
    /// Continue recording at the consumed position without re-encoding values.
    /// In particular, a long-form saved zero must remain a seventeen-bit value.
    pub fn into_recorder(self) -> RouteRecorder {
        let (data, position) = self.buffer.into_data();
        RouteRecorder {
            data: RouteData(data),
            position,
        }
    }

    pub fn read_persistent(&mut self) -> Result<i32, BitReadError> {
        // Native 0x348f0 sign-extends the long form via 0x343b0.
        if self.buffer.read_bool()? {
            Ok(i32::from(self.buffer.read_bool()?))
        } else {
            Ok(self.buffer.read_int::<u16>(16)? as i16 as i32)
        }
    }

    pub fn read_choice(&mut self) -> Result<i32, BitReadError> {
        self.buffer.read_int(4)
    }

    pub fn read_quiz(&mut self) -> Result<i32, BitReadError> {
        self.buffer.read_int(6)
    }

    pub fn bit_position(&self) -> usize {
        self.buffer.bit_position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_native_stream_preserves_unaligned_values_and_snapshot() {
        let mut recorder = RouteRecorder::default();
        recorder.record_persistent(0); // 10
        recorder.record_persistent(1); // 11
        recorder.record_choice(12); // 1100
        recorder.record_quiz(48); // 110000
        recorder.record_persistent(-32768); // 0 + 1000000000000000
        let snapshot = recorder.snapshot();
        assert_eq!(recorder.bit_position(), 31);
        assert_eq!(&snapshot.0[..4], &[0xbc, 0xc1, 0, 0]);
        assert_eq!(snapshot.0.len(), 1024);
        recorder.record_quiz(127);
        let mut reader = snapshot.reader();
        assert_eq!(reader.read_persistent().unwrap(), 0);
        assert_eq!(reader.read_persistent().unwrap(), 1);
        assert_eq!(reader.read_choice().unwrap(), 12);
        assert_eq!(reader.read_quiz().unwrap(), 48);
        assert_eq!(reader.read_persistent().unwrap(), -32768);
        assert_eq!(reader.bit_position(), 31);
        // Subsequent recording does not mutate an existing save snapshot.
        assert_eq!(&snapshot.0[..4], &[0xbc, 0xc1, 0, 0]);
    }

    #[test]
    fn persistent_long_form_truncates_and_sign_extends_like_native() {
        let values = [
            2,
            32767,
            32768,
            65535,
            65536,
            -1,
            -32769,
            i32::MIN,
            i32::MAX,
        ];
        let mut recorder = RouteRecorder::default();
        for value in values {
            recorder.record_persistent(value);
        }
        assert_eq!(recorder.bit_position(), values.len() * 17);
        let mut reader = recorder.snapshot().reader();
        for value in values {
            assert_eq!(reader.read_persistent().unwrap(), value as i16 as i32);
        }
        let mut reader = RouteData(vec![]).reader();
        assert!(reader.read_persistent().is_err());
        assert!(reader.read_choice().is_err());
        assert!(reader.read_quiz().is_err());
    }

    #[test]
    fn resuming_recording_preserves_long_form_zero_and_unaligned_cursor() {
        let mut original = RouteRecorder::default();
        original.record_persistent(65536); // Long form, saved as zero.
        original.record_choice(12);
        let mut reader = original.snapshot().reader();
        assert_eq!(reader.read_persistent().unwrap(), 0);
        assert_eq!(reader.read_choice().unwrap(), 12);
        let mut resumed = reader.into_recorder();
        assert_eq!(resumed.bit_position(), 21);
        resumed.record_quiz(27);
        let mut reader = resumed.snapshot().reader();
        assert_eq!(reader.read_persistent().unwrap(), 0);
        assert_eq!(reader.read_choice().unwrap(), 12);
        assert_eq!(reader.read_quiz().unwrap(), 27);
        assert_eq!(reader.bit_position(), 27);
    }

    #[test]
    #[ignore = "requires supplied Switch executable"]
    fn reference_route_encoding_matches_native_command_handlers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("umineko_switch_files/uncompressed_executable");
        let executable = std::fs::read(path).unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap())
        };
        let branch_target = |address: usize| {
            let instruction = word(address);
            assert_eq!(instruction >> 26, 0x25); // BL immediate
            let displacement = ((instruction << 6) as i32 >> 4) as isize;
            (address as isize + displacement) as usize
        };
        for (call, target) in [
            (0x4dd4, 0x34940),
            (0x5f80, 0x349b0),
            (0xe21c, 0x349d0),
            (0x4d08, 0x348f0),
            (0x5b34, 0x349a0),
            (0xdf9c, 0x349c0),
        ] {
            assert_eq!(branch_target(call), target);
        }
        for (address, width) in [
            (0x34990, 16),
            (0x349b8, 4),
            (0x349d8, 6),
            (0x349a4, 4),
            (0x349c4, 6),
        ] {
            let instruction = word(address);
            let decoded = if instruction & 0xffe0001f == 0x52800001 {
                (instruction >> 5) & 0xffff // MOVZ w1
            } else {
                assert_eq!(instruction & 0xff8003ff, 0x320003e1); // ORR w1, wzr
                let mask_bits = ((instruction >> 10) & 63) + 1;
                assert!(mask_bits < 32); // These constants use full 32-bit masks.
                ((1u32 << mask_bits) - 1).rotate_right((instruction >> 16) & 63)
            };
            assert_eq!(decoded, width);
        }
        assert_eq!(word(0x34958), 0x7100043f); // CMP w1, #1
        assert_eq!(word(0x3495c) & 0xff00001f, 0x54000008); // B.HI long form
        assert_eq!(word(0x343d0), 0x1ac82920); // ASR w0, w9, w8 sign extension
        assert_eq!((word(0x344cc) >> 10) & 0xfff, 0x3ff); // 1024-byte growth
    }
}
