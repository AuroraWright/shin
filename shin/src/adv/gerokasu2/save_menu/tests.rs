use super::*;
fn available() -> [bool; 100] {
    std::array::from_fn(|i| i % 3 != 0)
}
fn active(slot: u8) -> Controller {
    let mut m = Controller::new(slot, available()).unwrap();
    m.assets_ready();
    m.update(10.0);
    m
}
#[test]
fn load_requires_valid_slot_final_yes_and_completed_close() {
    let mut m = active(0);
    m.input(Input {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(m.phase(), Phase::Active);
    assert_eq!(m.take_sounds(), ["error"]);
    assert!(m.take_confirmation().is_none());
    let mut m = active(1);
    m.input(Input {
        confirm: true,
        ..Default::default()
    });
    assert_eq!(m.phase(), Phase::RowFlash);
    m.update(7.0);
    assert!(m.take_confirmation().is_none());
    m.update(0.0);
    assert_eq!(m.take_confirmation(), Some(1));
    m.dialog_response(false, 0);
    assert_eq!(m.phase(), Phase::Confirmation);
    assert!(m.take_storage_requests().is_empty());
    m.dialog_response(true, 1);
    assert_eq!(m.phase(), Phase::Active);
    m.confirm();
    m.update(7.0);
    m.update(0.0);
    m.dialog_response(true, 0);
    assert_eq!(m.take_storage_requests(), [StorageRequest::Remember(1)]);
    assert!(m.update(5.0).is_none());
    assert_eq!(m.update(5.0), Some(Action::Load { slot: 1 }));
    assert!(m.update(50.0).is_none());
    m.load_failed();
    m.update(10.0);
    assert_eq!(m.phase(), Phase::Active);
    assert_eq!(m.selected(), 1);
}
#[test]
fn input_gates_clamps_priority_geometry_and_scroll_clock() {
    assert!(Controller::new(100, available()).is_err());
    let mut m = Controller::new(5, available()).unwrap();
    m.input(Input {
        down: true,
        ..Default::default()
    });
    assert_eq!(m.selected(), 5);
    m.assets_ready();
    m.update(10.0);
    m.input(Input {
        down: true,
        confirm: true,
        close: true,
        ..Default::default()
    });
    assert_eq!(m.selected(), 6);
    assert_eq!(m.phase(), Phase::Active);
    m.input(Input {
        down: true,
        ..Default::default()
    });
    assert_eq!(m.selected(), 6);
    m.update(1000.0);
    assert!(!m.accepts_input());
    for _ in 0..3 {
        m.update(0.0);
    }
    assert!(m.accepts_input());
    m.input(Input {
        delete: true,
        close: true,
        ..Default::default()
    });
    assert_eq!(m.phase(), Phase::Active);
    let m = active(0);
    assert_eq!(m.pointer(1766.0, 46.0), Pointer::Close);
    assert_ne!(m.pointer(1874.0, 46.0), Pointer::Close);
    assert_eq!(m.pointer(160.0, 176.0), Pointer::Content(Some(0)));
    assert_eq!(m.pointer(160.0, 373.0), Pointer::Content(None));
    assert_eq!(m.pointer(160.0, 377.0), Pointer::Content(Some(1)));
    assert_eq!(m.pointer(f32::NAN, 176.0), Pointer::Outside);
}
#[test]
#[ignore = "requires bounded original Ger SaveMenu CPU oracle"]
fn ger_native_save_menu_constructor_input_hit_and_final_callback() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let oracle = std::env::var_os("SHIN_GEROKASU2_SAVE_MENU_ORACLE").unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROOT").unwrap());
    let mut request = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    let snapshot =
        |op: char, m: &mut Controller, action: u32, remembered: u8, loaded: bool| -> String {
            let sounds = m.take_sounds();
            let cues = ["move", "decide", "cancel", "error"]
                .map(|s| sounds.iter().filter(|&&v| v == s).count());
            format!(
                "{op} 1 {} {} {action} 0 {} {} {} {} {remembered} {}",
                m.selected(),
                m.scroll().to_bits(),
                cues[0],
                cues[1],
                cues[2],
                cues[3],
                u8::from(loaded)
            )
        };
    let inputs = [
        (0, 0, 0.0),
        (0, 1 << 24, 0.0),
        (0, 1 << 25, 0.0),
        (0, 1 << 26, 0.0),
        (0, 1 << 27, 0.0),
        (0, 1 << 10, 0.0),
        (0, 1 << 11, 0.0),
        (1 << 28, 0, 0.0),
        (1 << 29, 0, 0.0),
        ((1 << 7) | (1 << 29), 0, 0.0),
        ((1 << 28) | (1 << 29), 1 << 25, 0.0),
        (0, 0, 1.0),
        (0, 0, -1.0),
    ];
    for remembered in [0, 1, 3, 4, 50, 96, 99] {
        for (pressed, repeated, axis) in inputs {
            for (permission, captured) in [(true, false), (false, false), (true, true)] {
                let mut m = active(remembered);
                request += &format!("N {remembered} 1\n");
                expected.push(snapshot('N', &mut m, 0, remembered, false));
                request += &format!(
                    "I {} {} {pressed:x} {repeated:x} {axis}\n",
                    u8::from(permission),
                    u8::from(captured)
                );
                if permission && !captured {
                    m.input(Input {
                        up: repeated & (1 << 24) != 0,
                        down: repeated & (1 << 25) != 0,
                        left: repeated & (1 << 26) != 0,
                        right: repeated & (1 << 27) != 0,
                        previous_page: repeated & (1 << 10) != 0,
                        next_page: repeated & (1 << 11) != 0,
                        confirm: pressed & (1 << 28) != 0,
                        delete: pressed & (1 << 7) != 0,
                        close: pressed & (1 << 29) != 0,
                        axis,
                    })
                }
                let action = match m.phase() {
                    Phase::RowFlash => 1,
                    Phase::CloseFlash => 3,
                    _ => 0,
                };
                expected.push(snapshot('I', &mut m, action, remembered, false));
                profiles += 1;
            }
        }
    }
    for remembered in 0..100 {
        let mut m = active(remembered);
        request += &format!("N {remembered} 1\n");
        expected.push(snapshot('N', &mut m, 0, remembered, false));
        for (x, y) in [
            (159.0, 176.0),
            (160.0, 176.0),
            (1759.99, 372.99),
            (1760.0, 176.0),
            (160.0, 373.0),
            (160.0, 377.0),
            (1766.0, 46.0),
            (1874.0, 46.0),
            (1766.0, 154.0),
        ] {
            request += &format!("H {x} {y}\n");
            let (hit, row) = match m.pointer(x, y) {
                Pointer::Close => (1, -1),
                Pointer::Content(Some(i)) => (-1, i as i32 + 1),
                Pointer::Content(None) => (
                    -1,
                    if (160.0..1760.0).contains(&x) && (176.0..976.0).contains(&y) {
                        0
                    } else {
                        -1
                    },
                ),
                Pointer::Outside => (0, -1),
            };
            expected.push(format!(
                "H {} {hit} {row}",
                u8::from(!matches!(m.pointer(x, y), Pointer::Content(None)) || row == 0)
            ));
        }
    }
    request += "N 1 1\nQ\nL 1 0 0\nL 1 1 1\nL 1 1 -1\nL 1 1 0\n";
    let mut m = active(1);
    expected.push(snapshot('N', &mut m, 0, 1, false));
    expected.push("Q 5 1 1".into());
    for _ in 0..3 {
        expected.push(snapshot('L', &mut m, 0, 1, false))
    }
    expected.push(snapshot('L', &mut m, 4, 1, true));
    let mut scroll_samples = 0;
    for remembered in [0, 3, 50, 99] {
        for (repeat, input) in [
            (
                1 << 25,
                Input {
                    down: true,
                    ..Default::default()
                },
            ),
            (
                1 << 10,
                Input {
                    previous_page: true,
                    ..Default::default()
                },
            ),
            (
                1 << 11,
                Input {
                    next_page: true,
                    ..Default::default()
                },
            ),
        ] {
            let mut m = active(remembered);
            request += &format!("N {remembered} 1\n");
            expected.push(snapshot('N', &mut m, 0, remembered, false));
            request += &format!("I 1 0 0 {repeat:x} 0\n");
            m.input(input);
            expected.push(snapshot('I', &mut m, 0, remembered, false));
            for delta in [0.0, 0.25, 2.5, 100.0] {
                request += "T 1\n";
                m.update(delta);
                expected.push(snapshot('T', &mut m, 0, remembered, false));
                scroll_samples += 1;
            }
        }
    }
    let mut clock_samples = 0;
    for delta in [0.25f32, 1.0, 2.5, 10.0, 100.0] {
        for load in [false, true] {
            let mut m = Controller::new(2, available()).unwrap();
            m.assets_ready();
            request += "N 2 1\n";
            expected.push(snapshot('N', &mut m, 0, 2, false));
            for _ in 0..((10.0 / delta).ceil() as usize).max(1) {
                request += &format!("O {delta}\n");
                m.update(delta);
                expected.push(format!(
                    "O {} {} 0 0",
                    u8::from(m.progress() < 1.0),
                    m.progress().to_bits()
                ));
                clock_samples += 1;
            }
            if load {
                request += "L 2 1 0\n";
                expected.push(snapshot('L', &mut m, 4, 2, true));
                m.confirmed = Some(2);
            }
            m.phase = Phase::Closing;
            while m.phase == Phase::Closing {
                request += &format!("C {delta}\n");
                let done = m.update(delta).is_some();
                expected.push(format!(
                    "C {} {} {} {}",
                    u8::from(!done),
                    m.progress().to_bits(),
                    if done { if load { 7 } else { 6 } } else { 0 },
                    u8::from(done && !load)
                ));
                clock_samples += 1;
            }
        }
    }
    let mut flash_samples = 0;
    for delta in [0.25f32, 1.0, 2.5, 10.0, 100.0] {
        let mut m = active(2);
        request += "N 2 1\n";
        expected.push(snapshot('N', &mut m, 0, 2, false));
        request += "I 1 0 10000000 0 0\n";
        m.confirm();
        expected.push(snapshot('I', &mut m, 1, 2, false));
        while m.phase() == Phase::RowFlash {
            request += &format!("R {delta}\n");
            m.update(delta);
            let q = m.take_confirmation().is_some();
            expected.push(format!(
                "R {} {} {}",
                m.flash.value().to_bits(),
                u8::from(m.flash.busy()),
                if q { 5 } else { 0 }
            ));
            flash_samples += 1;
        }
    }
    let mut child = Command::new(oracle)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let mut stdin = stdin;
        stdin.write_all(request.as_bytes()).unwrap()
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    let actual = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(a, &e, "native observation{i}")
    }
    println!(
        "Ger SaveMenu: {profiles} input profiles,100 constructors,900 pointer boundaries, {scroll_samples} exact scroll samples,{clock_samples} native open/close samples,{flash_samples} exact flash samples, original Load prompt/default/cancel,4 final/intermediate callback cases"
    );
}
