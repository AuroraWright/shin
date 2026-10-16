//! Standalone comparison transport. Parent services are checked by the C probe.
#[path = "../chapter.rs"]
mod chapter;
#[path = "../../cg_gallery/motion.rs"]
mod motion;
use chapter::{Controller, Event, Input, Phase};
use motion::Motion;
use std::io::{self, BufRead};

struct Browser {
    controller: Controller,
    selected: usize,
    first: usize,
    last: usize,
    scroll: Motion,
    target: f32,
    jump: bool,
}
impl Browser {
    fn apply(&mut self, events: Vec<Event>) -> [u32; 8] {
        let mut counts = [0; 8];
        for event in events {
            match event {
                Event::ParentSelect { row } => {
                    self.selected = row;
                    let top = (row as i32 - 2) as f32 * 248.;
                    let old = self.scroll.value();
                    self.target = if top - old < 0. {
                        top.max(-496.)
                    } else if top - old + 240. > 832. {
                        (top + 240. - 832.).min(3464. - 832.).max(-496.)
                    } else {
                        old
                    };
                    self.scroll.target(self.target, 4., 0, false);
                }
                Event::DisableInput => counts[0] += 1,
                Event::Back => counts[1] += 1,
                Event::Move => counts[2] += 1,
                Event::Decide => counts[3] += 1,
                Event::ReplayVoice { row } => {
                    assert_eq!(row, self.selected);
                    counts[4] += 1;
                }
                Event::QuickLoad { row } => {
                    assert_eq!(row, self.selected);
                    counts[5] += 1;
                }
                Event::CloseBrowser => counts[6] += 1,
                Event::Jump { row } => {
                    assert_eq!(row, self.selected);
                    self.jump = true;
                    counts[7] += 1;
                }
                Event::CloseFlash { .. } => {}
            }
        }
        counts
    }
    fn print(&self, op: &str, result: u32, counts: [u32; 8]) {
        let c = &self.controller;
        let phase = match c.phase() {
            Phase::Opening => 0,
            Phase::Active => 1,
            Phase::Closing => 2,
            Phase::Finished => u32::MAX,
        };
        let values = [
            result,
            phase,
            c.progress().to_bits(),
            c.selected() as u32,
            c.scroll().to_bits(),
            c.scroll_target().to_bits(),
            c.hover().map_or(u32::MAX, |i| i as u32),
            u32::from(c.dragging()),
            c.velocity().to_bits(),
            self.selected as u32,
            self.scroll.value().to_bits(),
            self.target.to_bits(),
            c.parent_target() as u32,
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            counts[5],
            counts[6],
            counts[7],
            u32::from(self.jump),
        ];
        print!("{op}");
        for value in values {
            print!(" {value}");
        }
        println!();
    }
}
fn main() {
    let mut browser: Option<Browser> = None;
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut args = line.split_whitespace();
        let op = args.next().unwrap();
        let words: Vec<_> = args.collect();
        let u = |i: usize| words[i].parse::<u32>().unwrap();
        let f = |i: usize| words[i].parse::<f32>().unwrap();
        let bits = |i: usize| f32::from_bits(u32::from_str_radix(words[i], 16).unwrap());
        if op == "X" {
            assert!(browser.take().is_some());
            println!("X");
            continue;
        }
        if op == "N" {
            let markers: Vec<_> = (0..16).filter(|i| u(2) >> i & 1 != 0).collect();
            browser = Some(Browser {
                controller: Controller::new(16, &markers, u(0) as usize, u(1) as usize).unwrap(),
                selected: u(0) as usize,
                first: u(3) as usize,
                last: u(4) as usize,
                scroll: Motion::new(0.),
                target: 0.,
                jump: false,
            });
            browser.as_ref().unwrap().print(op, 0, [0; 8]);
            continue;
        }
        let b = browser.as_mut().unwrap();
        let mut result = 0;
        if op == "I" {
            print!("I");
            for entry in b.controller.entries() {
                print!(" {} {} {}", entry.kind, entry.marker_index, entry.row);
            }
            println!();
            continue;
        }
        let events = match op {
            "F" => b.controller.update(
                bits(0),
                Input {
                    permission: u(1) != 0,
                    captured: u(2) != 0,
                    pressed: u32::from_str_radix(words[3], 16).unwrap(),
                    repeated: u32::from_str_radix(words[4], 16).unwrap(),
                    axis: bits(5),
                    parent_close_busy: u(6) != 0,
                    first_row: b.first,
                    last_row: b.last,
                },
            ),
            "P" => {
                result = u32::from(b.controller.probe(f(0), f(1)));
                vec![]
            }
            "Z" => {
                b.controller.reset();
                vec![]
            }
            "D" => b.controller.press(),
            "U" => b.controller.release(),
            "T" => b.controller.drag([f(0), f(1)], [f(2), f(3)]),
            "G" => b.controller.fling([f(0), f(1)]),
            "C" => b.controller.cancel(),
            "Q" => b.controller.jump_response(u(0) == 1, u(1) as i32),
            _ => panic!("unknown {op}"),
        };
        let counts = b.apply(events);
        if op == "F" {
            b.scroll.update(1.);
        }
        b.print(op, result, counts);
    }
    assert!(browser.is_none());
}
