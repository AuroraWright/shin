//! Default transition owns the outgoing front until its visual clock ends.
//! Group texture preparation and the command scheduler supply its render
//! inputs and advancement; this owner does not consume VM commands itself.
use shin_core::time::Ticks;
use shin_render::{
    RenderRequestBuilder, render_pass::RenderPass, shaders::types::texture::TextureSource,
};

use super::layers::{HigurashiLayers, HigurashiPreviousFront};
use crate::wiper::higurashi::derived::HigurashiWiper;
pub(super) mod preparation;

pub struct HigurashiDefaultTransition {
    pub previous: HigurashiPreviousFront,
    wiper: HigurashiWiper,
    flags: u32,
}

impl HigurashiDefaultTransition {
    /// Native 65a4 skips effect construction for signed duration <=0 or fast
    /// skip. Positive durations reach ec930's unsigned constructor.
    pub fn begin(layers: &mut HigurashiLayers, duration: i32, flags: u32, fast_skip: bool) -> Self {
        Self::from_previous(layers.commit_back(), duration, flags, fast_skip)
    }
    pub(super) fn from_previous(
        previous: HigurashiPreviousFront,
        duration: i32,
        flags: u32,
        fast_skip: bool,
    ) -> Self {
        Self {
            previous,
            flags,
            wiper: HigurashiWiper::default(if duration > 0 && !fast_skip {
                duration as u32
            } else {
                0
            }),
        }
    }
    pub(super) fn from_wiper(
        previous: HigurashiPreviousFront,
        wiper: HigurashiWiper,
        flags: u32,
    ) -> Self {
        Self {
            previous,
            wiper,
            flags,
        }
    }
    pub fn advance(&mut self, delta: Ticks) -> bool {
        self.wiper.advance(delta)
    }
    pub fn fast_forward(&mut self) {
        self.wiper.fast_forward();
    }
    pub fn is_running(&self) -> bool {
        self.wiper.is_running()
    }
    pub fn progress(&self) -> f32 {
        self.wiper.progress()
    }
    /// WIPE phase2 6950 tests bit0. WIPEWAIT always waits for root retirement.
    pub fn command_complete(&self) -> bool {
        self.flags & 1 != 0 || !self.is_running()
    }
    pub fn wait_complete(&self) -> bool {
        !self.is_running()
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        incoming: TextureSource,
        outgoing: TextureSource,
    ) {
        self.wiper.render(pass, builder, incoming, outgoing);
    }
}
