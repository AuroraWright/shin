use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
};

#[test]
#[ignore = "executes original55548 packets and a9510 copy via checked GPU services"]
fn higurashi_overlay_packets_and_snapshot_extents_match_native() {
    let mut request = String::from("B\n");
    let mut expected = vec![String::from("B 1 15 1 1 2 6 8 2")];
    for (w, h) in [(1280, 720), (1920, 1080), (17, 13)] {
        request.push_str(&format!("C {w} {h}\n"));
        expected.push(format!("C {w} {h} 1"));
    }
    let mut profiles = 0;
    for source in [false, true] {
        for mask in [false, true] {
            for dimensions in [(256, 16), (17, 13), (1920, 1080)] {
                for softness in [1.0 / 256.0, 0.3, 1.0, 4.0] {
                    for flags in 0..16 {
                        for progress in [-0.25, 0.0, 0.125, 0.5, 0.9, 1.0, 1.25] {
                            let mut clock = OverlayClock::captured(source);
                            clock.masked(30, softness, flags, mask);
                            clock.elapsed = progress * 30.0;
                            request.push_str(&format!(
                                "D {} {} {:x} {:x} {:x} {flags} {} {}\n",
                                u32::from(source),
                                u32::from(mask),
                                clock.elapsed.to_bits(),
                                clock.duration.to_bits(),
                                clock.softness.to_bits(),
                                dimensions.0,
                                dimensions.1
                            ));
                            let packet = clock.draw_packet(mask.then_some(dimensions));
                            let mut words = Vec::new();
                            if let Some(packet) = packet {
                                let matrix = [
                                    2.0f32, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, -1.0,
                                    0.0, -1.0, 1.0, -0.0, 1.0,
                                ];
                                match packet {
                                    DrawPacket::Plain { vertices, opacity } => {
                                        words.extend([1, 5, 6, 1, 64]);
                                        words.extend(matrix.map(f32::to_bits));
                                        words.extend([opacity.to_bits(); 4]);
                                        for v in vertices {
                                            words.extend(
                                                {
                                                    let f = v.position;
                                                    f.to_array()
                                                }
                                                .map(f32::to_bits),
                                            );
                                            words.extend(
                                                {
                                                    let f = v.texture_position;
                                                    f.to_array()
                                                }
                                                .map(f32::to_bits),
                                            );
                                        }
                                    }
                                    DrawPacket::Masked { vertices, minmax } => {
                                        words.extend([1, 5, 18, 2, 96]);
                                        words.extend(matrix.map(f32::to_bits));
                                        words.extend([
                                            minmax.x.to_bits(),
                                            minmax.y.to_bits(),
                                            0,
                                            0,
                                        ]);
                                        for v in vertices {
                                            words.extend(
                                                {
                                                    let f = v.position;
                                                    f.to_array()
                                                }
                                                .map(f32::to_bits),
                                            );
                                            words.extend(
                                                {
                                                    let f = v.texture_position;
                                                    f.to_array()
                                                }
                                                .map(f32::to_bits),
                                            );
                                            words.extend(
                                                {
                                                    let f = v.mask_position;
                                                    f.to_array()
                                                }
                                                .map(f32::to_bits),
                                            );
                                        }
                                    }
                                }
                            } else {
                                words.resize(25, 0);
                            }
                            expected.push(format!(
                                "D {}",
                                words
                                    .iter()
                                    .map(u32::to_string)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            ));
                            profiles += 1;
                        }
                    }
                }
            }
        }
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_RECONSTRUCTION_OVERLAY").unwrap())
            .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(), expected.len());
    for (i, (a, e)) in actual.lines().zip(expected).enumerate() {
        assert_eq!(a, e, "native overlay row{i}");
    }
    println!("Native overlay:{profiles} exact draw packet profiles and3 capture extents pass");
}

fn sample(width: u32, height: u32, pixels: &[u8], uv: Vec2) -> f32 {
    let x = uv.x * width as f32 - 0.5;
    let y = uv.y * height as f32 - 0.5;
    let at = |x: f32, y: f32| {
        pixels[(y.clamp(0.0, height as f32 - 1.0) as u32 * width
            + x.clamp(0.0, width as f32 - 1.0) as u32) as usize] as f32
            / 255.0
    };
    let fx = x - x.floor();
    let fy = y - y.floor();
    let a = at(x.floor(), y.floor()) * (1.0 - fx) + at(x.floor() + 1.0, y.floor()) * fx;
    let b = at(x.floor(), y.floor() + 1.0) * (1.0 - fx) + at(x.floor() + 1.0, y.floor() + 1.0) * fx;
    a * (1.0 - fy) + b * fy
}

