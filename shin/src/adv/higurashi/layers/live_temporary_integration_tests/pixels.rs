use super::gpu::Shaders;
use super::*;
use crate::{
    adv::characters::view::CharacterTextures,
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
    layer::render_params::higurashi::HigurashiTransform,
};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    PassKind,
    shaders::types::texture::{TextureTarget, TextureTargetKind},
};

fn cases(append: bool) -> Vec<Case> {
    let mut cases = vec![];
    for (index, ids) in [
        vec![2, 3, 2],
        vec![0, 17, 33, 244, 314, 17],
        vec![2, 3, 2],
        vec![0, 17, 33, 244, 314, 17],
    ]
    .into_iter()
    .enumerate()
    {
        let mut params = [0; 8];
        params[0] = i32::from(index >= 2);
        params[1] = ids.len() as i32;
        params[2..2 + ids.len()].copy_from_slice(&ids);
        cases.push(Case {
            kind: 12,
            old: index % 2 != 0,
            append,
            route: false,
            pattern: if index % 2 == 0 { 2 } else { 4 },
            params,
        });
    }
    for (index, id) in [0, 6, 7, 27].into_iter().enumerate() {
        cases.push(Case {
            kind: 13,
            old: index % 2 != 0,
            append,
            route: false,
            pattern: 2,
            params: [id, 0, 0, 0, 0, 0, 0, 0],
        });
    }
    for (index, count) in [1, 3, 6, 7].into_iter().enumerate() {
        let mut params = [0; 8];
        params[0] = count;
        params[1..].copy_from_slice(&[0, 2, 33, 2, 127, 159, 9]);
        cases.push(Case {
            kind: 14,
            old: index % 2 != 0,
            append,
            route: false,
            pattern: 2,
            params,
        });
    }
    cases
}
fn images(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layer: &HigurashiUserLayer,
    native: &Capture,
    atlas: &CharacterTextures,
    shaders: &Shaders,
    extent: (u32, u32),
) -> (Vec<u8>, Vec<u8>) {
    let background = UnormColor::from_rgba(173, 127, 91, 113);
    let actual = reference_render_to_texture(device, queue, extent, |context, target| {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "Live11..14 publication pixels",
        );
        pass.clear(Some(background), Some(0), Some(1.));
        for kind in [PassKind::Opaque, PassKind::Transparent] {
            layer
                .render(&mut pass, &HigurashiTransform::default(), 7, kind)
                .unwrap();
        }
    });
    let original = reference_render_to_texture(device, queue, extent, |context, target| {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Original live widgets background",
            );
            pass.clear(Some(background), Some(0), Some(1.));
        }
        for draw in &native.draws {
            let texture = if draw.program == 2 {
                let name = native.names.get(&draw.texture).unwrap();
                Some(atlas.get(name).as_source())
            } else {
                None
            };
            shaders.draw(context, target, draw, texture);
        }
    });
    (actual, original)
}
fn compare_pixels(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let differences = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let max = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert_eq!(
        differences, 0,
        "{label}:literal native RGBA differs,max{max}"
    );
    let colors = actual
        .chunks_exact(4)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        colors.len() >= 3,
        "{label}:source artwork must remain visibly nonuniform"
    );
}
#[test]
#[ignore = "48 moving live12..14 frames at full720p/1080p,real tipsget/otsuget/kakeraget art,literal native programs1/2 raw WGPU; no production expectedshader/audio/windows/user saves"]
fn higurashi_live_temporary_moving_real_art_pixels_match_original() {
    let (device, queue, assets) = reference_gpu_assets();
    let scenario = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let cases = cases(assets.contains("/append.txt"));
    assert_eq!(cases.len(), 12);
    let mut input = String::new();
    for c in &cases {
        input_new(&mut input, c);
        input.push_str("P 0\nU 0 55\nR 0\nC 0 1\nU 1 40\nR 1\nD 0\nD 1\n");
    }
    let mut original = native(input, "moving-pixels").into_iter();
    let shaders = Shaders::new(&device);
    let mut frames = 0;
    let mut packet_count = 0;
    let mut changed = 0;
    let mt = crate::engine_random::seed_call_count();
    for (index, c) in cases.iter().enumerate() {
        let label = format!("live temporary moving{index} {c:?}");
        let mut settings = settings(c);
        let prepared = shin_tasks::block_on(Prepared::load(
            &assets,
            &scenario,
            Kind::from_native(c.kind).unwrap(),
            array(c.params),
            0,
        ))
        .unwrap();
        let old = c.old.then(|| {
            let mut old = old_layer();
            old.advance(Ticks::from_f32(2.5), None);
            old
        });
        let mut layer = prepared
            .construct_with_settings(old.as_ref(), (!c.old).then_some(SEEDS), Some(&mut settings))
            .unwrap()
            .unwrap();
        compare(&layer, &snapshot(original.next().unwrap()), &label);
        let mut wrapper = layer.reference_temporary().unwrap().retain();
        wrapper.poll_phase();
        compare(&layer, &snapshot(original.next().unwrap()), &label);
        layer.advance(Ticks::from_f32(55.), None);
        compare(&layer, &snapshot(original.next().unwrap()), &label);
        let Original::Render(first) = original.next().unwrap() else {
            panic!("Original moving capture")
        };
        compare(&layer, &snapshot(original.next().unwrap()), &label);
        let mut copy = layer.native_copy().unwrap();
        compare(&copy, &snapshot(original.next().unwrap()), &label);
        copy.advance(Ticks::from_f32(40.), None);
        compare(&copy, &snapshot(original.next().unwrap()), &label);
        let Original::Render(second) = original.next().unwrap() else {
            panic!("Original copied capture")
        };
        compare(&copy, &snapshot(original.next().unwrap()), &label);
        assert!(matches!(original.next(), Some(Original::Drop)));
        assert!(matches!(original.next(), Some(Original::Drop)));
        assert_eq!(settings.save_vectors.extra_words_0, expected_bits(c));
        let archive = match c.kind {
            12 => "/tipsget.txa",
            13 => "/otsuget.txa",
            14 => "/kakeraget.txa",
            _ => unreachable!(),
        };
        let atlas: Arc<CharacterTextures> = shin_tasks::block_on(assets.load(archive)).unwrap();
        // The native notification foreground/black layer ignores generic base
        // alpha. Test the root's early temporary dispatch using that same
        // original packet set under alpha0; XY/private clocks stay unchanged.
        layer.properties.tweener_mut(9).unwrap().fast_forward_to(0.);
        copy.properties.tweener_mut(9).unwrap().fast_forward_to(0.);
        for extent in [(1280, 720), (1920, 1080)] {
            let (a, e) = images(&device, &queue, &layer, &first, &atlas, &shaders, extent);
            compare_pixels(&a, &e, &format!("{label}/55ticks/{extent:?}"));
            let (b, e) = images(&device, &queue, &copy, &second, &atlas, &shaders, extent);
            compare_pixels(&b, &e, &format!("{label}/95ticks copy/{extent:?}"));
            let differences = a.iter().zip(&b).filter(|(a, b)| a != b).count();
            if differences != 0 {
                changed += 1;
            }
            assert!(
                differences != 0,
                "{label}:actual moving clocks must change visible RGBA at{extent:?}"
            );
            frames += 2;
        }
        packet_count += first.draws.len() + second.draws.len();
    }
    assert!(original.next().is_none());
    assert_eq!(frames, 48);
    assert_eq!(changed, 24);
    assert_eq!(crate::engine_random::seed_call_count(), mt);
    eprintln!(
        "Live temporary moving pixels:{frames} exact full720p/1080p frames,{packet_count} original programs1/2 packets,24 visibly changing pairs,all12 realart owners at alpha0; independent original GLSL/rawblend/vertices/projection, no audio/window/save initialization."
    );
}
