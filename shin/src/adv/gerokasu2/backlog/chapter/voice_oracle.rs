//! Compile the production Sequence against the checked original voice owner.
mod history {
    pub mod layout {
        #[derive(Clone, Debug, PartialEq)]
        pub struct Voice {
            pub path: String,
            pub volume: f32,
        }
    }
}
#[path = "../voice_queue.rs"]
mod voice_queue;
use history::layout::Voice;
use std::io::{self, BufRead};
fn main() {
    let mut sequence = voice_queue::Sequence::default();
    let (mut status, mut last, mut volume, mut tick) = (0, 0, 0, 0);
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let args: Vec<_> = line.split_whitespace().collect();
        let (mut stops, mut updates, mut plays) = (0, 0, 0);
        match args[0] {
            "R" => {
                let count: usize = args[1].parse().unwrap();
                sequence.restart(
                    (0..count)
                        .map(|i| Voice {
                            path: ((b'A' + i as u8) as char).to_string(),
                            volume: (i + 1) as f32 * 0.25,
                        })
                        .collect(),
                );
                stops = 1;
            }
            "S" => {
                sequence.stop();
                stops = 1;
            }
            "F" => {
                status = args[1].parse::<u32>().unwrap();
                tick = u32::from_str_radix(args[2], 16).unwrap();
                assert_eq!(tick, 1f32.to_bits());
                if let Some(voice) = sequence.advance(status & 2 != 0) {
                    plays = 1;
                    last = (voice.path.as_bytes()[0] - b'A') as u32;
                    volume = voice.volume.to_bits();
                }
                updates = 1;
            }
            _ => panic!("invalid voice op"),
        }
        println!(
            "{} {} {} {} {} {} {} {} {} {} {}",
            args[0],
            u32::from(sequence.active()),
            sequence.index(),
            sequence.elapsed().to_bits(),
            stops,
            updates,
            plays,
            last,
            volume,
            tick,
            u32::from(status & 2 != 0)
        );
    }
}
