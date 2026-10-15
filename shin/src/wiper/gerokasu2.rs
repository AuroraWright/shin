//! Later engine transition requests, using Gerokasu2 UTF-8 metadata directly.
use super::*;
use shin_core::format::scenario::gerokasu2::Gerokasu2Scenario;
use shin_core::format::scenario::instruction_elements::FromNumber;

#[derive(Debug, Clone)]
pub struct TransitionRequest {
    pub duration: Ticks,
    pub kind: TransitionKind,
}

#[derive(Debug, Clone)]
pub enum TransitionKind {
    Default,
    Mask {
        path: String,
        parameter: MaskParam,
        flags: MaskFlags,
    },
    Scroll {
        incoming: i32,
        outgoing: i32,
    },
    Zoom {
        incoming: i32,
        outgoing: i32,
    },
    Turn(i32),
    Wave,
    Scanline(i32),
    Ripple,
    Whirl(i32),
    Glass,
}

impl TransitionRequest {
    pub fn resolve(
        scenario: &Gerokasu2Scenario,
        kind: i32,
        duration: i32,
        params: [i32; 8],
    ) -> Option<Self> {
        let kind = match kind {
            0 => TransitionKind::Default,
            1 => {
                let mask = usize::try_from(params[0])
                    .ok()
                    .and_then(|id| scenario.mask_info.get(id));
                match mask {
                    Some(mask) => TransitionKind::Mask {
                        path: format!("/mask/{}.msk", mask.name.as_str()),
                        parameter: MaskParam::from_number(params[1]),
                        flags: MaskFlags::from_bits_retain(params[2]),
                    },
                    None => TransitionKind::Default,
                }
            }
            2 => TransitionKind::Scroll {
                incoming: params[0],
                outgoing: params[1],
            },
            3 => TransitionKind::Zoom {
                incoming: params[0],
                outgoing: params[1],
            },
            4 => TransitionKind::Turn(params[0]),
            5 => TransitionKind::Wave,
            6 => TransitionKind::Scanline(params[0]),
            7 => TransitionKind::Ripple,
            8 => TransitionKind::Whirl(params[0]),
            9 => TransitionKind::Glass,
            _ => return None,
        };
        Some(Self {
            duration: Ticks::from_f32(duration as f32),
            kind,
        })
    }

    /// The CPU comparison captures factory arguments. Shared rendering is
    /// reused here; full later-engine effect/GPU/Glass RNG parity is separate.
    pub async fn load(&self, assets: &AssetServer) -> anyhow::Result<AnyWiper> {
        let duration = self.duration;
        Ok(match &self.kind {
            TransitionKind::Default => DefaultWiper::new(duration).into(),
            TransitionKind::Mask {
                path,
                parameter,
                flags,
            } => {
                let mask: std::sync::Arc<MaskTexture> = assets.load(path).await?;
                MaskWiper::new(duration, mask, *parameter, *flags).into()
            }
            TransitionKind::Scroll { incoming, outgoing } => {
                scroll::ScrollWiper::new(duration, *incoming, *outgoing).into()
            }
            TransitionKind::Zoom { incoming, outgoing } => {
                zoom::ZoomWiper::new(duration, *incoming, *outgoing).into()
            }
            TransitionKind::Turn(direction) => turn::TurnWiper::new(duration, *direction).into(),
            TransitionKind::Wave => wave::WaveWiper::new(duration).into(),
            TransitionKind::Scanline(exponent) => {
                scanline::ScanlineWiper::new(duration, *exponent).into()
            }
            TransitionKind::Ripple => ripple::RippleWiper::new(duration).into(),
            TransitionKind::Whirl(direction) => {
                whirl::WhirlWiper::new(duration, *direction, 1.0).into()
            }
            TransitionKind::Glass => glass::GlassWiper::new(duration).into(),
        })
    }
}

#[cfg(test)]
mod tests;
