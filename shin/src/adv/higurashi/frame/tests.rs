use super::super::{
    layers::HigurashiLayers, message_glyphs::tests::gpu::gpu_assets, message_playback::WaitKind,
};
use super::*;
use shin_core::format::scenario::instruction_elements::UntypedNumberArray;
use std::fmt::Write as _;
use std::{
    io::Write,
    process::{Command, Stdio},
};

struct Voice;
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        false
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        false
    }
    fn stop(&mut self) {}
}

fn snapshot(frame: &HigurashiFrame, getter: u32, updated: bool) -> Vec<u32> {
    let mut words = vec![
        getter,
        if updated { 2 } else { 0 },
        u32::from(updated),
        u32::from(frame.autoplay_request),
        u32::from(frame.skip_request),
    ];
    if updated {
        words.extend([1, 2]);
    }
    let p = &frame.message.playback;
    words.extend([
        p.natural_slide.to_bits(),
        p.modal_slide.to_bits(),
        p.current_block as u32,
        p.current_time.to_bits(),
        p.height.to_bits(),
        p.height_target.to_bits(),
        match p.wait {
            None => u32::MAX,
            Some(WaitKind::Regular) => 0,
            Some(WaitKind::Last) => 1,
        },
        p.ticks_since_wait.to_bits(),
    ]);
    let layers = frame.scene.layers();
    for owner in [layers.message_properties(), layers.screen_properties()] {
        words.extend(owner.values().map(f32::to_bits));
        words.extend(owner.oscillators.map(|o| o.phase.to_bits()));
        let t = owner.composed_transform(&Default::default());
        words.extend(t.transform.to_cols_array().map(f32::to_bits));
        words.extend(t.camera.to_array().map(f32::to_bits));
        words.extend(t.inherited.to_array().map(f32::to_bits));
        words.extend(t.wobble.to_array().map(f32::to_bits));
    }
    for draw in frame.render_plan() {
        words.extend([
            match draw.child {
                Child::Overlay => 0,
                Child::Message => 1,
                Child::Screen => 2,
            },
            u32::from(draw.stencil),
            match draw.pass {
                PassKind::Opaque => 1,
                PassKind::Transparent => 0,
            },
        ]);
    }
    words
}

