//! Entire unmodified normal prologue through the real Gameplay/Host frame loop.
//! Reports every admitted frame around MSG1616; no guessed mid-script entry.
use super::{selection::Frames, *};
use crate::adv::higurashi::{
    audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture,
};
use shin_core::{
    primitives::color::UnormColor, vm::higurashi::command::HigurashiRuntimeCommand as Cmd,
};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

#[test]
#[ignore = "unmodified normal prologue/live autoskip and exact quoted-line frame captures; offscreen Metal/exact Loopback Audio, no window/user saves"]
fn higurashi_gameplay_prologue_autoskip_has_no_wait_frame_after_last_message() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    // An empty replacement preserves every authored byte, including init0.
    let (authored, common) = super::character::common(&server, &[]);
    assert_eq!(common.scenario.raw(), authored.raw());
    assert_eq!(authored.picture_info[745].name.as_str(), "a/cinema");
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut saved = SaveData::default();
    saved.settings.common.v11_disallowskipunread = false;
    saved.settings.common.v9_msgspeed = 100;
    saved.settings.common.v10_skipspeed = 100;
    let globals = Globals {
        storage: None,
        runtime_saves: None,
        common,
        data: saved,
        clock: PlayClock::from_saved_seconds(0),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), authored),
        server,
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    };
    let mut game = None;
    reference_render_to_texture(&device, &queue, (1280, 720), |context, _| {
        let mut source = context.new_render_texture("Normal prologue fresh source".into());
        context
            .begin_pass(source.as_texture_target(), None, "Initial black")
            .clear(Some(UnormColor::BLACK), None, None);
        let mut data = fresh();
        data.entry.random_seed = 42;
        game = Some(Gameplay::new(globals, data, 0, source, || [1, 2, 3, 4]).unwrap());
    });
    let mut game = game.unwrap();
    let mut frames = Frames { number: 0 };
    let mut toggled = false;
    let mut saw_first = false;
    let mut saw_last = false;
    let mut tail_waits = Vec::new();
    let mut rows = Vec::new();
    let output =
        std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_PROLOGUE_BARS_CAPTURE").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let runner = &game.host().unwrap().runner;
        if matches!(runner.pending_command(), Some(Cmd::MSGSET { msg_id, .. }) if msg_id.0 == 1579)
        {
            saw_first = true;
        }
        if matches!(runner.pending_command(), Some(Cmd::MSGSET { msg_id, .. }) if msg_id.0 == 1616)
        {
            saw_last = true;
        }
        let previous_epoch = runner.frame().visual_epoch();
        let activate = saw_first && game.input_permission && !toggled;
        let buttons = if activate {
            toggled = true;
            Buttons {
                pressed: 1 << 11,
                ..Default::default()
            }
        } else {
            Buttons::default()
        };
        frames.step(&mut game, buttons, &[]);
        let pixels =
            reference_render_to_texture(&device, &queue, (1280, 720), |context, target| {
                game.prepare(context).unwrap();
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: target,
                        kind: TextureTargetKind::RenderTexture,
                    },
                    Some(context.depth_stencil),
                    "Normal prologue displayed frame",
                );
                game.render(&mut pass).unwrap();
            });
        let runner = &game.host().unwrap().runner;
        if !runner.native_vm_attached() {
            assert!(
                Instant::now() < deadline,
                "normal prologue initial resource gate timed out"
            );
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        let pc = runner.position().0;
        let pending = format!("{:?}", runner.pending_command());
        let tail = (0x5a89e..0x5aa13).contains(&pc);
        let eligible = crate::adv::higurashi::input::suppress_transient_audio(
            runner.native_read_state(),
            game.reference_play_modes(),
            false,
            false,
        );
        if tail {
            assert!(
                game.reference_play_modes() & 2 != 0,
                "live skip must be accepted before tail"
            );
            assert!(
                eligible,
                "tail must have genuine native read-policy eligibility"
            );
        }
        if tail
            && matches!(runner.pending_command(), Some(Cmd::MSGSET { msg_id, .. }) if msg_id.0 == 1616)
        {
            saw_last = true;
        }
        if tail && saw_last && matches!(runner.pending_command(), Some(Cmd::WAIT { .. })) {
            tail_waits.push(pc);
        }
        let banks = [0, 1].map(|bank| {
            runner.frame().scene.layers().reference_group_entries(bank)
            .iter().map(|(slot, remaining, owner)| serde_json::json!({
                "slot": slot, "remaining_bits": remaining, "logical": owner.owner_id(),
                "kind": owner.layer_type().native_id(), "properties": owner.properties.values().to_vec(),
            })).collect::<Vec<_>>()
        });
        let row = serde_json::json!({
            "frame": frames.number, "pc": pc, "pending": pending, "eligible": eligible,
            "visual_update_admitted": runner.frame().visual_epoch() != previous_epoch,
            "commands": runner.completed_commands(), "modes": game.reference_play_modes(),
            "read": format!("{:?}", runner.native_read_state()),
            "epoch": runner.frame().visual_epoch(),
            "cache": format!("{:?}", runner.frame().cache.reference_snapshot()),
            "banks": banks,
        });
        if tail || saw_last {
            eprintln!("PROLOGUE_FRAME {row}");
        }
        rows.push(row);
        // Render every outer update, including loading/waiting frames. Save
        // actual pixels only near the issue, keeping the run bounded in size.
        if tail {
            std::fs::write(
                output.join(format!("frame-{}-{pc:x}.rgba", frames.number)),
                pixels,
            )
            .unwrap();
        }
        // Stop at the first observed protected movie event. This full update
        // may have started Movie0's worker; no movie playback acceptance,
        // deferred chapter or controller acceptance is claimed.
        if pc >= 0x222e69 && saw_last {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "normal prologue timed out at {pc:x} {pending}"
        );
        assert!(frames.number < 3000, "normal prologue frame bound");
        std::thread::sleep(Duration::from_millis(1));
    }
    std::fs::write(
        output.join("frames.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
    assert!(
        saw_first && saw_last && toggled,
        "must traverse authored MSG1579..1616 with live skip toggle"
    );
    assert!(
        tail_waits.is_empty(),
        "eligible WAIT must complete before displaying the cinema tail: {tail_waits:x?}"
    );
    eprintln!(
        "PROLOGUE_PASS frames={} final_quoted_message=true tail_waits=0",
        frames.number
    );
}
