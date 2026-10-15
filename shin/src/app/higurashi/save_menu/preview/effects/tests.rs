use super::*;

pub(in crate::app::higurashi::save_menu::preview) mod native;

fn properties(changes: &[(usize, f32)]) -> HigurashiProperties {
    let mut properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
    for &(index, value) in changes {
        properties
            .tweener_mut(index)
            .unwrap()
            .fast_forward_to(value);
    }
    properties
}

#[test]
fn higurashi_saved_effect_factory_has_no_derived_entropy_or_phases() {
    for kind in [5, 6, 10] {
        crate::engine_random::with_reference_seed(5489, || {
            let before = crate::engine_random::seed_call_count();
            let prepared = Prepared::new(kind).unwrap();
            let first = prepared.clone_refresh();
            let second = prepared.clone_refresh();
            assert_eq!((first.kind(), second.kind()), (kind, kind));
            assert_eq!((first.phases(), second.phases()), ([0.; 2], [0.; 2]));
            assert_eq!(crate::engine_random::seed_call_count(), before);
            assert_eq!(crate::engine_random::next_seed(), 3499211612);
        });
    }
    for kind in [0, 4, 7, 9, 11] {
        assert!(Prepared::new(kind).is_err());
    }
}

#[test]
fn higurashi_saved_effect_native_resource_and_pass_contracts() {
    let blur = Prepared::new(5).unwrap().clone_refresh();
    let mosaic = Prepared::new(6).unwrap().clone_refresh();
    let raster = Prepared::new(10).unwrap().clone_refresh();
    assert_eq!(
        blur.resource_plan(),
        ResourcePlan {
            extent: (960, 540),
            color_count: 2,
            depth_count: 2,
            shared_depth: false
        }
    );
    for view in [&mosaic, &raster] {
        assert_eq!(
            view.resource_plan(),
            ResourcePlan {
                extent: (1920, 1080),
                color_count: 1,
                depth_count: 0,
                shared_depth: true
            }
        );
    }
    let p = properties(&[]);
    assert!(blur.packet(&p, PassKind::Transparent).unwrap().is_none());
    assert_eq!(
        blur.packet(&p, PassKind::Opaque).unwrap().unwrap().program,
        23
    );
    assert_eq!(
        mosaic
            .packet(&p, PassKind::Opaque)
            .unwrap()
            .unwrap()
            .program,
        6
    );
    assert_eq!(
        mosaic
            .packet(&p, PassKind::Transparent)
            .unwrap()
            .unwrap()
            .program,
        6
    );
    assert_eq!(
        raster
            .packet(&p, PassKind::Opaque)
            .unwrap()
            .unwrap()
            .program,
        24
    );
    assert!(raster.packet(&p, PassKind::Transparent).unwrap().is_none());
    // Original Blur68abc checks these four gates. Mosaic/Raster bypass it.
    for gate in [9, 12, 13, 19] {
        let p = properties(&[(gate, 0.)]);
        assert!(blur.packet(&p, PassKind::Opaque).unwrap().is_none());
        assert!(mosaic.packet(&p, PassKind::Opaque).unwrap().is_some());
        assert!(mosaic.packet(&p, PassKind::Transparent).unwrap().is_some());
        assert!(raster.packet(&p, PassKind::Opaque).unwrap().is_some());
    }
    let clipped = properties(&[(44, 20.)]);
    assert_eq!(
        blur.packet(&clipped, PassKind::Opaque)
            .unwrap()
            .unwrap()
            .program,
        23
    );
    // Native696d0/6a994 clips the final Blur stage in View::render.
    let clip = crate::layer::user::higurashi::leaf_clip::Clip::from_properties(&clipped);
    assert!(clip.active());
    assert_eq!(
        clip.0.map(f32::to_bits),
        [0x41a0_0000, 0, 0x449d_8000, 0x4434_0000]
    );
    assert!(
        blur.packet(&clipped, PassKind::Transparent)
            .unwrap()
            .is_none()
    );
    assert!(mosaic.packet(&clipped, PassKind::Opaque).unwrap().is_some());
    assert!(raster.packet(&clipped, PassKind::Opaque).unwrap().is_some());
    assert!(
        blur.blur_stage0(&properties(&[(49, 0.)])).is_some(),
        "Native never skips the horizontal stage for radius0"
    );
}

#[test]
fn higurashi_saved_effects_retain_direct_mosaic_shader_and_zero_raster_frequency() {
    let mosaic = Prepared::new(6).unwrap().clone_refresh();
    for (operation, program) in [(1, 9), (2, 7), (5, 11)] {
        let p = properties(&[(21, operation as f32), (26, 0.)]);
        let packet = mosaic.packet(&p, PassKind::Transparent).unwrap().unwrap();
        assert_eq!(
            packet.program, program,
            "Original8b584 bypasses69894 identity simplification"
        );
        assert_eq!(packet.native_sampler, Some([0, 0, 1, 0, 0]));
    }
    let raster = Prepared::new(10).unwrap().clone_refresh();
    let packet = raster
        .packet(
            &properties(&[(55, 0.), (58, 0.), (21, 5.)]),
            PassKind::Opaque,
        )
        .unwrap()
        .unwrap();
    assert_eq!(packet.program, 24);
    // Saved65 targets are integer words: zero restores positive zero.
    assert_eq!(packet.param0.z.to_bits(), 0);
    assert_eq!(packet.param0.w.to_bits(), 0);
    assert_eq!(packet.param1, Vec4::ZERO);
}
