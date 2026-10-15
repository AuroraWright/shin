//! Original1db58/1e160 LayerReloader. Integer records and unrelated owners stay
//! in place; factories are prepared asynchronously and installed at one poll.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReloadKind {
    Front,
    Back,
    CopyFront,
}
#[derive(Clone, Debug)]
pub(crate) struct ReloadJob {
    pub slot: u8,
    pub kind: ReloadKind,
    record: Record,
}
pub(crate) struct ReloadPlan {
    pub jobs: Vec<ReloadJob>,
}
pub(crate) struct LoadedReload {
    jobs: Vec<(ReloadJob, Option<HigurashiUserLayer>)>,
}

impl LayerBanks {
    pub(crate) fn reload_plan(&self) -> ReloadPlan {
        let mut jobs = Vec::new();
        for slot in 0..SLOTS {
            let mut add = |bank: usize, kind| {
                jobs.push(ReloadJob {
                    slot: slot as u8,
                    kind,
                    record: self.records[slot][bank].clone(),
                });
            };
            if self.records[slot][0].layer_type() & !1 == 2 {
                add(0, ReloadKind::Front);
            }
            if self.records[slot][1].layer_type() & !1 == 2 && self.dirty[slot][0] != 0 {
                add(
                    1,
                    if self.dirty[slot][1] != 0 {
                        ReloadKind::Back
                    } else {
                        ReloadKind::CopyFront
                    },
                );
            }
        }
        ReloadPlan { jobs }
    }
}
impl ReloadPlan {
    pub(crate) async fn load(
        self,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        setting: u8,
    ) -> Result<LoadedReload> {
        // Start every requested asset future before waiting for all results.
        // Temporary clocks never reach the live group. Native factories create
        // their typed owner with the then-current outgoing owner at publication.
        let jobs = futures::future::try_join_all(self.jobs.into_iter().map(|job| async move {
            let owner = if job.kind == ReloadKind::CopyFront {
                None
            } else {
                let kind = if job.record.layer_type() == 2 {
                    LayerType::Picture
                } else {
                    LayerType::Bustup
                };
                Some(
                    HigurashiUserLayer::load_with_setting(
                        assets,
                        scenario,
                        kind,
                        reconstruction::params(&job.record),
                        [0; 4],
                        setting,
                    )
                    .await?,
                )
            };
            Ok::<_, anyhow::Error>((job, owner))
        }))
        .await?;
        Ok(LoadedReload { jobs })
    }
}
impl HigurashiLayers {
    pub(crate) fn publish_reload(
        &mut self,
        loaded: &mut LoadedReload,
        mut fresh_seeds: impl FnMut() -> Result<[u32; 4]>,
    ) -> Result<()> {
        // Prepare live property copies before replacing any bank. In particular
        // do not replace queued tweens with the VM's integer target values.
        let mut properties = Vec::with_capacity(loaded.jobs.len());
        for (job, _) in &loaded.jobs {
            let bank = usize::from(job.kind != ReloadKind::Front);
            properties.push(if job.kind == ReloadKind::CopyFront {
                None
            } else {
                Some(if let Some(old) = &self.owners[job.slot as usize][bank] {
                    (old.properties.clone(), old.owner_id())
                } else {
                    (HigurashiProperties::from_seeds(fresh_seeds()?), 0)
                })
            });
        }
        for ((job, owner), properties) in loaded.jobs.iter_mut().zip(properties) {
            let slot = job.slot as usize;
            if job.kind == ReloadKind::CopyFront {
                self.owners[slot][1] = self.owners[slot][0].clone();
            } else {
                let bank = usize::from(job.kind == ReloadKind::Back);
                if let Some(owner) = owner {
                    let (properties, owner_id) = properties.unwrap();
                    owner.properties = properties;
                    owner.set_owner_id(owner_id);
                }
                self.owners[slot][bank] = owner.take();
            }
        }
        // Original36860/36868 rebind BUP's voice host and raw character override.
        // This host queries voice dynamically and stores the override in Record;
        // original166ac reads asset metadata36870, not that override. The other
        // post-pass cast is AnimationLayer (RTTI14b9e9), absent from supported
        // typed owners, so no PIC/BUP property reset occurs here.
        Ok(())
    }
}

#[cfg(test)]
mod tests;
