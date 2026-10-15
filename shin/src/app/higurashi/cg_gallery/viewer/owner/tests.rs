//! The new concrete InputArea adapter is tested directly. Art/worker/GPU owner
//! acceptance remains a separate parent-run integration test, not a mock claim.
use super::*;
use crate::adv::input_area::{PointerEvent, PointerKind};
use glam::IVec2;
#[derive(Default)]
struct Checked {
    loads: Vec<u16>,
    fail: Option<u16>,
    cues: usize,
}
impl Services for Checked {
    fn prepare_picture(&mut self, id: u16) -> Result<()> {
        self.loads.push(id);
        ensure!(Some(id) != self.fail, "checked picture preparation failure");
        Ok(())
    }
    fn worker_status(&mut self) -> u32 {
        0
    }
    fn take_prepared(&mut self) -> Result<Picture> {
        bail!("no checked art")
    }
    fn capture_parent(&mut self, _: &Model) -> Result<()> {
        bail!("no checked bitmap")
    }
    fn publish_capture(&mut self, _: &Model) -> Result<()> {
        bail!("no staged checked bitmap")
    }
    fn input(&mut self, _: &mut Model, _: bool) -> Result<()> {
        bail!("no recursive input")
    }
    fn cue(&mut self, _: Cue) -> Result<()> {
        self.cues += 1;
        Ok(())
    }
    fn disable_input(&mut self) {}
}
fn model(services: &mut Checked) -> Model {
    Model::new(
        super::super::super::Entry {
            name: "001a".into(),
            pictures: vec![0, 1, 2],
            unlocked: vec![true, true, true],
        },
        services,
    )
    .unwrap()
}
fn event(kind: PointerKind, width: u16, height: u16) -> PointerEvent {
    PointerEvent {
        kind,
        id: 0,
        position: IVec2::new(width as i32 / 3, height as i32 / 2),
    }
}
#[test]
fn reference_cg_viewer_real_input_area_release_cache_cancel_and_worker_error() {
    // Primary40914 always probes true/clears inertia;4093c cues decide then
    //3f800 starts the next unlocked variant. Reset/press/cancel/longpress RET.
    for (width, height) in [(1280, 720), (1920, 1080)] {
        let mut checked = Checked::default();
        let mut model = model(&mut checked);
        let mut area = InputArea::new(width, height);
        model.velocity = Vec2::new(17., -9.);
        let down = [event(PointerKind::Down, width, height)];
        let up = [event(PointerKind::Up, width, height)];
        assert!(
            dispatch_pointer(
                &mut area,
                &mut model,
                true,
                &PointerFrame {
                    number: 1,
                    time_seconds: 0.,
                    events: &down
                },
                &mut checked
            )
            .unwrap()
        );
        assert_eq!(model.velocity, Vec2::ZERO);
        assert_eq!(checked.loads, [0]);
        assert!(
            dispatch_pointer(
                &mut area,
                &mut model,
                true,
                &PointerFrame {
                    number: 2,
                    time_seconds: 0.05,
                    events: &up
                },
                &mut checked
            )
            .unwrap()
        );
        assert_eq!(
            (model.variant, checked.loads.clone(), checked.cues),
            (1, vec![0, 1], 1)
        );
        // A second consumer sees the cached capture result without repeating
        // a release callback, and a later orphan Up cannot advance a variant.
        assert!(
            dispatch_pointer(
                &mut area,
                &mut model,
                true,
                &PointerFrame {
                    number: 2,
                    time_seconds: 0.05,
                    events: &up
                },
                &mut checked
            )
            .unwrap()
        );
        assert!(
            !dispatch_pointer(
                &mut area,
                &mut model,
                true,
                &PointerFrame {
                    number: 3,
                    time_seconds: 0.06,
                    events: &up
                },
                &mut checked
            )
            .unwrap()
        );
        assert_eq!(checked.loads, [0, 1]);
        dispatch_pointer(
            &mut area,
            &mut model,
            true,
            &PointerFrame {
                number: 4,
                time_seconds: 0.1,
                events: &down,
            },
            &mut checked,
        )
        .unwrap();
        dispatch_pointer(
            &mut area,
            &mut model,
            false,
            &PointerFrame {
                number: 5,
                time_seconds: 0.12,
                events: &[],
            },
            &mut checked,
        )
        .unwrap();
        dispatch_pointer(
            &mut area,
            &mut model,
            true,
            &PointerFrame {
                number: 6,
                time_seconds: 0.15,
                events: &up,
            },
            &mut checked,
        )
        .unwrap();
        assert_eq!(checked.loads, [0, 1]);
        checked.fail = Some(2);
        dispatch_pointer(
            &mut area,
            &mut model,
            true,
            &PointerFrame {
                number: 7,
                time_seconds: 0.2,
                events: &down,
            },
            &mut checked,
        )
        .unwrap();
        let error = dispatch_pointer(
            &mut area,
            &mut model,
            true,
            &PointerFrame {
                number: 8,
                time_seconds: 0.25,
                events: &up,
            },
            &mut checked,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("checked picture preparation failure")
        );
        assert_eq!((checked.loads, checked.cues), (vec![0, 1, 2], 2));
    }
}

