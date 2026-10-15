//! Mask work starts after phase0's old-wiper wait, before phase1 fast skip.
use crate::{
    asset::{mask::MaskTexture, system::AssetServer},
    wiper::higurashi::derived::HigurashiWiper,
};
use shin_core::format::scenario::{
    higurashi::HigurashiScenario, instruction_elements::UntypedNumberArray,
};
use std::sync::Arc;

/// Explicit test readiness service after the actual decoder completes. The
/// result stays in the original loader scope until readiness or cancellation.
#[cfg(test)]
pub(crate) type ReferenceGate = Box<
    dyn FnOnce(&anyhow::Result<Arc<MaskTexture>>) -> futures::future::BoxFuture<'static, ()> + Send,
>;

enum Mask {
    Unchecked,
    Loading,
    Ready(Option<Arc<MaskTexture>>),
    Failed(String),
}
pub(in crate::adv::higurashi) struct Preparation {
    kind: i32,
    params: UntypedNumberArray,
    services: Option<(Arc<AssetServer>, Arc<HigurashiScenario>)>,
    task: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<MaskTexture>>>>,
    mask: Mask,
    #[cfg(test)]
    pub(in crate::adv::higurashi) reference_gate: Option<ReferenceGate>,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        // Native6208 cancels/joins task78 before releasing prepared handle70.
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
    }
}
impl Preparation {
    pub(in crate::adv::higurashi) fn new(
        kind: i32,
        params: UntypedNumberArray,
        services: Option<(Arc<AssetServer>, Arc<HigurashiScenario>)>,
    ) -> Self {
        Self {
            kind,
            params,
            services,
            task: None,
            mask: Mask::Unchecked,
            #[cfg(test)]
            reference_gate: None,
        }
    }
    pub(in crate::adv::higurashi) fn fault(&self) -> Option<&str> {
        if let Mask::Failed(message) = &self.mask {
            Some(message)
        } else {
            None
        }
    }
    pub(in crate::adv::higurashi) fn animations_enabled(&self) -> bool {
        !matches!(self.mask, Mask::Loading | Mask::Failed(_))
    }
    pub(in crate::adv::higurashi) fn poll(&mut self, duration: i32) -> bool {
        if matches!(self.mask, Mask::Unchecked) {
            if self.kind == 1 && duration > 0 {
                let (assets, scenario) = self
                    .services
                    .as_ref()
                    .expect("Mask wipe preparation needs actual asset services");
                let info = usize::try_from(self.params.0)
                    .ok()
                    .and_then(|i| scenario.mask_info.get(i));
                if let Some(info) = info {
                    let path = format!("/mask/{}.msk", info.name.as_str().to_ascii_lowercase());
                    let assets = assets.clone();
                    #[cfg(test)]
                    let gate = self.reference_gate.take();
                    self.task = Some(shin_tasks::async_io::spawn_loader(async move {
                        let result = assets.load(&path).await;
                        #[cfg(test)]
                        if let Some(gate) = gate {
                            gate(&result).await;
                        }
                        result
                    }));
                    self.mask = Mask::Loading;
                } else {
                    // Actual6408 changes command kind to0 on absent metadata.
                    self.kind = 0;
                    self.mask = Mask::Ready(None);
                }
            } else {
                self.mask = Mask::Ready(None);
            }
        }
        if matches!(self.mask, Mask::Loading) {
            match self.task.as_mut().unwrap().poll_naive() {
                None => return false,
                Some(Ok(mask)) => self.mask = Mask::Ready(Some(mask)),
                Some(Err(error)) => {
                    self.mask = Mask::Failed(format!("Higurashi Mask wipe asset failed: {error:#}"))
                }
            }
        }
        matches!(self.mask, Mask::Ready(_))
    }
    pub(in crate::adv::higurashi) fn construct(
        &self,
        duration: i32,
        fast_skip: bool,
    ) -> Option<HigurashiWiper> {
        if duration <= 0 || fast_skip {
            return None;
        }
        let Mask::Ready(mask) = &self.mask else {
            unreachable!("Only ready WIPE phase1 can publish")
        };
        Some(
            HigurashiWiper::new(self.kind, duration as u32, self.params, mask.clone())
                .expect("Ready Mask preparation retains its asset"),
        )
    }
}
