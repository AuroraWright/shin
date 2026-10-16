//! Native Murderer live/frozen capture targets and canonical presentation.
use super::*;
use shin_render::render_texture::RenderTexture;

impl MurdererAccusationImpl {
    pub(super) fn capture_child(
        &self,
        context: &mut PreRenderContext,
        holder: &mut RenderTextureHolder,
        model: &MurdererModel,
        transform: &TransformParams,
    ) {
        let canvas = context.resize_source.canvas_handle().get();
        let initialize = holder
            .get()
            .is_none_or(|texture| texture.dimensions() != (canvas.width, canvas.height));
        let target = holder.get_or_init(context).as_texture_target();
        let mut pass = context.begin_pass(
            target,
            Some(context.depth_stencil),
            "Murderer child capture",
        );
        // Native9b250/8cb60 clear depth+stencil, then only depth before each
        // child pass. Keep prior color on redraw. Black initializes fresh port
        // storage; native NVN allocation contents are outside this guarantee.
        pass.clear(initialize.then_some(UnormColor::BLACK), Some(0), Some(1.0));
        let neutral = DrawableParams {
            color_multiplier: FloatColor4::WHITE,
            blend_type: LayerBlendType::Type1,
            fragment_shader: LayerFragmentShader::Default,
            shader_param: Vec4::ONE,
        };
        let clip = DrawableClipParams {
            mode: crate::layer::render_params::DrawableClipMode::None,
            area: Vec4::ZERO,
        };
        for kind in [PassKind::Opaque, PassKind::Transparent] {
            pass.clear(None, None, Some(1.0));
            self.render_model(model, &mut pass, transform, &neutral, &clip, 1, kind);
        }
    }

    pub(super) fn present_capture(
        &self,
        context: &mut PreRenderContext,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        capture: &RenderTexture,
    ) {
        let mut pass =
            context.begin_pass(target, Some(depth_stencil), "Murderer captured viewport");
        pass.clear(None, Some(0), None);
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .render_sprite(
                &mut pass,
                RenderRequestBuilder::new()
                    .depth_stencil_shorthand(1, true, false)
                    .color_blend_type(ColorBlendType::Opaque),
                capture.as_texture_source(),
                crate::render::top_left_projection_matrix(),
            );
    }
}
