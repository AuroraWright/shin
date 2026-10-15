//! Native18db0 asset preparation and19578 target publication.
//! This operates on final replayed memory; the replay loop has its own owner.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackReconstruction {
    Load,
    CopyFront,
    Empty,
}
#[derive(Clone, Debug)]
pub struct LayerReconstruction {
    pub slot: u8,
    pub back: BackReconstruction,
}
impl LayerBanks {
    /// Native18e70 visits all32 slots, independently of allocator membership.
    pub fn reconstruction_plan(&self) -> [LayerReconstruction; SLOTS] {
        std::array::from_fn(|slot| LayerReconstruction {
            slot: slot as u8,
            back: if self.dirty[slot][0] == 0 {
                BackReconstruction::Empty
            } else if self.dirty[slot][1] == 0 {
                BackReconstruction::CopyFront
            } else {
                BackReconstruction::Load
            },
        })
    }
}
fn kind(value: i32) -> Result<Option<crate::layer::user::higurashi::HigurashiLayerKind>> {
    use crate::layer::user::higurashi::HigurashiLayerKind as K;
    let Some(kind) = K::from_native(value) else {
        return Ok(None);
    };
    Ok(Some(kind))
}
pub(super) fn params(record: &Record) -> UntypedNumberArray {
    let p = &record.words[6..14];
    (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7])
}
fn targets(values: &[i32], mut properties: HigurashiProperties) -> HigurashiProperties {
    for (i, &value) in values.iter().enumerate() {
        // Native8d454 writes current/target and empties the queued tween list.
        properties
            .tweener_mut(i)
            .unwrap()
            .fast_forward_to(value as f32);
    }
    properties
}
impl HigurashiLayers {
    /// Actual19578 publishes captured FRONT/BACK jobs slotwise into the
    /// existing keyed vectors. Negative entries are separate retained owners.
    /// The later pass resets only surviving keyed owners, after ALL jobs.
    pub(crate) fn publish_reconstruction_in_place(
        &mut self,
        prepared: ReadyReconstruction,
        seeds: impl FnMut() -> [u32; 4],
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<()> {
        // Direct bank publication has no source VM retirement owner. Scene's
        // native reconstruction path uses the ordered tail below instead.
        ensure!(
            self.pending_front_ids.is_none(),
            "Cannot restore a prepared WIPE publication without its source VM owner"
        );
        self.publish_reconstruction_with_tail(prepared, seeds, settings, || {})
    }
    pub(crate) fn publish_reconstruction_with_tail(
        &mut self,
        prepared: ReadyReconstruction,
        mut seeds: impl FnMut() -> [u32; 4],
        mut settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
        after_keyed: impl FnOnce(),
    ) -> Result<()> {
        ensure!(
            settings.is_some() || !prepared.requires_settings(),
            "Live Higurashi derived layer requires the actual settings bank"
        );
        ensure!(
            self.movie_services.is_some() || !prepared.requires_media(),
            "Reconstructed Higurashi Movie requires configured media services"
        );
        let ReadyReconstruction {
            state,
            owners: jobs,
        } = prepared;
        //1964c..196c8 restores existing root/front/back65 clocks first.
        self.root = targets(&state.root, self.root.clone());
        for bank in 0..2 {
            self.screen[bank] = targets(&state.screen[bank], self.screen[bank].clone());
        }
        for (plan, front, back) in jobs {
            let slot = plan.slot as usize;
            let replacement = if let Some(front) = front {
                let old = self.owners[slot][0].as_ref();
                let fresh = front.present() && (!front.inherits_base() || old.is_none());
                front.construct_with_services(
                    old,
                    fresh.then(&mut seeds),
                    settings.as_deref_mut(),
                    self.movie_services.as_ref(),
                )?
            } else {
                None
            };
            self.owners[slot][0] = replacement;
            // The new FRONT still has its constructor/copied clocks here.
            // BACK copy happens before the separate final target/reset pass.
            let replacement = match plan.back {
                BackReconstruction::Empty => None,
                BackReconstruction::CopyFront => self.owners[slot][0]
                    .as_ref()
                    .context("Higurashi reconstruction CopyFront requires a non-NULL front owner")?
                    .native_copy(),
                BackReconstruction::Load => {
                    if let Some(back) = back {
                        let fresh = back.present().then(&mut seeds);
                        back.construct_with_services(
                            None,
                            fresh,
                            settings.as_deref_mut(),
                            self.movie_services.as_ref(),
                        )?
                    } else {
                        None
                    }
                }
            };
            self.owners[slot][1] = replacement;
        }
        //19a50..19dfc visits only keyed FRONT thenBACK, physical0..31.
        for slot in 0..SLOTS {
            for bank in 0..2 {
                if let Some(owner) = &mut self.owners[slot][bank] {
                    let record = &state.records[slot][bank];
                    owner.properties =
                        targets(&record.words[PROPERTIES_WORD..], owner.properties.clone());
                    owner.reconstruction_refresh()?;
                    owner.set_owner_id(record.words[1]);
                }
            }
        }
        // Native19e68 clears outgoing/wiper before message/Page restoration.
        after_keyed();
        self.page = targets(&state.page, self.page.clone());
        self.state = *state;
        Ok(())
    }
    /// Prepare native reconstruction targets without publishing partial assets.
    /// Native workers prepare front factories even for unallocated slots, then
    /// copy/load/clear back owners from the two dirty bytes. Every live target
    /// receives saved targets and empty tween queues. Native6717c inherits
    /// front-owner seeds/phases from the outgoing owner; separately loaded
    /// back owners receive fresh clocks. Root/screen/page clocks are retained.
    pub async fn reconstruct(
        &self,
        state: &LayerBanks,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        seeds: impl FnMut() -> [u32; 4],
    ) -> Result<Self> {
        self.reconstruct_with_setting(state, assets, scenario, seeds, 0)
            .await
    }
    pub(crate) async fn reconstruct_with_setting(
        &self,
        state: &LayerBanks,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        seeds: impl FnMut() -> [u32; 4],
        setting: u8,
    ) -> Result<Self> {
        self.reconstruct_with_settings(state, assets, scenario, seeds, setting, None)
            .await
    }
    pub(crate) async fn reconstruct_with_settings(
        &self,
        state: &LayerBanks,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        seeds: impl FnMut() -> [u32; 4],
        setting: u8,
        settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<Self> {
        let prepared = Self::prepare_reconstruction(state, assets, scenario, setting).await?;
        self.construct_reconstruction(prepared, seeds, settings)
    }
    /// Native18db0 prepares factories without constructing owners or touching
    /// settings/OS/MT. The main19578 poll owns the ready constructors.
    pub(crate) async fn prepare_reconstruction(
        state: &LayerBanks,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        setting: u8,
    ) -> Result<ReadyReconstruction> {
        use crate::layer::user::higurashi::Prepared;
        // Native18db0 prepares every required factory and waits for readiness
        // before19578 starts constructors/publication in physical slot order.
        let mut prepared = Vec::with_capacity(SLOTS);
        for plan in state.reconstruction_plan() {
            let slot = plan.slot as usize;
            let front = &state.records[slot][0];
            let front = if let Some(kind) = kind(front.layer_type())? {
                Some(Prepared::load(assets, scenario, kind, params(front), setting).await?)
            } else {
                None
            };
            let back = if plan.back == BackReconstruction::Load {
                let back = &state.records[slot][1];
                if let Some(kind) = kind(back.layer_type())? {
                    Some(Prepared::load(assets, scenario, kind, params(back), setting).await?)
                } else {
                    None
                }
            } else {
                None
            };
            prepared.push((plan, front, back));
        }
        Ok(ReadyReconstruction {
            state: Box::new(state.clone()),
            owners: prepared,
        })
    }
    pub(crate) fn construct_reconstruction(
        &self,
        prepared: ReadyReconstruction,
        mut seeds: impl FnMut() -> [u32; 4],
        mut settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<Self> {
        ensure!(
            self.retiring.iter().all(|group| group.entries().is_empty()),
            "Higurashi in-place restoration of live retiring layers requires native ownership integration"
        );
        ensure!(
            settings.is_some() || !prepared.requires_settings(),
            "Live Higurashi derived layer requires the actual settings bank"
        );
        ensure!(
            self.movie_services.is_some() || !prepared.requires_media(),
            "Reconstructed Higurashi Movie requires configured media services"
        );
        let ReadyReconstruction {
            state,
            owners: prepared,
        } = prepared;
        let mut owners: Vec<[Option<HigurashiUserLayer>; 2]> =
            (0..SLOTS).map(|_| [None, None]).collect();
        for (plan, front, back) in prepared {
            let slot = plan.slot as usize;
            if let Some(front) = front {
                let old = self.owners[slot][0].as_ref();
                let fresh = front.present() && (!front.inherits_base() || old.is_none());
                owners[slot][0] = front.construct_with_services(
                    old,
                    fresh.then(&mut seeds),
                    settings.as_deref_mut(),
                    self.movie_services.as_ref(),
                )?;
            }
            owners[slot][1] = match plan.back {
                BackReconstruction::Empty => None,
                BackReconstruction::CopyFront => owners[slot][0]
                    .as_ref()
                    .context("Higurashi reconstruction CopyFront requires a non-NULL front owner")?
                    .native_copy(),
                BackReconstruction::Load => {
                    if let Some(back) = back {
                        let fresh = back.present().then(&mut seeds);
                        back.construct_with_services(
                            None,
                            fresh,
                            settings.as_deref_mut(),
                            self.movie_services.as_ref(),
                        )?
                    } else {
                        None
                    }
                }
            };
            for bank in 0..2 {
                if let Some(owner) = &mut owners[slot][bank] {
                    let record = &state.records[slot][bank];
                    owner.properties =
                        targets(&record.words[PROPERTIES_WORD..], owner.properties.clone());
                    owner.reconstruction_refresh()?;
                    owner.set_owner_id(record.words[1]);
                    // Native36868 stores the override, but166ac uses36870
                    // (asset metadata) for older voice feedback. The raw words
                    // remain in LayerBanks; do not apply later-engine behavior.
                }
            }
        }
        let root = targets(&state.root, self.root.clone());
        let screen = std::array::from_fn(|i| targets(&state.screen[i], self.screen[i].clone()));
        let page = targets(&state.page, self.page.clone());
        Ok(Self {
            state: *state,
            owners,
            retiring: std::array::from_fn(|_| super::super::temporal::Group::new(Vec::new())),
            temporal_fault: None,
            root,
            screen,
            page,
            pending_front_ids: None,
            movie_services: self.movie_services.clone(),
            pending_metadata: Vec::new(),
        })
    }
}

pub(crate) struct ReadyReconstruction {
    state: Box<LayerBanks>,
    owners: Vec<(
        LayerReconstruction,
        Option<crate::layer::user::higurashi::Prepared>,
        Option<crate::layer::user::higurashi::Prepared>,
    )>,
}
impl ReadyReconstruction {
    pub fn requires_settings(&self) -> bool {
        self.owners.iter().any(|(_, front, back)| {
            front.as_ref().is_some_and(|p| p.requires_settings())
                || back.as_ref().is_some_and(|p| p.requires_settings())
        })
    }
    pub fn requires_media(&self) -> bool {
        self.owners.iter().any(|(_, front, back)| {
            front.as_ref().is_some_and(|p| p.requires_media())
                || back.as_ref().is_some_and(|p| p.requires_media())
        })
    }
}

#[cfg(test)]
mod retained_native_tests;
#[cfg(test)]
mod source_command_native_tests;
#[cfg(test)]
mod tests;
