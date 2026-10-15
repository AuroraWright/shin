//! Originalb74c captures physical slots/banks; b924 checks their current owners.
use super::*;

pub(in crate::adv::higurashi) struct MovieWait {
    targets: Vec<Coordinate>,
    mode: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Coordinate {
    slot: u8,
    bank: u8,
}
impl MovieWait {
    pub fn capture(layers: &HigurashiLayers, selector: i32, mode: i32) -> Result<Self> {
        // -1/-3 are constructor no-ops. The authored -4 selector captures the
        // selected callbacks; subsequent selector/bank edits cannot retarget it.
        let targets = if selector == -2 {
            // Literal20c48(-2) reads the valid allocator count's high byte,
            // which is0 for counts0..32. b830 captures selected.to's LOW byte.
            // Keep that raw bank until16184's nonzero→back lookup below.
            vec![Coordinate {
                slot: 0,
                bank: layers.state.selected_native()[1] as u8,
            }]
        } else {
            layers
                .state
                .targets(selector)?
                .into_iter()
                .map(|target| Coordinate {
                    slot: target.slot,
                    bank: target.bank,
                })
                .collect()
        };
        Ok(Self { targets, mode })
    }
    pub fn complete(&self, layers: &HigurashiLayers) -> bool {
        self.targets.iter().all(|target| {
            let owner = layers.owners[target.slot as usize][usize::from(target.bank != 0)].as_ref();
            if !matches!(self.mode, 1 | 2) {
                return true;
            }
            !crate::layer::user::higurashi::movie::wait_pending(
                self.mode,
                owner.and_then(HigurashiUserLayer::movie_status),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "frozen original MOVIEWAIT -2 capture corpus; no GPU/audio/window/save"]
    fn higurashi_movie_wait_minus_two_matches_original_captured_coordinates() {
        let path = std::env::var("SHIN_HIGURASHI_MOVIEWAIT_MINUS_TWO_CAPTURE").unwrap();
        let capture = std::fs::read_to_string(path).unwrap();
        let mut checked = 0;
        for line in capture.lines() {
            let words: Vec<_> = line.split_whitespace().collect();
            assert_eq!(words[0], "W");
            let profile: usize = words[1].parse().unwrap();
            let mode: i32 = words[3].parse().unwrap();
            let end: i32 = words[4].parse().unwrap();
            let mut layers = HigurashiLayers::new(|| [0; 4]);
            for id in 0..[0, 1, 32][(profile - 1) / 24] {
                layers.state.allocate(id * 2).unwrap();
            }
            layers.state.select(0, end);
            let wait = MovieWait::capture(&layers, -2, mode).unwrap();
            layers.state.select(0, end ^ 1);
            assert_eq!(wait.targets.len(), 1);
            let tail = words.iter().position(|word| *word == "R").unwrap();
            assert_eq!(wait.targets[0].slot, words[tail + 6].parse::<u8>().unwrap());
            assert_eq!(wait.targets[0].bank, words[tail + 7].parse::<u8>().unwrap());
            // Native bank normalization, after the retained raw capture.
            assert_eq!(
                usize::from(wait.targets[0].bank != 0),
                words[tail - 1].parse::<usize>().unwrap()
            );
            assert!(wait.complete(&layers)); // Both actual scene banks empty.
            checked += 1;
        }
        assert_eq!(checked, 432);
        eprintln!(
            "MOVIEWAIT -2:72 original constructors/432 retained coordinate snapshots, valid allocated0/1/32, selected.to low-byte aliases and16184 bank normalization; current Movie status covered by live-owner check"
        );
    }
}