#[test]
fn reference_cg_viewer_default_long_press_still_releases_next_variant() {
    let mut checked = Checked::default();
    let mut model = model(&mut checked);
    let mut area = InputArea::new(1920, 1080);
    let down = [event(PointerKind::Down, 1920, 1080)];
    let up = [event(PointerKind::Up, 1920, 1080)];
    for (number, time_seconds, events) in [
        (1, 0., down.as_slice()),
        (2, 2., &[][..]),
        (3, 2.01, up.as_slice()),
    ] {
        dispatch_pointer(
            &mut area,
            &mut model,
            true,
            &PointerFrame {
                number,
                time_seconds,
                events,
            },
            &mut checked,
        )
        .unwrap();
    }
    assert_eq!(
        (model.variant, checked.loads, checked.cues),
        (1, vec![0, 1], 1)
    );
}

#[test]
fn reference_cg_owner_original14_lifetime_order_profiles() {
    let output = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/cg-gallery-owner-capture/output.txt"
    ));
    let (mut owners, mut events, mut seeds, mut captures) = (0, 0, 0, 0);
    let mut names = Vec::new();
    for line in output.lines() {
        let w = line.split_whitespace().collect::<Vec<_>>();
        match w[0] {
            "N" => {
                owners += 1;
                names.clear();
            }
            "E" => {
                events += 1;
                names.push(w[2]);
                if w[2] == "seed" {
                    seeds += 1;
                }
                if w[2] == "capture" {
                    captures += 1;
                }
            }
            "X" => {
                assert_eq!(w[5], "0");
                assert_eq!(w[2].parse::<usize>().unwrap(), names.len());
                assert!(names.contains(&"join"));
            }
            _ => panic!("owner trace opcode"),
        }
    }
    assert_eq!((owners, events, seeds, captures), (14, 250, 72, 16));
}

#[test]
#[ignore = "runs frozen actual original CPU owner/worker/Picture14a8 zero-block order probe;no GPU/audio/window/save"]
fn reference_cg_owner_original_order_reexecution() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let exe = std::env::var("SHIN_HIGURASHI_CG_OWNER_CAPTURE")
        .unwrap_or_else(|_| "/tmp/higurashi-cg-gallery-owner-native-new".into());
    let mut child = Command::new(exe)
        .arg(root.join("higurashi/uncompressed_executable_update"))
        .arg("/tmp/higurashi-newrodin.fnt")
        .arg("/tmp/higurashi-scenario-patch.rom.snr")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../reverse-engineering/higurashi/cg-gallery-owner-capture/input.txt"
        )))
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../reverse-engineering/higurashi/cg-gallery-owner-capture/output.txt"
        ))
    );
}
