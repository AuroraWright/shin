//! Blink timing recovered from native BustupLayer0x35c50/0x36b80.
use shin_render::shaders::types::RenderClone;

#[derive(Clone, Debug, RenderClone)]
pub(super) struct BustupBlink {
    seed: u32,
    elapsed: f32,
    wait: f32,
    sequence: Vec<u32>,
}

impl BustupBlink {
    #[cfg(test)]
    pub(crate) fn reference_state(&self) -> (u32, u32, u32, Vec<u32>) {
        (
            self.seed,
            self.elapsed.to_bits(),
            self.wait.to_bits(),
            self.sequence.clone(),
        )
    }
    pub fn new(count: usize, seed: u32) -> Self {
        // Native constructor builds0..N-1 followed byN-2..0.
        let sequence = (0..count)
            .chain((0..count.saturating_sub(1)).rev())
            .map(|frame| frame as u32)
            .collect();
        let mut blink = Self {
            seed: (seed % 2_147_483_647).max(1),
            elapsed: 0.0,
            wait: 0.0,
            sequence,
        };
        blink.next_wait();
        blink
    }

    fn next_wait(&mut self) {
        // Schrage form of MINSTD48271,m2147483647,q44488,r3399.
        let quotient = self.seed / 44_488;
        let value = i64::from(self.seed % 44_488) * 48_271 - i64::from(quotient) * 3_399;
        self.seed = if value < 0 {
            (value + 2_147_483_647) as u32
        } else {
            value as u32
        };
        let unit = (self.seed - 1) as f32 * f32::from_bits(0x30000000);
        self.wait = (1.0 - unit * unit) * 240.0 + 80.0;
    }

