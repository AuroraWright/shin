use super::*;
use crate::{layer::user::TileLayer, wiper::native_batch_tests};
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

fn tile(id: usize, position: f32) -> TileLayer {
    let i = id as f32;
    let mut layer = TileLayer::new(
        FloatColor4::from_rgba(0.125 * i, 0.25, 0.5, 1.0),
        // Larger overlapping shapes exercise spatial wipes throughout the
        // frame instead of checking only the small native factory rectangle.
        glam::vec4(-800.0 + 80.0 * i, -440.0 + 64.0 * i, 1200.0, 640.0),
    );
    layer
        .properties_mut()
        .property_tweener_mut(LayerProperty::RenderPosition)
        .fast_forward_to(position);
    layer
}

fn render_group(
    context: &mut PreRenderContext,
    view: &wgpu::TextureView,
    group: &LayerGroup<TileLayer>,
) {
    let mut pass = context.begin_pass(
        TextureTarget {
            view,
            kind: TextureTargetKind::Screen,
        },
        Some(context.depth_stencil),
        "Wipe batch composition",
    );
    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
    crate::layer::render_layer_without_bg(&mut pass, &TransformParams::default(), group, 0);
}

pub(super) fn run(
    context: &mut PreRenderContext,
    view: &wgpu::TextureView,
    target: &wgpu::Texture,
) {
    let masks = native_batch_tests::reference_masks(context.device, context.queue);
    let mut profiles = (0..10)
        .flat_map(|kind| {
            [
                (kind, true, false, false),
                (kind, true, true, false),
                (kind, false, false, false),
            ]
        })
        .collect::<Vec<_>>();
    // Native Scanline/Glass equality does not compare elapsed or duration.
    // The first wipe's clocks render the entire prepared batch in these cases.
    profiles.extend([(6, true, false, true), (9, true, false, true)]);
    let mut checked_pixels = 0;
    let mut previous_composition_mismatches = 0;
    for (kind, source_present, ordinary_upper, distinct_timer) in profiles {
        let words = native_batch_tests::render_words(kind, if source_present { 7.5 } else { 22.5 });
        let mut second_words = words;
        if distinct_timer {
            second_words[0] = 15.0f32.to_bits();
            second_words[1] = 60.0f32.to_bits();
        }
        let mut group = LayerGroup::new(None);
        let mut old = LayerGroup::new(None);
        let mut new = LayerGroup::new(None);
        for (bank, old_id, new_id, position) in [
            (1, 1, 2, 2000.0),
            (5, 3, if ordinary_upper { 3 } else { 4 }, 1000.0),
        ] {
            let bank = LayerbankId::new(bank);
            let has_source = old_id == 3 || source_present;
            if has_source {
                group.add_layer(bank, tile(old_id, position));
                old.add_layer(bank, tile(old_id, position));
            }
            new.add_layer(bank, tile(new_id, position));
            if old_id == 3 && ordinary_upper {
                continue;
            }
            group.add_layer_with_wiper(
                bank,
                tile(new_id, position),
                Some(native_batch_tests::wipe(
                    kind,
                    if old_id == 1 { &words } else { &second_words },
                    &masks,
                )),
            );
        }

        // Explicit independent semantic old/new owners, with no transition
        // slots. Compose each scene, then run the first wipe exactly once.
        let mut old_texture =
            context.new_render_texture("Native batch independent old scene".into());
        let mut new_texture =
            context.new_render_texture("Native batch independent new scene".into());
        for (scene, texture) in [(&mut old, &mut old_texture), (&mut new, &mut new_texture)] {
            scene.pre_render(context, &TransformParams::default());
            let target = texture.as_texture_target();
            render_group(context, target.view, scene);
        }
        let expected_wipe = native_batch_tests::wipe(kind, &words, &masks);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Independent aggregate wipe",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            expected_wipe.render(
                &mut pass,
                RenderRequestBuilder::new().depth_stencil_shorthand(1, true, false),
                new_texture.as_texture_source(),
                old_texture.as_texture_source(),
            );
        }
        let expected = crate::layer::user::readback_test_texture(context, target);

        // Reproduce the prior per-slot preparation for non-Default types. This
        // uses the same semantic owners and render path, without folding them.
        if kind != 0 {
            group.layers[0]
                .layer
                .pre_render_with_lower(context, &TransformParams::default(), &[]);
            let (left, right) = group.layers.split_at_mut(1);
            right[0].layer.pre_render_with_lower(
                context,
                &TransformParams::default(),
                &[&left[0].layer],
            );
            group.layers_to_render = vec![
                crate::layer::layer_group::LayerRenderItem {
                    source: crate::layer::layer_group::LayerRenderSource::Entry(0),
                    stencil_ref_relative: 1,
                },
                crate::layer::layer_group::LayerRenderItem {
                    source: crate::layer::layer_group::LayerRenderSource::Entry(1),
                    stencil_ref_relative: 0,
                },
            ];
            group.stencil_bump = group
                .layers
                .iter()
                .map(|i| i.layer.get_stencil_bump())
                .sum();
            render_group(context, view, &group);
            let prior = crate::layer::user::readback_test_texture(context, target);
            previous_composition_mismatches += prior
                .chunks_exact(4)
                .zip(expected.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
        }

        group.pre_render(context, &TransformParams::default());
        assert_eq!(
            group.layers_to_render.len(),
            1,
            "wipe{kind} must form one native batch"
        );
        render_group(context, view, &group);
        let actual = crate::layer::user::readback_test_texture(context, target);
        let mismatches = actual
            .chunks_exact(4)
            .zip(expected.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            mismatches, 0,
            "wipe{kind}: source={source_present}, ordinary={ordinary_upper}, distinct_timer={distinct_timer}"
        );
        checked_pixels += actual.len() / 4;
        eprintln!(
            "Wipe{kind} batch: source={source_present}, ordinary={ordinary_upper}, distinct_timer={distinct_timer};zero pixel mismatches"
        );
    }
    assert!(previous_composition_mismatches > 0);
    eprintln!(
        "All native wipe batches: 32 profiles; {checked_pixels} RGBA pixels; zero mismatches; prior per-slot composition differs at {previous_composition_mismatches} pixels"
    );
}
