use super::*;
use crate::layer::DrawableLayer;
#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn ger_app_message_layer_minus5_controls_render_and_capture_without_changing_screen() {
    use shin_core::vm::command::types::LayerProperty;
    let gpu = Offscreen::new();
    let original = Gerokasu2Scenario::new(bytes().into()).unwrap();
    let mut data = bytes();
    data.truncate(original.header.code_offset as usize);
    let start = data.len();
    msg(&mut data, 1, "@rメッセージレイヤー復元");
    data[start + 4] = 0;
    fn wait(d: &mut Vec<u8>) {
        d.extend([0x83, 0]);
        num(d, 30);
    }
    fn opacity(d: &mut Vec<u8>, value: i32) {
        d.push(0xc3);
        num(d, -5);
        num(d, 9);
        d.push(3);
        num(d, value);
        num(d, 0);
    }
    wait(&mut data);
    opacity(&mut data, 0);
    wait(&mut data);
    opacity(&mut data, 1000);
    wait(&mut data);
    data.extend([0, 0, 0]);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    let mut host = host(&gpu, data);
    let mut frame = FrameId::default();
    let idle = EnumMap::default();
    let mut frames = Frames::new(&gpu);
    let mut hidden = false;
    let mut visible = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.status != RunnerStatus::Exited {
        tick(&mut host, &mut frame, &idle, Vec2::ZERO, 6.0);
        let pixels = frames.draw(&gpu, &mut host);
        let target = host.scene.save_session().descriptors.message_properties[9];
        if target == 0 && host.scene.runner().completed_commands() >= 3 {
            assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            hidden = true;
        } else if hidden && target == 1000 {
            assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
            visible = true;
        }
        assert_eq!(
            host.scene
                .backend_mut()
                .root()
                .properties()
                .get_value(LayerProperty::MulColorAlpha),
            1000.0
        );
        assert!(Instant::now() < deadline);
    }
    assert!(hidden && visible);
    assert_eq!(host.scene.runner().completed_commands(), 7);
    println!(
        "GEROKASU2_MESSAGE_PROPERTY_APP commands=7 minus5_opacity_hide_restore_descriptors_screen_preservation_EXIT=verified frames={} channels={} audio=Loopback Audio",
        frames.frames,
        frames.frames * 320 * 180 * 4
    );
}
