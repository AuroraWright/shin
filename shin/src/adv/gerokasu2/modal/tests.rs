use super::*;
use std::{cell::RefCell, process::Command, rc::Rc};
struct Owners {
    overlay: bool,
    events: Rc<RefCell<String>>,
}
impl Owners {
    fn event(&mut self, hide: bool, yes: char, no: char) {
        self.events.borrow_mut().push(if hide { yes } else { no });
    }
}
impl Services for Owners {
    fn hide_message(&mut self, b: bool) {
        self.event(b, 'M', 'm')
    }
    fn hide_overlay(&mut self, b: bool) {
        if self.overlay {
            self.event(b, 'O', 'o')
        }
    }
    fn hide_indicator(&mut self, b: bool) {
        self.event(b, 'I', 'i')
    }
    fn hide_menu(&mut self, b: bool) {
        self.event(b, 'N', 'n')
    }
    fn pause_voice(&mut self, b: bool) {
        self.event(b, 'V', 'v')
    }
    fn pause_effects(&mut self, b: bool) {
        self.event(b, 'E', 'e')
    }
}
fn snapshot(op: char, m: &Modal, events: &Rc<RefCell<String>>) -> String {
    let e = std::mem::take(&mut *events.borrow_mut());
    format!(
        "M {op} {} {} {} {} {} {}",
        m.pause_depth,
        m.conceal_depth,
        u8::from(m.animations_enabled),
        m.fade.value().to_bits(),
        u8::from(m.fade.busy()),
        if e.is_empty() { "-" } else { &e }
    )
}
#[test]
#[ignore = "executes original Ger modal/conceal/tween functions; no audio or GPU device"]
fn ger_modal_scope_matches_native_nested_pause_conceal_order_and_fades() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let out = root.join("reverse-engineering/gerokasu2/modal");
    std::fs::create_dir_all(&out).unwrap();
    let binary = out.join("native-modal");
    assert!(
        Command::new("clang")
            .args(["-std=c11", "-O2"])
            .arg(root.join("gerokasu2/native-modal.c"))
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for pause in [0, 1, 2, u32::MAX] {
        for hide in [0, 1, u32::MAX] {
            for animations in [false, true] {
                for fade in [0.0f32, 0.37, 1.0] {
                    for overlay in [false, true] {
                        profiles += 1;
                        let events = Rc::new(RefCell::new(String::new()));
                        let mut owners = Owners {
                            overlay,
                            events: events.clone(),
                        };
                        let mut m = Modal {
                            pause_depth: pause,
                            conceal_depth: hide,
                            animations_enabled: animations,
                            fade: Motion::new(fade),
                        };
                        input += &format!(
                            "N {pause} {hide} {} {:x} {}\n",
                            u8::from(animations),
                            fade.to_bits(),
                            u8::from(overlay)
                        );
                        expected.push(snapshot('N', &m, &events));
                        for (op, value) in [
                            ('E', 0.),
                            ('U', 0.5),
                            ('E', 0.),
                            ('U', 3.),
                            ('X', 0.),
                            ('H', 1.),
                            ('U', 10.),
                            ('X', 0.),
                            ('H', 0.),
                            ('U', 20.),
                            ('X', 0.),
                            ('U', 0.),
                            ('E', 0.),
                            ('U', 1.),
                            ('X', 0.),
                            ('U', 10.),
                        ] {
                            match op {
                                'E' => {
                                    input += "E\n";
                                    let e = events.clone();
                                    m.enter(
                                        || {
                                            e.borrow_mut().push('C');
                                            Ok(())
                                        },
                                        &mut owners,
                                    )
                                    .unwrap();
                                }
                                'X' => {
                                    input += "X\n";
                                    m.exit(&mut owners);
                                }
                                'H' => {
                                    input += &format!("H {}\n", value as u8);
                                    m.conceal(value != 0., &mut owners);
                                }
                                'U' => {
                                    input += &format!("U {:x}\n", (value as f32).to_bits());
                                    m.advance(value);
                                }
                                _ => unreachable!(),
                            }
                            expected.push(snapshot(op, &m, &events));
                        }
                        let old = snapshot('F', &m, &events);
                        assert!(
                            m.enter(|| anyhow::bail!("capture preparation failed"), &mut owners)
                                .is_err()
                        );
                        assert_eq!(snapshot('F', &m, &events), old);
                        input += "D\n";
                    }
                }
            }
        }
    }
    let stdin = out.join("native-modal.input");
    std::fs::write(&stdin, &input).unwrap();
    let native = Command::new(&binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(std::fs::File::open(&stdin).unwrap())
        .output()
        .unwrap();
    std::fs::write(out.join("native-modal.output"), &native.stdout).unwrap();
    std::fs::write(out.join("native-modal.stderr"), &native.stderr).unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let actual = String::from_utf8(native.stdout).unwrap();
    let lines: Vec<_> = actual.lines().collect();
    assert_eq!(lines.len(), expected.len());
    for (i, (a, e)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "modal observation{i}");
    }
    println!(
        "GER_MODAL_RUST profiles={profiles} observations={} original_counts_order_pause_conceal_fades=exact",
        expected.len()
    );
}
