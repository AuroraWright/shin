use super::*;
use crate::adv::higurashi::hidden_window::{ConcealServices, ConcealState};
use std::{
    cell::RefCell,
    io::Write,
    process::{Command, Stdio},
    rc::Rc,
};

struct Owners {
    message: bool,
    indicator: bool,
    menu: bool,
    overlay: bool,
    voice: bool,
    effects: bool,
    events: Rc<RefCell<Vec<u32>>>,
    edges: Vec<[u32; 4]>,
}
impl ConcealServices for Owners {
    fn hide_message(&mut self, hidden: bool) {
        self.message = hidden;
    }
    fn hide_indicator(&mut self, hidden: bool) {
        self.indicator = hidden;
    }
    fn hide_menu(&mut self, hidden: bool) {
        self.menu = hidden;
    }
    fn hide_overlay(&mut self, hidden: bool) {
        if self.overlay {
            self.edges.push([
                hidden as u32,
                self.message as u32,
                self.indicator as u32,
                self.menu as u32,
            ]);
        }
    }
}
struct Services {
    conceal: ConcealState,
    owners: Owners,
}
impl ModalServices for Services {
    fn conceal(&mut self, raw: u32) {
        self.conceal.control(raw, &mut self.owners);
    }
    fn pause_voice(&mut self, paused: bool) {
        if self.owners.voice != paused {
            self.owners
                .events
                .borrow_mut()
                .extend([14 + paused as u32, 16 + paused as u32]);
        }
        self.owners.voice = paused;
    }
    fn pause_effects(&mut self, paused: bool) {
        if self.owners.effects != paused {
            self.owners
                .events
                .borrow_mut()
                .extend([24 + paused as u32, 26 + paused as u32]);
        }
        self.owners.effects = paused;
    }
}
fn snapshot(op: char, state: &ModalState, services: &mut Services) -> String {
    let owners = &mut services.owners;
    let events = owners.events.take();
    let edges = std::mem::take(&mut owners.edges);
    let mut s = format!(
        "O {op} {} {} {} {} {} {} {} {} {} {} {} {}",
        state.pause_depth,
        services.conceal.depth,
        state.animations_enabled as u32,
        owners.voice as u32,
        owners.effects as u32,
        state.fade.value().to_bits(),
        state.fade.is_waiting() as u32,
        owners.message as u32,
        owners.indicator as u32,
        owners.menu as u32,
        events.len(),
        edges.len()
    );
    for v in events {
        s += &format!(" {v}");
    }
    for edge in edges {
        for v in edge {
            s += &format!(" {v}");
        }
    }
    s
}

#[test]
#[ignore = "executes original modal/conceal/audio-scope/tween functions from updated AArch64 image"]
fn higurashi_modal_scope_matches_native_counts_audio_and_fades() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut sequences = 0;
    for pause_depth in [0, 1, 2, u32::MAX] {
        for hide in [0, 1, u32::MAX] {
            for flags in 0..16 {
                for fade in [0.0f32, 0.37, 1.0] {
                    let bit = |i| flags & (1 << i) != 0;
                    let mut state = ModalState {
                        pause_depth,
                        animations_enabled: bit(0),
                        fade: HigurashiTweener::new(fade),
                    };
                    let events = Rc::new(RefCell::new(Vec::new()));
                    let mut services = Services {
                        conceal: ConcealState { depth: hide },
                        owners: Owners {
                            message: hide != 0,
                            indicator: hide != 0,
                            menu: hide != 0,
                            voice: bit(1),
                            effects: bit(2),
                            overlay: bit(3),
                            events: events.clone(),
                            edges: Vec::new(),
                        },
                    };
                    input += &format!(
                        "N {pause_depth} {hide} {} {} {} {:x} {}\n",
                        bit(0) as u32,
                        bit(1) as u32,
                        bit(2) as u32,
                        fade.to_bits(),
                        bit(3) as u32
                    );
                    expected.push(snapshot('N', &state, &mut services));
                    for (op, value) in [
                        ('E', 0.0),
                        ('U', 0.5),
                        ('E', 0.0),
                        ('U', 3.0),
                        ('X', 0.0),
                        ('H', 1.0),
                        ('U', 10.0),
                        ('X', 0.0),
                        ('H', 0.0),
                        ('U', 20.0),
                        ('X', 0.0),
                        ('U', 0.0),
                        ('E', 0.0),
                        ('U', 1.0),
                        ('X', 0.0),
                        ('U', 10.0),
                    ] {
                        match op {
                            'E' => {
                                input += "E\n";
                                state
                                    .enter(
                                        || {
                                            events.borrow_mut().push(1);
                                            Ok(())
                                        },
                                        &mut services,
                                    )
                                    .unwrap();
                            }
                            'X' => {
                                input += "X\n";
                                state.exit(&mut services);
                            }
                            'H' => {
                                input += &format!("H {}\n", value as u32);
                                services.conceal(value as u32);
                            }
                            'U' => {
                                input += &format!("U {:x}\n", (value as f32).to_bits());
                                state.advance(Ticks::from_f32(value));
                            }
                            _ => unreachable!(),
                        }
                        expected.push(snapshot(op, &state, &mut services));
                    }
                    // Desktop capture failure must not acquire any scope or replace clocks.
                    let old = snapshot('F', &state, &mut services);
                    assert!(
                        state
                            .enter(|| anyhow::bail!("capture failed"), &mut services)
                            .is_err()
                    );
                    assert_eq!(snapshot('F', &state, &mut services), old);
                    input += "D\n";
                    sequences += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MODAL_SCOPE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    let actual: Vec<_> = output.lines().collect();
    assert_eq!(actual.len(), expected.len());
    for (index, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "observation{index}");
    }
    println!(
        "Higurashi modal scope:{sequences} native timelines/{} exact observations; nested/wrapping pause and conceal, animation, active audio filtering/order, interrupted fades and capture failure pass",
        expected.len()
    );
}