    pub fn update(&mut self, ticks: f32, enabled: bool) -> u32 {
        if !enabled {
            self.elapsed = 0.0;
            return 0;
        }
        self.elapsed += ticks;
        if self.sequence.is_empty() || self.elapsed < self.wait {
            return 0;
        }
        if self.elapsed >= self.wait + 20.0 {
            // Native discards overshoot and schedules one new wait per update.
            self.elapsed = 0.0;
            self.next_wait();
            return 0;
        }
        let phase =
            ((self.elapsed - self.wait) / 20.0 * self.sequence.len() as f32).floor() as usize;
        self.sequence.get(phase).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied Umineko ROM and executable"]
    fn reference_native_blink_and_supplied_eye_descriptors() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
        };
        use std::{collections::BTreeSet, io::Read};
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, word) in [
            (0x359c8, 0x12800017),
            (0x359ec, 0x110006e8),
            (0x35ac8, 0x510006f7),
            (0x35ae8, 0x54fffeec),
            (0x35c50, 0x9403179c),
            (0x35c78, 0x2b080008),
            (0x35c7c, 0x1a9f1508),
            (0x35c80, 0x9ba97d09),
            (0x35c90, 0x529791ea),
            (0x35c98, 0x5281a8ea),
            (0x35ca0, 0x6b090108),
            (0x35ca8, 0x1a9f3129),
            (0x35cb4, 0x1e230120),
            (0x35cd4, 0x1e200803),
            (0x35ce4, 0x1e210800),
            (0x35cf0, 0xbd00fa60),
            (0x36b80, 0xbd40f660),
            (0x36b84, 0x1e282800),
            (0x36b94, 0x54000280),
            (0x36b9c, 0x1e269002),
            (0x36ba8, 0x5400028a),
            (0x36bb0, 0x540001ab),
            (0x36bb8, 0x1e221800),
            (0x36bcc, 0x1e210800),
            (0x36bd0, 0x1e30000a),
            (0x36bdc, 0xb86ad929),
            (0x36bec, 0xf900767f),
            (0x36bf0, 0xb900f67f),
            (0x36c04, 0x9b0b7d4b),
            (0x36c30, 0x0b0a016a),
            (0x36c34, 0x291e7e6a),
            (0x36c60, 0x1e200803),
            (0x36c7c, 0xbd00fa60),
            (0x36edc, 0xb980ee89),
            (0x36ee0, 0xf8697908),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                word,
                "native blink{address:x}"
            );
        }
        for (address, bits) in [
            (0x1594c4, 0x30000000),
            (0x159660, 0x43700000),
            (0x159a68, 0x42a00000),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                bits,
                "native blink constant{address:x}"
            );
        }
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap())
                .unwrap();
        let entry = rom.find_file("/main.snr").unwrap();
        let mut data = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let scenario = Scenario::new(data.into()).unwrap();
        let paths: BTreeSet<_> = scenario
            .info_tables()
            .bustup_info
            .iter()
            .map(|info| info.path())
            .collect();
        let mut expressions = 0;
        let mut with_eyes = 0;
        for path in &paths {
            let entry = rom.find_file(path).unwrap();
            let mut data = Vec::new();
            rom.open_file(entry)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            assert_eq!(&data[..4], b"BUP4");
            let word = |a| u32::from_le_bytes(data[a..a + 4].try_into().unwrap());
            assert_eq!(word(4), 4);
            let mut cursor = word(44) as usize;
            let count = word(cursor);
            cursor += 4;
            for _ in 0..count {
                let size = word(cursor) as usize;
                assert!(size >= 32 && cursor + size <= data.len());
                let eyes = u16::from_le_bytes(data[cursor + 30..cursor + 32].try_into().unwrap());
                expressions += 1;
                if eyes != 0 {
                    with_eyes += 1;
                    println!("Authored eye animation:{path},frames{eyes}");
                }
                cursor += size;
            }
        }
        println!(
            "Supplied bustup descriptor scan:{} archives,{} expressions,{} with eye frames",
            paths.len(),
            expressions,
            with_eyes
        );
        assert_eq!(
            with_eyes, 0,
            "Supplied Umineko reference is expected to omit eye blocks"
        );
    }

    #[test]
    fn blink_random_recurrence_matches_modular_product_for_boundary_seeds() {
        for seed in [
            0,
            1,
            44_488,
            2_147_483_646,
            2_147_483_647,
            2_147_483_648,
            u32::MAX,
        ] {
            let mut expected = (seed % 2_147_483_647).max(1);
            let mut blink = BustupBlink::new(3, seed);
            for index in 0..20 {
                expected = (u64::from(expected) * 48_271 % 2_147_483_647) as u32;
                if index != 0 {
                    blink.next_wait();
                }
                assert_eq!(blink.seed, expected, "initialseed{seed},step{index}");
                assert!((80.0..=320.0).contains(&blink.wait));
            }
        }
    }

    #[test]
    fn blink_uses_native_round_trip_sequence_and_twenty_tick_duration() {
        let mut blink = BustupBlink::new(3, 1);
        assert_eq!(blink.sequence, [0, 1, 2, 1, 0]);
        assert_eq!(blink.seed, 48271);
        assert_eq!(blink.wait, 320.0);
        assert_eq!(blink.update(319.5, true), 0);
        assert_eq!(blink.update(0.5, true), 0);
        assert_eq!(blink.update(3.5, true), 0);
        assert_eq!(blink.update(0.5, true), 1);
        assert_eq!(blink.update(4.0, true), 2);
        assert_eq!(blink.update(4.0, true), 1);
        assert_eq!(blink.update(4.0, true), 0);
        assert_eq!(blink.update(4.0, true), 0);
        assert_eq!(blink.seed, 182605794);
        assert_eq!(blink.elapsed, 0.0);
        assert!((80.0..=320.0).contains(&blink.wait));
    }
    #[test]
    fn disabled_blink_resets_clock_without_consuming_randomness_or_rescheduling() {
        let mut blink = BustupBlink::new(4, 42);
        blink.update(blink.wait + 10.0, true);
        let (seed, wait) = (blink.seed, blink.wait);
        assert_eq!(blink.update(1000.0, false), 0);
        assert_eq!(blink.elapsed, 0.0);
        assert_eq!((blink.seed, blink.wait), (seed, wait));
        assert_eq!(blink.update(wait - 1.0, true), 0);
        assert_eq!(blink.update(10000.0, true), 0);
        assert_eq!(blink.elapsed, 0.0);
        let mut expected = BustupBlink::new(4, 42);
        expected.next_wait();
        assert_eq!(blink.seed, expected.seed);
    }
    #[test]
    fn absent_eye_frames_do_not_consume_randomness_and_clone_preserves_phase() {
        let mut empty = BustupBlink::new(0, 1);
        let seed = empty.seed;
        assert_eq!(empty.update(10000.0, true), 0);
        assert_eq!(empty.seed, seed);
        let mut blink = BustupBlink::new(2, 123);
        blink.update(blink.wait + 7.5, true);
        let mut cloned = blink.clone();
        for dt in [0.5, 4.0, 1000.0, 2.25] {
            assert_eq!(blink.update(dt, true), cloned.update(dt, true));
            assert_eq!(
                (blink.seed, blink.elapsed, blink.wait),
                (cloned.seed, cloned.elapsed, cloned.wait)
            );
        }
    }
}