#[test]
#[ignore = "executes actual ADV gate, message/property clocks and outer traversal; offscreen resources only"]
fn higurashi_frame_matches_native_gate_properties_and_outer_order() {
    let (device, queue, assets) = gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut voice = Voice;
    for case in 0..96 {
        let mut frame = HigurashiFrame::new(
            HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4])),
            shin_tasks::block_on(HigurashiMessageOwner::load(&assets)).unwrap(),
        );
        frame.message.playback.shown = true;
        frame.message.playback.wait = Some(WaitKind::Last);
        input.push_str("N\n");
        expected.push(snapshot(&frame, 0, false));
        for (id, prop, value, duration, flags) in [
            (
                -3,
                5,
                [900, 1000, 1100][case % 3],
                10,
                0x100 | (case % 4) as u32,
            ),
            (
                -1,
                5,
                [1000, 900, 1100][(case / 3) % 3],
                10,
                0x100 | ((case / 4) % 4) as u32,
            ),
            (-3, 0, (case as i32 - 48) * 3, 17, 0x100),
            (-1, 0, (48 - case as i32) * 5, 13, 0x100),
            (-3, 12, 750 + (case as i32 % 4) * 125, 15, 0x100),
            (-1, 13, 1100, 11, 0x100),
            (-3, 27, 2, 0, 0x100),
            (-3, 28, 20, 0, 0x100),
            (-3, 29, 33, 0, 0x100),
            (-1, 30, 5, 0, 0x100),
            (-1, 31, 15, 0, 0x100),
            (-1, 32, 27, 0, 0x100),
        ] {
            input.push_str(&format!("C {id} {prop} {value} {duration} {flags:x}\n"));
            assert_eq!(
                shin_tasks::block_on(frame.dispatch(
                    &HigurashiRuntimeCommand::LAYERCTRL {
                        args: [id, prop],
                        params: (value, duration, flags as i32, 0, 0, 0, 0, 0)
                    },
                    &assets,
                    &scenario,
                    || [1, 2, 3, 4],
                    &device,
                    &queue,
                    false,
                    0,
                    50,
                    0,
                    &voice
                ))
                .unwrap(),
                Some(CommandStatus::Complete)
            );
            expected.push(snapshot(&frame, if id == -3 { 1 } else { 2 }, false));
        }
        for step in 0..24 {
            if step % 6 == 0 {
                let automatic = step % 12 == 0;
                let skip = step % 12 == 6;
                input.push_str(&format!("Q {} {}\n", u8::from(automatic), u8::from(skip)));
                frame.request_message_update(automatic, skip);
                expected.push(snapshot(&frame, 0, false));
            }
            if step % 7 == 0 {
                let hidden = step % 14 == 7;
                input.push_str(&format!("H {}\n", u8::from(hidden)));
                frame.message.playback.hidden = hidden;
                expected.push(snapshot(&frame, 0, false));
            }
            let delta = [0.0f32, 0.5, 1.0, 2.0, 5.0, -0.5][(step + case) % 6];
            let animate = step % 5 != 0;
            let paused = step % 5 == 1;
            input.push_str(&format!(
                "U {:x} {} {}\n",
                delta.to_bits(),
                u8::from(animate),
                u8::from(paused)
            ));
            frame.advance(Ticks::from_f32(delta), animate, paused, &mut voice, None);
            expected.push(snapshot(&frame, 0, animate && !paused));
        }
        // One continuation owner covers message waits and screen commands.
        frame.message.playback.hidden = false;
        frame.message.playback.modal_slide = 1.0;
        shin_tasks::block_on(frame.dispatch(
            &HigurashiRuntimeCommand::MSGSET {
                msg_id: shin_core::format::scenario::instruction_elements::MessageId(456),
                auto_wait: 0,
                argument: -1,
                text: "名前@r本文。".into(),
            },
            &assets,
            &scenario,
            || [1, 2, 3, 4],
            &device,
            &queue,
            true,
            0,
            50,
            0,
            &voice,
        ))
        .unwrap();
        assert_eq!(
            shin_tasks::block_on(frame.dispatch(
                &HigurashiRuntimeCommand::MSGWAIT { signal: -1 },
                &assets,
                &scenario,
                || [1, 2, 3, 4],
                &device,
                &queue,
                false,
                0,
                50,
                0,
                &voice
            ))
            .unwrap(),
            Some(CommandStatus::Pending)
        );
        assert!(
            shin_tasks::block_on(frame.dispatch(
                &HigurashiRuntimeCommand::WIPE {
                    args: [0, 0, 0],
                    params: UntypedNumberArray::default()
                },
                &assets,
                &scenario,
                || [1, 2, 3, 4],
                &device,
                &queue,
                false,
                0,
                50,
                0,
                &voice
            ))
            .is_err()
        );
        assert_eq!(frame.poll_command(false, &voice), CommandStatus::Pending);
        frame.message.fast_forward(&mut voice);
        assert_eq!(frame.poll_command(false, &voice), CommandStatus::Complete);
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_FRAME").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
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
    assert!(String::from_utf8_lossy(&output.stderr).contains("balanced ownership"));
    let native = String::from_utf8(output.stdout).unwrap();
    assert_eq!(native.lines().count(), expected.len());
    for (row, (line, expected)) in native.lines().zip(&expected).enumerate() {
        let actual: Vec<u32> = line
            .split_whitespace()
            .skip(1)
            .map(|w| w.parse().unwrap())
            .collect();
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(a, e, "native frame row{row} word{i}");
        }
        assert_eq!(actual.len(), expected.len(), "row{row}");
    }
    println!(
        "Higurashi ADV frame:96 sequences, {} native snapshots, {} bitwise observations match; pending owner blocks cross-dispatch",
        expected.len(),
        expected.iter().map(Vec::len).sum::<usize>()
    );
}

pub(in crate::adv::higurashi) mod gpu;

#[test]
#[ignore = "original three-child6e340/686d8 priority/stencil/pass traversal with prepared property/draw services"]
fn higurashi_choice_overlay_draw_order_matches_native_three_children() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for overlay in [-5.0f32, 0.0, 1.0, 900.0, 1000.0] {
        for message in [-5.0f32, 0.0, 1.0, 900.0, 1000.0] {
            for screen in [-5.0f32, 0.0, 1.0, 900.0, 1000.0] {
                writeln!(
                    input,
                    "{:x} {:x} {:x}",
                    overlay.to_bits(),
                    message.to_bits(),
                    screen.to_bits()
                )
                .unwrap();
                let mut row = String::from("F");
                for draw in render_plan_with_overlay(message, screen, Some(overlay)) {
                    let id = match draw.child {
                        Child::Overlay => 0,
                        Child::Message => 1,
                        Child::Screen => 2,
                    };
                    write!(
                        row,
                        " {id} {} {}",
                        draw.stencil,
                        u8::from(draw.pass == PassKind::Opaque)
                    )
                    .unwrap();
                }
                expected.push(row);
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHOICE_ORDER").unwrap())
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
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
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().collect::<Vec<_>>(), expected);
    eprintln!(
        "Choice overlay draw order:125 native three-child priority/stencil/pass profiles and balanced ownership pass"
    );
}
