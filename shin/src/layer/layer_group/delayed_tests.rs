use super::*;
use crate::layer::{render_layer_without_bg, user::TileLayer};
use shin_core::primitives::update::FrameId;
use shin_render::shaders::types::RenderClone;

fn tile(color: FloatColor4) -> TileLayer {
    TileLayer::new(color, glam::vec4(-960.0, -540.0, 1920.0, 1080.0))
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
fn reference_native_delayed_removal_retention_countdown_and_fast_forward() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            (
                u64::from_le_bytes(executable[offset + 256..offset + 264].try_into().unwrap()),
                u64::from_le_bytes(executable[offset + 272..offset + 280].try_into().unwrap()),
            )
        })
        .collect();
    assert_eq!(relocations.get(&0x1d0e50), Some(&0xa120));
    assert_eq!(relocations.get(&0x1d0e60), Some(&0x163cb0));
    assert_eq!(relocations.get(&0x1daf30), Some(&0x171eb1));
    assert_eq!(relocations.get(&0x1daf38), Some(&0x1d7c58));
    assert_eq!(relocations.get(&0x224518), Some(&0x1d7cc8));
    for (address, expected) in [
        (0xa254, 0xb9401e62u32),
        (0xa258, 0x9401c48e),
        (0xa290, 0x320003e0),
        (0x7b500, 0x34000622),
        (0x7b504, 0x1e220040),
        (0x7b510, 0x12800008),
        (0x7b518, 0xb90003e8),
        (0x7b520, 0xbd0007e0),
        (0x7b6e4, 0xf9403261),
        (0x7b6f4, 0x97fffdb3),
        (0x7bcf4, 0xf9400908),
        (0x7bd00, 0xd63f0100),
        (0x7bd40, 0x36f804a8),
        (0x7bd48, 0x1e283800),
        (0x7bd4c, 0x1e294800),
        (0x7bd50, 0xbd0006e0),
        (0x7bd58, 0x1e2a2000),
        (0x7bd5c, 0x540003c5),
        (0x7bdac, 0x91016108),
        (0x7bdb0, 0xc8dffd19),
        (0x7bdd0, 0xb40000b9),
        (0x7baf4, 0xb9400288),
        (0x7baf8, 0x37f805e8),
        (0x7bb30, 0xb9400129),
        (0x7bb34, 0x37f80269),
        (0x79d10, 0xb9400348),
        (0x79d18, 0xb9400748),
        (0x159ab8, 0x34000000),
        (0x81274, 0x97ffcdcf),
        (0x81348, 0x97ffcd9a),
        (0x81380, 0x6b14027f),
        (0x81388, 0x1a9fa7e8),
        (0x81394, 0x1e212000),
        (0x81398, 0x1a894113),
        (0x749b0, 0xb9402000),
        (0x8129c, 0xbd49f2a8),
        (0x812a0, 0x32007bf3),
        (0x1599f0, 0xff7fffff),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native word{address:x}"
        );
    }
}

#[test]
fn delayed_removal_frees_bank_lookup_and_preserves_anonymous_entries() {
    let bank = LayerbankId::new(3);
    let other = LayerbankId::new(7);
    let mut group = LayerGroup::new(None);
    group.add_layer(bank, tile(FloatColor4::BLUE));
    group.add_layer(other, tile(FloatColor4::WHITE));
    assert!(group.remove_layer(bank, Ticks::from_f32(3.0)).is_none());
    assert!(group.get_layer(bank).is_none());
    assert_eq!(group.get_used_layerbank_ids(), vec![other]);
    assert_eq!(group.layers.len(), 2);
    group.add_layer(bank, tile(FloatColor4::RED));
    assert_eq!(group.layers.len(), 3);
    assert_eq!(group.get_used_layerbank_ids(), vec![bank, other]);
    group.remove_layer(bank, Ticks::from_f32(-2.0));
    assert_eq!(group.layers.len(), 3);
    assert!(
        group.layers[..2]
            .iter()
            .all(|item| item.layerbank_id.is_none())
    );
    assert_eq!(group.layers[0].removal_delay, Ticks::from_f32(-2.0));
    assert_eq!(group.layers[1].removal_delay, Ticks::from_f32(3.0));
    assert!(group.remove_layer(bank, Ticks::from_f32(9.0)).is_none());
    assert!(group.remove_layer(other, Ticks::ZERO).is_some());
    assert!(group.needs_rendering());
    group.fast_forward();
    assert!(!group.needs_rendering());
}

