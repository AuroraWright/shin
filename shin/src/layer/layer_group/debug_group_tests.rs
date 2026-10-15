use super::*;

impl LayerGroup {
    pub(crate) fn assert_reference_mask_debug_exits(
        context: &mut PreRenderContext,
        server: &Arc<crate::asset::system::AssetServer>,
    ) {
        let mask: Arc<MaskTexture> = shin_tasks::block_on(server.load("/mask/1.msk")).unwrap();
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Masked group debug scopes"),
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
        let mut group: LayerGroup = LayerGroup::new(None);
        let mut cases = 0;
        for alpha in [0.0, 500.0, 1000.0] {
            group
                .props
                .property_tweener_mut(LayerProperty::MulColorAlpha)
                .fast_forward_to(alpha);
            for source_pass in [PassKind::Opaque, PassKind::Transparent] {
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    {
                        let mut pass = context.begin_pass(
                            TextureTarget {
                                view: &view,
                                kind:
                                    shin_render::shaders::types::texture::TextureTargetKind::Screen,
                            },
                            Some(context.depth_stencil),
                            "Masked group submission",
                        );
                        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                        pass.push_debug("Page parent");
                        group.finish_render_with_mask(
                            &mut pass,
                            PrerenderedDrawable {
                                render_texture: mask.texture.as_source(),
                                target_pass: source_pass,
                            },
                            &mask,
                            MaskFlags::SCALE,
                            &TransformParams::default(),
                            1,
                            kind,
                        );
                        pass.pop_debug();
                    }
                    let pixels = crate::layer::user::readback_test_texture(context, &target);
                    if alpha == 0.0
                        || (kind == PassKind::Opaque
                            && (source_pass == PassKind::Transparent || alpha < 1000.0))
                    {
                        assert!(
                            pixels.chunks_exact(4).all(|p| p[..3] == [0, 0, 0]),
                            "Rejected masked pass must preserve the background"
                        );
                    }
                    cases += 1;
                }
            }
        }
        println!(
            "Masked group submission: {cases} cases, including invisible and rejected opaque exits"
        );
    }
}
