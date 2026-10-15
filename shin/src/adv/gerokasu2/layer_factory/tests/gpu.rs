//! Actual basic request acquisition, typed ownership and shared drawable output.
use super::*;
use crate::{
    asset::{
        font::gerokasu2::draw::gpu_tests::Offscreen,
        system::{AssetLoadContext, locate_assets_for_game},
    },
    cli::Game,
    layer::{DrawableLayer, Layer, render_params::TransformParams},
};
use shin_core::vm::command::types::LayerProperty;
use shin_render::PassKind;

#[test]
#[ignore = "requires supplied ROM,Metal and exact Loopback Audio"]
fn basic_gerokasu2_factories_acquire_retained_utf8_assets_and_draw() {
    let gpu = Offscreen::new();
    let scenario = scenario();
    let path = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap());
    let assets = Arc::new(AssetServer::new(
        locate_assets_for_game(Game::Gerokasu2, Some(path.parent().unwrap()))
            .unwrap()
            .into(),
        AssetLoadContext {
            game: crate::cli::Game::Gerokasu2,
            wgpu_device: gpu.device.clone(),
            wgpu_queue: gpu.queue.clone(),
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            higurashi_bustup_cache: Default::default(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let mut frames = 0;
    let mut channels = 0;
    for kind in 0..=3 {
        let params = if kind == 1 {
            (0xf369, -960, -540, 1920, 1080, 0, 0, 0)
        } else {
            (0, 0, 0, 0, 0, 0, 0, 0)
        };
        let request = LayerFactoryRequest::resolve(&scenario, kind, params).unwrap();
        let prepared =
            shin_tasks::block_on(PreparedBasicDrawable::load(&assets, &request)).unwrap();
        let mut drawable = prepared.construct();
        if kind == 3 {
            // Retain the typed outer owner, keeping the UTF8 registry/cache key
            // alive with the shared Bustup implementation's inner resource.
            let owner = drawable.retained_bustup().unwrap();
            assert!(!owner.0.base_blocks.is_empty());
            assert!(
                owner.0.face1.is_some()
                    || owner.0.face2.is_some()
                    || !owner.0.mouth_blocks.is_empty()
            );
            let again =
                shin_tasks::block_on(PreparedBasicDrawable::load(&assets, &request)).unwrap();
            let PreparedBasicDrawable::Bustup(other, _) = again else {
                panic!()
            };
            assert!(Arc::ptr_eq(owner, &other));
            let info = &scenario.bustup_info[0];
            for (property, value) in [
                (LayerProperty::TranslateX, -960.0),
                (LayerProperty::TranslateY, 540.0),
                (
                    LayerProperty::NegatedTranslationX,
                    f32::from(info.raw_placement[0]),
                ),
                (
                    LayerProperty::NegatedTranslationY,
                    f32::from(info.raw_placement[1]),
                ),
                (LayerProperty::ScaleX, f32::from(info.raw_placement[2])),
                (LayerProperty::ScaleY, f32::from(info.raw_placement[2])),
            ] {
                drawable
                    .layer
                    .properties_mut()
                    .property_tweener_mut(property)
                    .fast_forward_to(value);
            }
        }
        let pixels = crate::adv::gerokasu2::message_ui::reference_frame(
            &gpu.device,
            &gpu.queue,
            (640, 360),
            false,
            |pass| {
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    drawable
                        .layer
                        .render(pass, &TransformParams::default(), 3, kind);
                }
            },
        );
        frames += 1;
        channels += pixels.len();
        let visible = pixels.chunks_exact(4).any(|p| p != [96, 136, 184, 255]);
        assert_eq!(visible, kind != 0, "basic kind{kind}");
    }
    let null = LayerFactoryRequest::resolve(&scenario, 3, (-1, 0, 0, 0, 0, 0, 0, 0)).unwrap();
    assert!(matches!(null, LayerFactoryRequest::Null));
    assert!(matches!(
        shin_tasks::block_on(PreparedBasicDrawable::load(&assets, &null)).unwrap(),
        PreparedBasicDrawable::Null
    ));
    let low = LayerFactoryRequest::resolve(&scenario, 3, (0, 1, 0, 0, 0, 0, 0, 0)).unwrap();
    assert!(
        shin_tasks::block_on(PreparedBasicDrawable::load(&assets, &low)).is_err(),
        "Missing L source must not silently use M"
    );
    let card = LayerFactoryRequest::resolve(&scenario, 9, (1, 2, 3, 0, 0, 0, 0, 0)).unwrap();
    assert!(matches!(card, LayerFactoryRequest::CardSelect { .. }));
    assert!(
        shin_tasks::block_on(PreparedBasicDrawable::load(&assets, &card)).is_err(),
        "Ger type9 must not construct an Umineko interactive layer"
    );
    println!(
        "GEROKASU2_BASIC_FACTORY_GPU kinds=4 frames={frames} channels={channels} actual_PIC_UTF8_BUP_typed_owner_cache_null_quality_no_fallback=verified audio=Loopback Audio"
    );
}