fn pixel(context: &mut PreRenderContext, group: &mut LayerGroup<TileLayer>) -> [u8; 4] {
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Delayed layer retention"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    group.pre_render(context, &TransformParams::default());
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "Delayed layer retention",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        render_layer_without_bg(&mut pass, &TransformParams::default(), group, 0);
    }
    let bytes = crate::layer::user::readback_test_texture(context, &target);
    bytes[(540 * 1920 + 960) * 4..(540 * 1920 + 960) * 4 + 4]
        .try_into()
        .unwrap()
}

impl LayerGroup<TileLayer> {
    pub(crate) fn reference_delayed_removal(
        context: &mut PreRenderContext,
        update: &mut AdvUpdateContext,
    ) {
        let bank = LayerbankId::new(0);
        let mut group = Self::new(None);
        // Retention frees the bank but preserves the drawable's logical ID.
        // Give the retained blue layer explicit priority over the new red
        // layer; native ordering uses this ID, not the anonymous bank sentinel.
        let mut blue = tile(FloatColor4::BLUE);
        blue.properties_mut()
            .set_layer_id(shin_core::vm::command::types::LayerId::new(7));
        group.add_layer(bank, blue);
        group.remove_layer(bank, Ticks::from_f32(3.0));
        group.add_layer(bank, tile(FloatColor4::RED));
        assert_eq!(pixel(context, &mut group), [0, 0, 255, 255]);
        update.frame_id = FrameId::default();
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(1.0);
        update.are_animations_allowed = false;
        group.update(update);
        assert_eq!(group.layers[0].removal_delay, Ticks::from_f32(2.0));
        update.delta_ticks = Ticks::from_f32(100.0);
        group.update(update);
        assert_eq!(group.layers[0].removal_delay, Ticks::from_f32(2.0));
        let mut copy = group.render_clone(&mut context.render_clone_ctx());
        update.delta_ticks = Ticks::from_f32(1.0);
        copy.update(update);
        assert_eq!(copy.layers[0].removal_delay, Ticks::from_f32(1.0));
        assert_eq!(group.layers[0].removal_delay, Ticks::from_f32(2.0));
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(2.0);
        group.update(update);
        assert_eq!(pixel(context, &mut group), [255, 0, 0, 255]);
        assert_eq!(pixel(context, &mut copy), [0, 0, 255, 255]);
        copy.update(update);
        assert_eq!(pixel(context, &mut copy), [255, 0, 0, 255]);
        let mut wiping = Self::new(None);
        wiping.add_layer(bank, tile(FloatColor4::RED));
        wiping.add_layer_with_wiper(
            bank,
            tile(FloatColor4::BLUE),
            Some(crate::wiper::AnyWiper::Default(
                crate::wiper::DefaultWiper::new(Ticks::from_f32(10.0)),
            )),
        );
        wiping.remove_layer(bank, Ticks::from_f32(1.0));
        update.delta_ticks = Ticks::from_f32(1.0);
        wiping.update(update);
        assert_eq!(wiping.layers.len(), 1);
        assert_eq!(wiping.layers[0].removal_delay, Ticks::ZERO);
        assert!(wiping.layers[0].layer.is_transition_active());
        assert!(wiping.get_layer(bank).is_none());
        // Countdown advances while animations are suspended; the wipe's own
        // timer waits for animation permission before it can finish.
        update.are_animations_allowed = true;
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(8.0);
        wiping.update(update);
        assert!(wiping.needs_rendering());
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(2.0);
        wiping.update(update);
        assert!(
            !wiping.needs_rendering(),
            "Expired entry retires after child wipe completion in the same update"
        );
        for delay in [-2.0, 1.0] {
            let mut signed = Self::new(None);
            signed.add_layer(bank, tile(FloatColor4::BLUE));
            signed.remove_layer(bank, Ticks::from_f32(delay));
            update.frame_id.advance();
            update.delta_ticks = Ticks::from_f32(-3.0);
            signed.update(update);
            assert_eq!(signed.layers[0].removal_delay, Ticks::from_f32(delay + 3.0));
            update.frame_id.advance();
            update.delta_ticks = Ticks::from_f32(10.0);
            signed.update(update);
            assert!(!signed.needs_rendering());
        }
        let mut boundary = Self::new(None);
        boundary.add_layer(bank, tile(FloatColor4::BLUE));
        boundary.remove_layer(bank, Ticks::from_f32(1.0));
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(1.0 - f32::EPSILON);
        boundary.update(update);
        assert!(
            boundary.needs_rendering(),
            "Native uses strict less-than epsilon"
        );
        update.frame_id.advance();
        update.delta_ticks = Ticks::from_f32(f32::EPSILON * 0.5);
        boundary.update(update);
        assert!(!boundary.needs_rendering());
        eprintln!(
            "Native delayed removal: anonymous bank reuse and blue/red pixels, copied independent countdowns, duplicate frame suppression, animation pause, wipe completion, signed delta and strict epsilon pass"
        );
    }
}
