//! Production clocks/packets versus original0..4; supplied texture bytes and
//! prepared clock are explicit services, independent replay remains frozen.
use super::{
    derived::{HigurashiWiper, Packet},
    derived_native_tests::{Operation, corpus},
};
use crate::{
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    asset::mask::reference_wipe_texture,
};
use shin_core::{primitives::color::UnormColor, time::Ticks};
use shin_render::{
    RenderRequestBuilder,
    gpu_texture::GpuTexture,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};
use std::sync::Arc;

fn parameters(
    p: [i32; 8],
) -> shin_core::format::scenario::instruction_elements::UntypedNumberArray {
    (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7])
}
fn mask(
    context: &crate::asset::system::AssetLoadContext,
    size: [u32; 2],
) -> Arc<crate::asset::mask::MaskTexture> {
    let pixels = (0..size[1])
        .flat_map(|y| (0..size[0]).map(move |x| ((x * 17 + y * 31 + (x / 11) * 7) % 256) as u8))
        .collect();
    Arc::new(reference_wipe_texture(context, size, pixels))
}
fn compare_packet(packet: &Packet, native: &[u32; 113], row: usize) -> Vec<String> {
    let differences = std::cell::RefCell::new(Vec::new());
    let float = |a: f32, b: u32, what: &str| {
        let b = f32::from_bits(b);
        if !(a == b || (a.is_nan() && b.is_nan())) {
            differences.borrow_mut().push(format!(
                "row{row} {what}: {a:?} != {b:?} ({:08x}/{:08x})",
                a.to_bits(),
                b.to_bits()
            ));
        }
    };
    let (transform, vertices, program) = match packet {
        Packet::Default {
            transform,
            vertices,
            alpha,
        } => {
            float(*alpha, native[41], "alpha");
            let v: Vec<f32> = vertices
                .iter()
                .flat_map(|v| {
                    [
                        v.position.x,
                        v.position.y,
                        v.texture_position.x,
                        v.texture_position.y,
                    ]
                })
                .collect();
            (*transform, v, 21)
        }
        Packet::Mask {
            transform,
            vertices,
            minmax,
        } => {
            for (a, b) in minmax.to_array().into_iter().zip(&native[41..43]) {
                float(a, *b, "minmax");
            }
            let v: Vec<f32> = vertices
                .iter()
                .flat_map(|v| {
                    [
                        v.position.x,
                        v.position.y,
                        v.texture_position.x,
                        v.texture_position.y,
                        v.mask_position.x,
                        v.mask_position.y,
                    ]
                })
                .collect();
            (*transform, v, 22)
        }
        Packet::Sprite {
            transform,
            vertices,
            incoming,
            blend,
        } => {
            assert_eq!(native[13], if *incoming { 2 } else { 1 }, "row{row} source");
            assert_eq!(
                native[4],
                if *blend == shin_render::ColorBlendType::Opaque {
                    1
                } else {
                    2
                }
            );
            let v: Vec<f32> = vertices
                .iter()
                .enumerate()
                .flat_map(|(index, v)| {
                    // Native2's packed colors are BGRA; all wipe RGB are white.
                    let color = v.color;
                    let packed = u32::from_le_bytes(bytemuck::bytes_of(&color).try_into().unwrap());
                    assert_eq!(packed, native[89 + index * 6 + 3], "row{row} packed color");
                    [
                        v.position.x,
                        v.position.y,
                        v.position.z,
                        f32::from_bits(packed),
                        v.texture_position.x,
                        v.texture_position.y,
                    ]
                })
                .collect();
            (*transform, v, 2)
        }
    };
    assert_eq!(native[3], program);
    for (a, b) in transform.to_cols_array().into_iter().zip(&native[25..41]) {
        float(a, *b, "projection");
    }
    for (a, b) in vertices.into_iter().zip(&native[89..]) {
        float(a, *b, "vertex");
    }
    differences.into_inner()
}
#[test]
#[ignore = "all2058 original actions/81 factories/243 clocks/1512 production packet comparisons; needs Metal only to construct explicit Mask resources"]
fn higurashi_derived_wipe_clocks_and_packets_match_original() {
    let (_, _, assets) = reference_gpu_assets();
    let corpus = corpus();
    let mut wiper = None;
    let mut packets = 0;
    let mut differences = Vec::new();
    for row in &corpus.rows {
        match row.operation {
            Operation::New {
                kind,
                duration,
                params,
                mask_extent,
            } => {
                wiper = Some(
                    HigurashiWiper::new(
                        kind,
                        duration as u32,
                        parameters(params),
                        Some(mask(assets.context(), mask_extent)),
                    )
                    .unwrap(),
                )
            }
            Operation::Clock(words) => wiper.as_mut().unwrap().reference_set_clock(words),
            Operation::Update(delta) => assert_eq!(
                u32::from(
                    wiper
                        .as_mut()
                        .unwrap()
                        .advance(Ticks::from_f32(f32::from_bits(delta)))
                ),
                row.snapshot.last_update_return
            ),
            Operation::FastForward => wiper.as_mut().unwrap().fast_forward(),
            Operation::Draw(_) => {
                let production = wiper.as_ref().unwrap().packets();
                let native: Vec<_> = row.draws().collect();
                assert_eq!(production.len(), native.len());
                for (a, b) in production.iter().zip(native) {
                    differences.extend(compare_packet(a, b, row.index));
                    packets += 1;
                }
            }
            Operation::Dispose => wiper = None,
        }
        if let Some(w) = &wiper {
            assert_eq!(
                w.reference_clock(),
                row.snapshot.clock,
                "row{} clock",
                row.index
            );
        }
    }
    assert_eq!(packets, 1512);
    assert!(
        differences.is_empty(),
        "{} packet differences: {:?}",
        differences.len(),
        &differences[..differences.len().min(32)]
    );
    eprintln!(
        "81 original wipe factories/243 actual callbacks/1512 complete production packets match; Mask GPU image and prepared clock services explicit"
    );
}
#[test]
#[ignore = "200 complete older wipe images against independent literal original shaders at720p/1080p on Metal"]
fn higurashi_derived_wipe_pixels_match_original() {
    let (device, queue, assets) = reference_gpu_assets();
    let pixels = |incoming: bool| {
        (0..1080u32)
            .flat_map(|y| {
                (0..1920u32).flat_map(move |x| {
                    if incoming {
                        [
                            (x * 7 + y * 11) as u8,
                            197,
                            (x / 13 + y / 9) as u8,
                            (79 + (x + y) % 177) as u8,
                        ]
                    } else {
                        [
                            23,
                            (x * 3 + y * 5) as u8,
                            (x / 17 + y / 7) as u8,
                            (41 + (x * 2 + y) % 213) as u8,
                        ]
                    }
                })
            })
            .collect::<Vec<u8>>()
    };
    let old = GpuTexture::new_static_with_data(
        &device,
        &queue,
        None,
        winit::dpi::PhysicalSize::new(1920, 1080),
        shin_render::TEXTURE_FORMAT,
        &pixels(false),
    );
    let new = GpuTexture::new_static_with_data(
        &device,
        &queue,
        None,
        winit::dpi::PhysicalSize::new(1920, 1080),
        shin_render::TEXTURE_FORMAT,
        &pixels(true),
    );
    let corpus = corpus();
    let mut count = 0;
    for observation in corpus.gpu_observations() {
        let profile = &corpus.manifest.profiles[observation.profile];
        let row = &corpus.rows[observation.row];
        let mask = mask(assets.context(), profile.mask_extent);
        let mut wiper = HigurashiWiper::new(
            profile.kind,
            profile.duration as u32,
            parameters(profile.params),
            Some(mask.clone()),
        )
        .unwrap();
        wiper.reference_set_clock(row.snapshot.clock);
        let output = (observation.extent[0], observation.extent[1]);
        let expected = reference_render_to_texture(&device, &queue, output, |context, target| {
            super::derived_native_tests::gpu::replay(
                context,
                target,
                row,
                [
                    old.as_source().view,
                    new.as_source().view,
                    mask.texture.as_source().view,
                ],
            )
        });
        let actual = reference_render_to_texture(&device, &queue, output, |context, target| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Production older wipe",
            );
            pass.clear(
                Some(UnormColor::from_rgba(0x57, 0x35, 0x21, 255)),
                Some(0),
                Some(0.),
            );
            wiper.render(
                &mut pass,
                RenderRequestBuilder::new(),
                new.as_source(),
                old.as_source(),
            );
        });
        let differences = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
        let max = actual
            .iter()
            .zip(&expected)
            .map(|(&a, &b)| a.abs_diff(b))
            .max()
            .unwrap();
        if differences != 0 {
            let directory = std::env::temp_dir().join(format!(
                "higurashi-derived-wipe-failure-{}-{}",
                std::process::id(),
                observation.index
            ));
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("actual.rgba"), &actual).unwrap();
            std::fs::write(directory.join("expected.rgba"), &expected).unwrap();
            std::fs::write(
                directory.join("frame.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "name": observation.name, "extent": observation.extent,
                    "profile": observation.profile, "row": observation.row,
                    "different_channels": differences, "max_difference": max
                }))
                .unwrap(),
            )
            .unwrap();
            eprintln!("Saved differing full frames to {}", directory.display());
        }
        assert_eq!(
            differences, 0,
            "{}: {differences} channels/max{max}",
            observation.name
        );
        count += 1;
        eprintln!(
            "wipe_frame={} name={} extent={}x{} differences=0",
            count, observation.name, output.0, output.1
        );
    }
    assert_eq!(count, 200);
    eprintln!(
        "200 byte-identical full720p/1080p wipe frames; literal original0/2/21/22 versus production0..4, nonopaque incoming/outgoing images and explicit gradient Mask fixture"
    );
}