#[test]
#[ignore = "offscreen Metal scalar shader/blend checks with real MSK3 and capture resize"]
fn higurashi_overlay_pixels_and_fixed_snapshot_survive_viewport_resize() {
    use crate::adv::higurashi::{
        frame::tests::gpu::render_to_texture, message_glyphs::tests::gpu::gpu_assets,
        runner::reconstruction_mode::Overlay,
    };
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        render_texture::RenderTexture,
        resize::ViewportParams,
        shaders::types::texture::{TextureTarget, TextureTargetKind},
    };
    let (device, queue, assets) = gpu_assets();
    let mask =
        shin_tasks::block_on(assets.load::<crate::asset::mask::MaskTexture, _>("/default.msk"))
            .unwrap();
    let bytes = std::fs::read(std::env::var("SHIN_HIGURASHI_DEFAULT_MASK").unwrap()).unwrap();
    let image = shin_core::format::mask::read_higurashi_mask(&bytes).unwrap();
    assert_eq!(mask.texture.size().width, image.width());
    assert_eq!(mask.texture.size().height, image.height());
    let mut count = 0;
    for size in [(1280, 720), (1920, 1080)] {
        for (masked, progress, flags) in [
            (false, 0.0, 0),
            (false, 0.37, 0),
            (false, 1.0, 0),
            (true, 0.0, 0),
            (true, 0.37, 0),
            (true, 0.63, 1),
            (true, 0.37, 6),
            (true, 0.37, 8),
            (true, 0.63, 15),
            (true, 1.0, 0),
        ] {
            let mut clock = OverlayClock::captured(true);
            clock.masked(30, 0.3, flags, masked);
            clock.elapsed = progress * 30.0;
            let pixels = render_to_texture(&device, &queue, size, |context, view| {
                let mut source = context.new_render_texture("Resize source720p".into());
                // Resize the live source to720p and paint all RGBA. Capture
                // after a pending1080p resize must still copy its old extent.
                context
                    .resize_source
                    .resize(ViewportParams::both(winit::dpi::PhysicalSize::new(
                        1280, 720,
                    )));
                {
                    let mut pass =
                        context.begin_pass(source.as_texture_target(), None, "Snapshot source");
                    pass.clear(Some(UnormColor::from_rgba(80, 50, 20, 160)), None, None);
                }
                context
                    .resize_source
                    .resize(ViewportParams::both(winit::dpi::PhysicalSize::new(
                        size.0, size.1,
                    )));
                let mut snapshot = source.copy_snapshot(&mut context.render_clone_ctx());
                assert_eq!(source.dimensions(), (1280, 720));
                assert_eq!(snapshot.dimensions(), (1280, 720));
                // Its independent resize owner also preserves dimensions if
                // accidentally queried as a render target after host resize.
                let _ = snapshot.as_texture_target();
                assert_eq!(snapshot.dimensions(), (1280, 720));
                // Change live pixels after copying to detect texture aliasing.
                {
                    let mut pass =
                        context.begin_pass(source.as_texture_target(), None, "Replace live source");
                    pass.clear(Some(UnormColor::from_rgba(1, 2, 3, 4)), None, None);
                }
                let overlay = Overlay {
                    clock: clock.clone(),
                    snapshot,
                    mask: masked.then(|| Arc::clone(&mask)),
                };
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Overlay scalar reference",
                );
                pass.clear(
                    Some(UnormColor::from_rgba(13, 29, 41, 73)),
                    Some(0),
                    Some(1.0),
                );
                overlay.render(&mut pass);
            });
            for y in (0..size.1).step_by(31) {
                for x in (0..size.0).step_by(23) {
                    let weight = if !clock.visible() {
                        0.0
                    } else if !masked {
                        1.0 - clock.elapsed / clock.duration
                    } else {
                        let mut uv = glam::vec2(
                            (x as f32 + 0.5) / size.0 as f32,
                            (y as f32 + 0.5) / size.1 as f32,
                        );
                        if flags & 2 != 0 {
                            uv.x = 1.0 - uv.x;
                        }
                        if flags & 4 != 0 {
                            uv.y = 1.0 - uv.y;
                        }
                        if flags & 8 != 0 {
                            uv *= glam::vec2(
                                1920.0 / image.width() as f32,
                                1080.0 / image.height() as f32,
                            );
                        }
                        let [a, b] = clock.mask_weights();
                        (sample(image.width(), image.height(), image.as_raw(), uv) * (b - a) + a)
                            .clamp(0.0, 1.0)
                    };
                    let alpha = 160.0 / 255.0 * weight;
                    let expected = [
                        80.0 * weight + 13.0 * (1.0 - alpha),
                        50.0 * weight + 29.0 * (1.0 - alpha),
                        20.0 * weight + 41.0 * (1.0 - alpha),
                        160.0 * weight * (1.0 - 73.0 / 255.0) + 73.0,
                    ];
                    let pixel = &pixels[((y * size.0 + x) * 4) as usize..][..4];
                    for c in 0..4 {
                        assert!(
                            (pixel[c] as f32 - expected[c]).abs() <= 1.6,
                            "{size:?} mask{masked} p{progress} flags{flags} at{x},{y}: {pixel:?} expected{expected:?}"
                        );
                    }
                }
            }
            count += 1;
        }
    }
    println!(
        "Higurashi overlay:{count}720p/1080p scalar pixel and fixed capture resize profiles pass"
    );
}
