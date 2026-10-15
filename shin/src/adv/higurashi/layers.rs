//! Updated Higurashi layer command state and drawable publication.
//! Native slots have two records selected independently for each logical ID.
use anyhow::{Context, Result, bail, ensure};
use shin_core::{
    format::scenario::{higurashi::HigurashiScenario, instruction_elements::UntypedNumberArray},
    time::Ticks,
    vm::{command::types::LayerType, higurashi::command::HigurashiRuntimeCommand},
};
use std::sync::Arc;

use crate::{
    asset::system::AssetServer,
    layer::{
        higurashi_properties::{HigurashiProperties, INITIAL_VALUES},
        user::higurashi::HigurashiUserLayer,
    },
};

pub const LOGICAL_LAYERS: usize = 1024;
pub const SLOTS: usize = 32;
mod movie_wait;
mod reconstruction;
mod unload;
#[cfg(test)]
mod unload_native_tests;
mod wait;
pub(crate) use unload::LayerUnload;
mod gallery;
pub(in crate::adv::higurashi) use gallery::unlock as unlock_gallery_bit;
#[cfg(test)]
mod movie_owner_tests;
pub(in crate::adv::higurashi) use movie_wait::MovieWait;
pub(in crate::adv::higurashi) use wait::LayerWait;
pub(crate) mod reload;
pub(crate) use reconstruction::ReadyReconstruction;
pub use reconstruction::{BackReconstruction, LayerReconstruction};
const RECORD_WORDS: usize = 79;
const PROPERTIES_WORD: usize = 14;

/// A load owns its proposed integer state and ready asset descriptors, while the
/// existing drawable owners remain available for updates and rendering.
pub(super) struct LayerLoadPlan {
    next: Box<LayerBanks>,
    kind: crate::layer::user::higurashi::HigurashiLayerKind,
    params: UntypedNumberArray,
    targets: Vec<(Target, bool)>,
}
pub(super) struct LoadedLayers {
    next: Box<LayerBanks>,
    owners: Vec<(Target, bool, crate::layer::user::higurashi::Prepared)>,
}
impl LayerLoadPlan {
    pub fn immediate(&self) -> bool {
        use crate::layer::user::higurashi::HigurashiLayerKind as K;
        matches!(self.kind, K::Null | K::Tile | K::Movie)
    }
    pub async fn load(
        self,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
    ) -> Result<LoadedLayers> {
        self.load_with_setting(assets, scenario, 0).await
    }
    pub async fn load_with_setting(
        self,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        setting: u8,
    ) -> Result<LoadedLayers> {
        let mut owners = Vec::new();
        for (target, keep) in self.targets {
            let owner = crate::layer::user::higurashi::Prepared::load(
                assets,
                scenario,
                self.kind,
                self.params,
                setting,
            )
            .await?;
            owners.push((target, keep, owner));
        }
        Ok(LoadedLayers {
            next: self.next,
            owners,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub logical: u16,
    pub slot: u8,
    pub bank: u8,
}

/// The integer targets retained by the older VM, independent of live tweens.
/// Keep the untouched words when copying a record (native memcpy of 0x13c).
#[derive(Debug, Clone)]
pub struct Record {
    words: [i32; RECORD_WORDS],
}

impl Record {
    fn empty() -> Self {
        let mut words = [0; RECORD_WORDS];
        words[0] = -1;
        Self { words }
    }
    pub fn layer_type(&self) -> i32 {
        self.words[0]
    }
    pub(crate) fn native_words(&self) -> &[i32; RECORD_WORDS] {
        &self.words
    }
    pub(crate) fn native_words_mut(&mut self) -> &mut [i32; RECORD_WORDS] {
        &mut self.words
    }
    pub fn property(&self, index: usize) -> Option<i32> {
        (index < INITIAL_VALUES.len()).then(|| self.words[PROPERTIES_WORD + index])
    }
    fn reset_properties(&mut self) {
        self.words[PROPERTIES_WORD..].copy_from_slice(&INITIAL_VALUES);
    }
}

#[derive(Debug, Clone)]
pub struct LayerBanks {
    free: [u8; SLOTS],
    allocated: usize,
    logical_to_slot: [u8; LOGICAL_LAYERS],
    slot_to_logical: [u16; SLOTS],
    selected: [u16; 2],
    back: [u8; LOGICAL_LAYERS],
    dirty: [[u8; 2]; SLOTS],
    records: [[Record; 2]; SLOTS],
    root: [i32; 65],
    screen: [[i32; 65]; 2],
    screen_back: u8,
    page: [i32; 65],
    loads_with_init: u32,
    back_started: bool,
}

impl Default for LayerBanks {
    fn default() -> Self {
        let mut page = INITIAL_VALUES;
        // The VM-state initializer 106a8 sets Page property 5 to 900.
        page[5] = 900;
        Self {
            free: std::array::from_fn(|i| i as u8),
            allocated: 0,
            logical_to_slot: [SLOTS as u8; LOGICAL_LAYERS],
            slot_to_logical: [LOGICAL_LAYERS as u16; SLOTS],
            selected: [0, 0],
            back: [0; LOGICAL_LAYERS],
            dirty: [[0; 2]; SLOTS],
            records: std::array::from_fn(|_| std::array::from_fn(|_| Record::empty())),
            root: INITIAL_VALUES,
            screen: [INITIAL_VALUES; 2],
            screen_back: 0,
            page,
            loads_with_init: 0,
            back_started: false,
        }
    }
}

impl LayerBanks {
    pub(crate) fn selected_native(&self) -> [u16; 2] {
        self.selected
    }
    pub(crate) fn root_targets(&self) -> &[i32; 65] {
        &self.root
    }
    pub(crate) fn front_screen_targets(&self) -> &[i32; 65] {
        &self.screen[0]
    }
    fn logical(id: i32) -> Result<usize> {
        ensure!(
            (0..LOGICAL_LAYERS as i32).contains(&id),
            "Higurashi logical layer {id} outside 0..1024"
        );
        Ok(id as usize)
    }
    pub fn select(&mut self, from: i32, to: i32) {
        let from = from.clamp(0, 1023) as u16;
        let to = to.clamp(0, 1023) as u16;
        self.selected = [from.min(to), from.max(to)];
    }
    /// Preview d543c sorts the signed operands before storing native u16s.
    pub(crate) fn select_native_u16(&mut self, from: i32, to: i32) {
        self.selected = [from.min(to) as u16, from.max(to) as u16];
    }
    /// Preview d5368/20c5c keeps records in their physical slots while logical
    /// mappings, retained back selectors and selected-bank owner IDs move.
    pub(crate) fn swap_native(&mut self, from: i32, to: i32) {
        if from == to || !(0..1024).contains(&from) || !(0..1024).contains(&to) {
            return;
        }
        let (from, to) = (from as usize, to as usize);
        self.logical_to_slot.swap(from, to);
        self.back.swap(from, to);
        for logical in [from, to] {
            let slot = self.logical_to_slot[logical];
            if slot != SLOTS as u8 {
                self.slot_to_logical[slot as usize] = logical as u16;
                self.records[slot as usize][self.back[logical] as usize].words[1] = logical as i32;
            }
        }
    }
    pub fn allocate(&mut self, logical: u16) -> Option<u8> {
        let existing = *self.logical_to_slot.get(logical as usize)?;
        if existing != SLOTS as u8 {
            return Some(existing);
        }
        if self.allocated == SLOTS {
            return None;
        }
        let slot = self.free[self.allocated];
        self.allocated += 1;
        self.logical_to_slot[logical as usize] = slot;
        self.slot_to_logical[slot as usize] = logical;
        Some(slot)
    }
    fn free(&mut self, logical: u16) {
        let slot = self.logical_to_slot[logical as usize];
        if slot == SLOTS as u8 {
            return;
        }
        self.logical_to_slot[logical as usize] = SLOTS as u8;
        self.slot_to_logical[slot as usize] = LOGICAL_LAYERS as u16;
        self.allocated -= 1;
        self.free[self.allocated] = slot;
    }
    ///20cdc/20bc4 reserve a mapping without creating a LAYERLOAD record.
    /// Character artwork is inserted into plane0 independently of the logical
    /// ID's retained back selector and integer resource metadata.
    pub(crate) fn reserve_temporary(&mut self) -> Result<Target> {
        let logical = self
            .logical_to_slot
            .iter()
            .position(|&slot| slot == SLOTS as u8)
            .context("Higurashi has no free temporary logical layer")? as u16;
        let slot = self
            .allocate(logical)
            .context("Higurashi has no free temporary scene slot")?;
        Ok(Target {
            logical,
            slot,
            bank: 0,
        })
    }
    pub(super) fn release_temporary(&mut self, target: Target) {
        if self.logical_to_slot[target.logical as usize] == target.slot {
            self.free(target.logical);
        }
    }
    pub fn target(&self, logical: u16) -> Option<Target> {
        let slot = *self.logical_to_slot.get(logical as usize)?;
        (slot != SLOTS as u8).then(|| Target {
            logical,
            slot,
            bank: self.back[logical as usize],
        })
    }
    pub fn record(&self, target: Target) -> &Record {
        &self.records[target.slot as usize][target.bank as usize]
    }
    pub(crate) fn record_mut(&mut self, target: Target) -> &mut Record {
        &mut self.records[target.slot as usize][target.bank as usize]
    }
    fn targets(&self, id: i32) -> Result<Vec<Target>> {
        if id != -4 {
            if (-3..=-1).contains(&id) {
                return Ok(Vec::new());
            }
            let logical = Self::logical(id)?;
            return Ok(self.target(logical as u16).into_iter().collect());
        }
        let [from, to] = self.selected;
        // 15f38 uses slot order for sparse ranges, logical order otherwise.
        if self.allocated as i32 <= i32::from(to) - i32::from(from) {
            Ok(self
                .slot_to_logical
                .iter()
                .copied()
                .filter(|logical| from <= *logical && *logical <= to)
                .filter_map(|logical| self.target(logical))
                .collect())
        } else {
            Ok((from..=to)
                .filter_map(|logical| self.target(logical))
                .collect())
        }
    }
    pub fn start_back(&mut self, id: i32) -> Result<Vec<Target>> {
        let targets = self.targets(id)?;
        let copied: Vec<_> = targets.into_iter().filter(|t| t.bank == 0).collect();
        for t in &copied {
            self.records[t.slot as usize][1] = self.records[t.slot as usize][0].clone();
            self.dirty[t.slot as usize] = [1, 0];
        }
        if id == -4 {
            for logical in self.selected[0]..=self.selected[1] {
                self.back[logical as usize] = 1;
            }
        } else if id >= 0 {
            self.back[Self::logical(id)?] = 1;
        } else if id == -2 && self.screen_back == 0 {
            self.screen[1] = self.screen[0];
            self.screen_back = 1;
        }
        self.back_started = true;
        Ok(copied)
    }
    pub fn initialize(&mut self, id: i32) -> Result<Vec<Target>> {
        let targets = self.targets(id)?;
        match id {
            -1 => self.root = INITIAL_VALUES,
            -2 => self.screen[self.screen_back as usize] = INITIAL_VALUES,
            -3 => self.page = INITIAL_VALUES,
            _ => {
                for t in &targets {
                    self.record_mut(*t).reset_properties();
                }
            }
        }
        Ok(targets)
    }
    pub fn control(
        &mut self,
        id: i32,
        property: i32,
        value: i32,
        flags: u32,
    ) -> Result<Vec<Target>> {
        ensure!(
            (0..65).contains(&property),
            "Higurashi property {property} outside 0..65"
        );
        let index = property as usize;
        let targets = self.targets(id)?;
        let apply = |old: &mut i32| {
            *old = if flags & 0x80 != 0 {
                old.wrapping_add(value)
            } else {
                value
            };
        };
        match id {
            -1 => apply(&mut self.root[index]),
            -2 => apply(&mut self.screen[self.screen_back as usize][index]),
            -3 => apply(&mut self.page[index]),
            _ => {
                for t in &targets {
                    apply(&mut self.record_mut(*t).words[PROPERTIES_WORD + index]);
                }
            }
        }
        Ok(targets)
    }
    pub fn load(
        &mut self,
        id: i32,
        kind: i32,
        flags: u32,
        params: UntypedNumberArray,
    ) -> Result<Vec<(Target, bool)>> {
        if (-3..=-1).contains(&id) {
            return Ok(Vec::new());
        }
        let logicals: Vec<u16> = if id == -4 {
            (self.selected[0]..=self.selected[1]).collect()
        } else {
            vec![Self::logical(id)? as u16]
        };
        let mut targets = Vec::new();
        for logical in logicals {
            if let Some(slot) = self.allocate(logical) {
                let target = Target {
                    logical,
                    slot,
                    bank: self.back[logical as usize],
                };
                let keep = flags & 1 != 0 && self.record(target).layer_type() >= 0;
                if target.bank != 0 {
                    self.dirty[slot as usize] = [1, 1];
                }
                let record = self.record_mut(target);
                record.words[0] = kind;
                record.words[6..14].copy_from_slice(&[
                    params.0, params.1, params.2, params.3, params.4, params.5, params.6, params.7,
                ]);
                if !keep {
                    record.reset_properties();
                    record.words[1] = i32::from(logical);
                    record.words[2] = -1;
                    self.loads_with_init = self.loads_with_init.wrapping_add(1);
                }
                targets.push((target, keep));
            }
        }
        Ok(targets)
    }
    pub fn unload(&mut self, id: i32) -> Result<Vec<Target>> {
        let targets = self.targets(id)?;
        for t in &targets {
            if t.bank == 0 {
                self.records[t.slot as usize][0].words[0] = -1;
                self.records[t.slot as usize][1].words[0] = -1;
                self.free(t.logical);
            } else {
                self.dirty[t.slot as usize] = [1, 1];
                self.records[t.slot as usize][1].words[0] = -1;
            }
        }
        Ok(targets)
    }

    /// WIPE constructor 5f90 promotes integer records before the live transition
    /// is prepared. Returned targets identify slots already edited in back.
    pub fn commit_back(&mut self) -> Vec<Target> {
        // 20c90 snapshots allocated IDs in slot order before any frees occur.
        let logicals: Vec<_> = self
            .slot_to_logical
            .iter()
            .copied()
            .filter(|id| *id != LOGICAL_LAYERS as u16)
            .collect();
        let mut promoted = Vec::new();
        for logical in logicals {
            let t = self.target(logical).expect("allocated logical ID");
            if t.bank == 1 {
                self.records[t.slot as usize][0] = self.records[t.slot as usize][1].clone();
                if self.records[t.slot as usize][0].layer_type() < 0 {
                    self.free(logical);
                }
                promoted.push(t);
            }
            self.dirty[t.slot as usize] = [0, 0];
        }
        self.back.fill(0);
        if self.screen_back != 0 {
            self.screen[0] = self.screen[1];
            self.screen_back = 0;
        }
        self.back_started = false;
        promoted
    }
}

/// Executes older layer commands on typed draw owners. Wipe/group publication,
/// message/audio commands and native entropy service are separate integration
/// work; unknown commands are returned to the caller without consuming them.
pub struct HigurashiLayers {
    pub state: LayerBanks,
    owners: Vec<[Option<HigurashiUserLayer>; 2]>,
    retiring: [super::temporal::Group<HigurashiUserLayer>; 2],
    temporal_fault: Option<String>,
    root: HigurashiProperties,
    screen: [HigurashiProperties; 2],
    page: HigurashiProperties,
    pending_front_ids: Option<[i32; SLOTS]>,
    movie_services: Option<crate::layer::user::higurashi::movie::Services>,
    pending_metadata: Vec<(i32, i32)>,
}
impl Drop for HigurashiLayers {
    fn drop(&mut self) {
        // Screenb8b98 clears front14c0 before back14c8. Group-handle release
        // later runs in a different order after these child vectors are empty.
        for bank in 0..2 {
            clear_group_reverse(
                self.owners.iter_mut().map(|owners| &mut owners[bank]),
                &mut self.retiring[bank],
            );
        }
    }
}

/// Integer publication happens in the WIPE constructor; live publication waits
/// for the previous root wiper. Only the scene scheduler holds this ticket.
pub(super) struct PreparedBack {
    promoted: Vec<Target>,
    screen_back: u8,
}

/// Previous front resources remain owned until the transition caller retires
/// this snapshot. Publication alone must not release the old screen's assets.
pub struct HigurashiPreviousFront {
    owners: Vec<Option<HigurashiUserLayer>>,
    retiring: super::temporal::Group<HigurashiUserLayer>,
    temporal_fault: Option<String>,
    owner_ids: [i32; SLOTS],
    pub screen: HigurashiProperties,
}
impl Drop for HigurashiPreviousFront {
    fn drop(&mut self) {
        // Wiper completion6b944 clears the combined vector in reverse:
        // keyed suffix highest slot first, then negative prefix oldest first.
        clear_group_reverse(self.owners.iter_mut(), &mut self.retiring);
    }
}
fn clear_group_reverse<'a, T: 'a>(
    owners: impl DoubleEndedIterator<Item = &'a mut Option<T>>,
    retiring: &mut super::temporal::Group<T>,
) {
    for owner in owners.rev() {
        drop(owner.take());
    }
    retiring.clear_reverse();
}

fn retiring_fault(group: &super::temporal::Group<HigurashiUserLayer>) -> Option<String> {
    group
        .entries()
        .iter()
        .any(|entry| entry.owner.is_none())
        .then(|| "Native retiring group contains a NULL virtual copy".to_owned())
}
fn retiring_children(
    group: &super::temporal::Group<HigurashiUserLayer>,
) -> impl Iterator<Item = super::group::Child<'_>> {
    group
        .entries()
        .iter()
        .filter_map(|entry| entry.owner.as_ref())
        .map(|layer| super::group::Child {
            owner_id: layer.owner_id(),
            slot: u8::MAX,
            layer,
        })
}

impl HigurashiPreviousFront {
    pub(crate) fn render_capture_group(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        parent: &crate::layer::render_params::higurashi::HigurashiTransform,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi outgoing group failed: {:?}",
            self.visual_fault()
        );
        super::group::render_capture(pass, self.group_children(), &self.screen, parent)
    }
    pub(super) fn temporal_update_failed(&self) -> bool {
        self.temporal_fault.is_some()
    }
    pub(crate) fn visual_fault(&self) -> Option<String> {
        self.temporal_fault
            .clone()
            .or_else(|| retiring_fault(&self.retiring))
            .or_else(|| self.movie_fault())
    }
    pub(crate) fn movie_fault(&self) -> Option<String> {
        self.retiring
            .entries()
            .iter()
            .filter_map(|entry| entry.owner.as_ref())
            .chain(self.owners.iter().flatten())
            .find_map(HigurashiUserLayer::movie_fault)
    }
    pub(crate) fn prepare_movies(
        &self,
        context: &mut crate::render::PreRenderContext,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi outgoing group failed: {:?}",
            self.visual_fault()
        );
        for owner in self
            .retiring
            .entries()
            .iter()
            .filter_map(|entry| entry.owner.as_ref())
            .chain(self.owners.iter().flatten())
        {
            owner.prepare_movie(context)?;
        }
        Ok(())
    }
    pub(crate) fn set_route_navigation(&mut self, value: bool) {
        for owner in self
            .retiring
            .entries_mut()
            .iter_mut()
            .filter_map(|entry| entry.owner.as_mut())
            .chain(self.owners.iter_mut().flatten())
        {
            owner.set_route_navigation(value);
        }
    }
    pub(super) fn prepare_group(
        &self,
        context: &mut crate::render::PreRenderContext,
        parent: &crate::layer::render_params::higurashi::HigurashiTransform,
        depth: &crate::layer::user::higurashi::effect_capture::SharedDepth,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi outgoing group failed: {:?}",
            self.visual_fault()
        );
        super::group::prepare_children(context, self.group_children(), &self.screen, parent, depth)
    }
    pub fn advance(&mut self, delta: Ticks, voice: Option<&dyn Fn(i32, f32) -> Option<u8>>) {
        self.screen.advance(delta);
        let count = self.owners.iter().flatten().count();
        let owners = &mut self.owners;
        if let Err(error) = self.retiring.advance_with_tail(
            delta.as_f32(),
            count,
            |owner, _| {
                owner.advance(delta, voice);
                Ok(())
            },
            || {
                for owner in owners.iter_mut().flatten() {
                    owner.advance(delta, voice);
                }
                Ok(())
            },
        ) {
            self.temporal_fault
                .get_or_insert_with(|| format!("{error:#}"));
        }
    }
    pub(crate) fn fast_forward(&mut self) {
        self.screen.fast_forward();
        let count = self.owners.iter().flatten().count();
        let owners = &mut self.owners;
        if let Err(error) = self.retiring.fast_forward_with_tail(
            count,
            |owner| {
                owner.fast_forward();
                Ok(())
            },
            || {
                for owner in owners.iter_mut().flatten() {
                    owner.fast_forward();
                }
                Ok(())
            },
        ) {
            self.temporal_fault
                .get_or_insert_with(|| format!("{error:#}"));
        }
    }
    pub fn layer(&self, slot: u8) -> Option<&HigurashiUserLayer> {
        self.owners.get(slot as usize)?.as_ref()
    }
    pub fn group_children(&self) -> Vec<super::group::Child<'_>> {
        retiring_children(&self.retiring)
            .chain(self.owners.iter().enumerate().filter_map(|(slot, layer)| {
                Some(super::group::Child {
                    owner_id: self.owner_ids[slot],
                    slot: slot as u8,
                    layer: layer.as_ref()?,
                })
            }))
            .collect()
    }
    pub fn render_group(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        parent: &crate::layer::render_params::higurashi::HigurashiTransform,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi outgoing group failed: {:?}",
            self.visual_fault()
        );
        super::group::render(pass, self.group_children(), &self.screen, parent)
    }
}

impl HigurashiLayers {
    pub(crate) fn front_properties(&self) -> &HigurashiProperties {
        &self.screen[0]
    }
    /// Direct native probe transport: physical keyed publication bypasses VM
    /// allocation. The concrete owner and all subsequent traversal are live.
    #[cfg(test)]
    pub(crate) fn reference_publish_physical(
        &mut self,
        bank: u8,
        slot: u8,
        owner: HigurashiUserLayer,
    ) -> Result<()> {
        ensure!(
            bank < 2 && (slot as usize) < SLOTS,
            "Invalid native physical probe target"
        );
        self.owners[slot as usize][bank as usize] = Some(owner);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn reference_physical_properties_mut(
        &mut self,
        which: u32,
    ) -> Option<&mut HigurashiProperties> {
        match which {
            0 => Some(&mut self.root),
            1 => Some(&mut self.screen[0]),
            2 => Some(&mut self.screen[1]),
            _ => None,
        }
    }
    /// Actual b919c plane transfer only. The native frame probe bypasses the
    /// earlier command/VM publication and its separate virtual-copy prefix.
    #[cfg(test)]
    pub(crate) fn reference_rotate_planes(&mut self) -> Result<HigurashiPreviousFront> {
        ensure!(
            self.pending_front_ids.is_none(),
            "Direct plane probe cannot bypass a prepared WIPE"
        );
        let previous = HigurashiPreviousFront {
            owner_ids: std::array::from_fn(|slot| {
                self.owners[slot][0]
                    .as_ref()
                    .map_or(0, HigurashiUserLayer::owner_id)
            }),
            owners: self
                .owners
                .iter_mut()
                .map(|banks| banks[0].take())
                .collect(),
            retiring: std::mem::replace(
                &mut self.retiring[0],
                super::temporal::Group::new(Vec::new()),
            ),
            temporal_fault: self.temporal_fault.clone(),
            screen: self.screen[0].clone(),
        };
        self.retiring.swap(0, 1);
        for banks in &mut self.owners {
            banks[0] = banks[1].take();
        }
        Ok(previous)
    }
    pub(crate) fn render_front_capture(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        parent: &crate::layer::render_params::higurashi::HigurashiTransform,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi group failed: {:?}",
            self.visual_fault()
        );
        super::group::render_capture(pass, self.group_children(), &self.screen[0], parent)
    }
    pub(super) fn temporal_update_failed(&self) -> bool {
        self.temporal_fault.is_some()
    }
    pub fn new(mut seeds: impl FnMut() -> [u32; 4]) -> Self {
        let mut page = HigurashiProperties::from_seeds(seeds());
        page.control(5, 900.0, Ticks::ZERO, 0)
            .expect("valid Page property");
        Self {
            state: LayerBanks::default(),
            owners: (0..SLOTS).map(|_| [None, None]).collect(),
            retiring: std::array::from_fn(|_| super::temporal::Group::new(Vec::new())),
            temporal_fault: None,
            root: HigurashiProperties::from_seeds(seeds()),
            screen: std::array::from_fn(|_| HigurashiProperties::from_seeds(seeds())),
            page,
            pending_front_ids: None,
            movie_services: None,
            pending_metadata: Vec::new(),
        }
    }
    pub fn layer(&self, target: Target) -> Option<&HigurashiUserLayer> {
        self.owners
            .get(target.slot as usize)?
            .get(target.bank as usize)?
            .as_ref()
    }
    pub(crate) fn layer_mut(&mut self, target: Target) -> Option<&mut HigurashiUserLayer> {
        self.owners
            .get_mut(target.slot as usize)?
            .get_mut(target.bank as usize)?
            .as_mut()
    }
    ///16238 publishes a concrete layer on scene plane0. The temporary command
    /// reservation stays separate from LAYERLOAD resource metadata.
    pub(crate) fn publish_temporary_front(
        &mut self,
        target: Target,
        owner: HigurashiUserLayer,
    ) -> Result<()> {
        ensure!(
            target.bank == 0
                && self
                    .state
                    .target(target.logical)
                    .is_some_and(|t| t.slot == target.slot),
            "Higurashi temporary scene target is no longer reserved"
        );
        self.owners[target.slot as usize][0] = Some(owner);
        Ok(())
    }
    ///164a4 removes the original layer only if the slot still contains it.
    /// The logical mapping may already be freed when command retirement runs.
    pub(crate) fn retire_character_front(&mut self, target: Target, identity: usize) {
        let Some(owner) = self
            .owners
            .get_mut(target.slot as usize)
            .and_then(|b| b.get_mut(0))
        else {
            return;
        };
        if owner
            .as_ref()
            .and_then(|l| l.character_view())
            .is_some_and(|v| Arc::as_ptr(&v.model) as usize == identity)
        {
            *owner = None;
        }
    }
    pub(crate) fn retire_temporary_front(&mut self, target: Target, identity: usize) {
        let Some(owner) = self
            .owners
            .get_mut(target.slot as usize)
            .and_then(|banks| banks.get_mut(0))
        else {
            return;
        };
        if owner
            .as_ref()
            .and_then(HigurashiUserLayer::temporary_owner)
            .is_some_and(|widget| widget.widget_identity() == identity)
        {
            *owner = None;
        }
    }
    pub(super) fn plan_load(
        &self,
        id: i32,
        kind: i32,
        flags: i32,
        params: UntypedNumberArray,
    ) -> Result<LayerLoadPlan> {
        use crate::layer::user::higurashi::HigurashiLayerKind as K;
        let kind = K::from_native(kind).context("Invalid Higurashi native layer type")?;
        ensure!(
            kind != K::Animation || (0..6).contains(&params.0),
            "Higurashi Animation mode {} returns NULL; native LAYERLOAD cannot publish it",
            params.0
        );
        let mut next = Box::new(self.state.clone());
        let targets = next.load(id, kind.native_id(), flags as u32, params)?;
        Ok(LayerLoadPlan {
            next,
            kind,
            params,
            targets,
        })
    }
    pub(super) fn publish_load(
        &mut self,
        loaded: LoadedLayers,
        mut seeds: impl FnMut() -> [u32; 4],
    ) -> Result<()> {
        self.publish_load_with_settings(loaded, &mut seeds, None)
    }
    pub(super) fn publish_load_with_settings(
        &mut self,
        loaded: LoadedLayers,
        mut seeds: impl FnMut() -> [u32; 4],
        mut settings: Option<&mut shin_core::format::save::higurashi::SaveData>,
    ) -> Result<()> {
        ensure!(
            settings.is_some() || loaded.owners.iter().all(|(_, _, p)| !p.requires_settings()),
            "Live Higurashi derived layer requires the actual settings bank"
        );
        ensure!(
            self.movie_services.is_some()
                || loaded.owners.iter().all(|(_, _, p)| !p.requires_media()),
            "Live Higurashi Movie requires configured media services"
        );
        //93c8 invokes20d20 once with requested kind/params if any target is
        // front. All targets share that descriptor, including mixed -4 banks.
        let metadata = loaded
            .owners
            .iter()
            .find(|(target, _, _)| target.bank == 0)
            .map(|(target, _, _)| {
                let record = loaded.next.record(*target);
                (record.layer_type(), record.words[6])
            })
            .filter(|(kind, _)| matches!(kind, 2 | 7));
        let owners = loaded
            .owners
            .into_iter()
            .map(|(target, keep, prepared)| {
                let old = keep.then(|| self.layer(target)).flatten();
                let fresh = prepared.present() && (!prepared.inherits_base() || old.is_none());
                let mut owner = prepared.construct_with_services(
                    old,
                    fresh.then(&mut seeds),
                    settings.as_deref_mut(),
                    self.movie_services.as_ref(),
                )?;
                if !keep {
                    if let Some(owner) = &mut owner {
                        owner.set_owner_id(i32::from(target.logical));
                    }
                }
                Ok((target, owner))
            })
            .collect::<Result<Vec<_>>>()?;
        self.state = *loaded.next;
        for (target, owner) in owners {
            self.owners[target.slot as usize][target.bank as usize] = owner;
        }
        if let Some(metadata) = metadata {
            self.pending_metadata.push(metadata);
        }
        Ok(())
    }
    pub(crate) fn set_route_navigation(&mut self, value: bool) {
        for group in &mut self.retiring {
            for owner in group
                .entries_mut()
                .iter_mut()
                .filter_map(|entry| entry.owner.as_mut())
            {
                owner.set_route_navigation(value);
            }
        }
        for owner in self.owners.iter_mut().flat_map(|b| b.iter_mut()).flatten() {
            owner.set_route_navigation(value);
        }
    }
    pub(crate) fn set_movie_services(
        &mut self,
        services: crate::layer::user::higurashi::movie::Services,
    ) {
        self.movie_services = Some(services);
    }
    pub(crate) fn has_movie_services(&self) -> bool {
        self.movie_services.is_some()
    }
    pub(crate) fn set_movie_preferences(&self, preferences: [u8; 3]) {
        if let Some(services) = &self.movie_services {
            services.set_preferences(preferences);
        }
    }
    pub(crate) fn movie_fault(&self) -> Option<String> {
        self.retiring
            .iter()
            .flat_map(|group| group.entries())
            .filter_map(|entry| entry.owner.as_ref())
            .chain(self.owners.iter().flat_map(|b| b.iter()).flatten())
            .find_map(HigurashiUserLayer::movie_fault)
    }
    pub(crate) fn visual_fault(&self) -> Option<String> {
        self.temporal_fault
            .clone()
            .or_else(|| self.retiring.iter().find_map(retiring_fault))
            .or_else(|| self.movie_fault())
    }
    pub(crate) fn prepare_movies(
        &self,
        context: &mut crate::render::PreRenderContext,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi group failed: {:?}",
            self.visual_fault()
        );
        for owner in self
            .retiring
            .iter()
            .flat_map(|group| group.entries())
            .filter_map(|entry| entry.owner.as_ref())
            .chain(self.owners.iter().flat_map(|b| b.iter()).flatten())
        {
            owner.prepare_movie(context)?;
        }
        Ok(())
    }
    pub fn group_children(&self) -> Vec<super::group::Child<'_>> {
        retiring_children(&self.retiring[0])
            .chain(self.owners.iter().enumerate().filter_map(|(slot, banks)| {
                let layer = banks[0].as_ref()?;
                Some(super::group::Child {
                    owner_id: layer.owner_id(),
                    slot: slot as u8,
                    layer,
                })
            }))
            .collect()
    }
    pub fn root_transform(&self) -> crate::layer::render_params::higurashi::HigurashiTransform {
        self.root.composed_transform(&Default::default())
    }
    /// ba210/67c78 and both group virtual10 callbacks. Message's override
    /// advances its text/window state separately and retains page properties.
    pub fn fast_forward_screen(&mut self) {
        self.fast_forward_root();
        self.fast_forward_groups();
    }
    pub(super) fn fast_forward_root(&mut self) {
        self.root.fast_forward();
    }
    pub(super) fn fast_forward_groups(&mut self) {
        for bank in 0..2 {
            self.fast_forward_group(bank);
            if self.temporal_fault.is_some() {
                break;
            }
        }
    }
    pub(super) fn fast_forward_group(&mut self, bank: usize) {
        self.screen[bank].fast_forward();
        let count = self
            .owners
            .iter()
            .filter(|owners| owners[bank].is_some())
            .count();
        let owners = &mut self.owners;
        if let Err(error) = self.retiring[bank].fast_forward_with_tail(
            count,
            |owner| {
                owner.fast_forward();
                Ok(())
            },
            || {
                for owner in owners.iter_mut().filter_map(|owners| owners[bank].as_mut()) {
                    owner.fast_forward();
                }
                Ok(())
            },
        ) {
            self.temporal_fault
                .get_or_insert_with(|| format!("{error:#}"));
        }
    }
    /// ADV child2 is ScreenLayer; child1 is MessageLayer, controlled by -3.
    /// The VM calls this target Page, but it is not the optional OverlayLayer.
    pub fn screen_properties(&self) -> &HigurashiProperties {
        &self.root
    }
    pub fn message_properties(&self) -> &HigurashiProperties {
        &self.page
    }
    pub fn render_front_group(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        parent: &crate::layer::render_params::higurashi::HigurashiTransform,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi group failed: {:?}",
            self.visual_fault()
        );
        super::group::render(pass, self.group_children(), &self.screen[0], parent)
    }
    pub(super) fn prepare_front_group(
        &self,
        context: &mut crate::render::PreRenderContext,
        parent: &crate::layer::render_params::higurashi::HigurashiTransform,
        depth: &crate::layer::user::higurashi::effect_capture::SharedDepth,
    ) -> Result<()> {
        ensure!(
            self.visual_fault().is_none(),
            "Higurashi group failed: {:?}",
            self.visual_fault()
        );
        super::group::prepare_children(
            context,
            self.group_children(),
            &self.screen[0],
            parent,
            depth,
        )
    }
    /// Publish prepared back owners and return the old front for the pending
    /// transition. This is bank publication, not a WIPE scheduler/compositor.
    pub fn commit_back(&mut self) -> HigurashiPreviousFront {
        let prepared = self.prepare_back();
        self.publish_back(prepared)
    }
    pub(super) fn prepare_back(&mut self) -> PreparedBack {
        assert!(self.pending_front_ids.is_none(), "WIPE already preparing");
        self.pending_front_ids = Some(std::array::from_fn(|slot| {
            self.owners[slot][0].as_ref().map_or(
                self.state.records[slot][0].words[1],
                HigurashiUserLayer::owner_id,
            )
        }));
        let screen_back = self.state.screen_back;
        PreparedBack {
            promoted: self.state.commit_back(),
            screen_back,
        }
    }
    /// Discard the old command's deferred publication ticket after native
    /// reconstruction. This changes no bank state, live owner or plane.
    pub(super) fn retire_source_command(&mut self) {
        self.pending_front_ids = None;
    }
    pub(super) fn publish_back(&mut self, prepared: PreparedBack) -> HigurashiPreviousFront {
        let mut previous = HigurashiPreviousFront {
            owners: self
                .owners
                .iter_mut()
                .map(|banks| banks[0].take())
                .collect(),
            retiring: super::temporal::Group::new(Vec::new()),
            temporal_fault: self.temporal_fault.clone(),
            owner_ids: self.pending_front_ids.take().expect("prepared WIPE"),
            screen: self.screen[0].clone(),
        };
        let mut changed = [false; SLOTS];
        for t in prepared.promoted {
            changed[t.slot as usize] = true;
        }
        for (slot, banks) in self.owners.iter_mut().enumerate() {
            // Native663c copies unchanged incoming children through virtual38;
            // b919c retains/swaps the original outgoing plane owners.
            // Native6620 leaves an existing BACK key untouched if an unchanged
            // FRONT is absent, even when its logical mapping has been freed.
            if !changed[slot] {
                if let Some(owner) = &previous.owners[slot] {
                    banks[1] = owner.native_copy();
                }
            }
            if changed[slot] {
                let record = &self.state.records[slot][0];
                if matches!(record.layer_type(), 2 | 7) {
                    self.pending_metadata
                        .push((record.layer_type(), record.words[6]));
                }
            }
        }
        // Actual6784: physical BACK destination <- FRONT source, BEFORE the
        // b919c plane swap. Destination compaction keeps every keyed BACK row.
        let count = self
            .owners
            .iter()
            .filter(|banks| banks[1].is_some())
            .count();
        let (front, back) = self.retiring.split_at_mut(1);
        if let Err(error) =
            back[0].copy_retired_from_with_tail(&front[0], count, |owner| Ok(owner.native_copy()))
        {
            self.temporal_fault
                .get_or_insert_with(|| format!("{error:#}"));
        }
        previous.retiring =
            std::mem::replace(&mut front[0], super::temporal::Group::new(Vec::new()));
        front[0] = std::mem::replace(&mut back[0], super::temporal::Group::new(Vec::new()));
        // Physical completed BACK becomes incoming, old FRONT is outgoing;
        // the new back plane starts empty after this native plane transfer.
        for banks in &mut self.owners {
            banks[0] = banks[1].take();
        }
        if prepared.screen_back != 0 {
            self.screen[0] = self.screen[1].clone();
        }
        previous
    }
    pub fn advance(&mut self, delta: Ticks, voice: Option<&dyn Fn(i32, f32) -> Option<u8>>) {
        self.advance_message(delta);
        self.advance_root(delta);
        self.advance_groups(delta, voice);
    }
    pub(super) fn advance_message(&mut self, delta: Ticks) {
        self.page.advance(delta);
    }
    pub(super) fn advance_root(&mut self, delta: Ticks) {
        self.root.advance(delta);
        if let Some(services) = &self.movie_services {
            services.begin_frame();
        }
    }
    pub(super) fn advance_groups(
        &mut self,
        delta: Ticks,
        voice: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        for bank in 0..2 {
            self.advance_group(bank, delta, voice);
            if self.temporal_fault.is_some() {
                break;
            }
        }
    }
    pub(super) fn advance_group(
        &mut self,
        bank: usize,
        delta: Ticks,
        voice: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        self.screen[bank].advance(delta);
        let count = self
            .owners
            .iter()
            .filter(|owners| owners[bank].is_some())
            .count();
        let owners = &mut self.owners;
        if let Err(error) = self.retiring[bank].advance_with_tail(
            delta.as_f32(),
            count,
            |owner, _| {
                owner.advance(delta, voice);
                Ok(())
            },
            || {
                for owner in owners.iter_mut().filter_map(|owners| owners[bank].as_mut()) {
                    owner.advance(delta, voice);
                }
                Ok(())
            },
        ) {
            self.temporal_fault
                .get_or_insert_with(|| format!("{error:#}"));
        }
    }
    fn properties_mut(&mut self, id: i32) -> Option<&mut HigurashiProperties> {
        match id {
            -1 => Some(&mut self.root),
            -2 => Some(&mut self.screen[self.state.screen_back as usize]),
            -3 => Some(&mut self.page),
            _ => None,
        }
    }
    pub async fn dispatch(
        &mut self,
        command: &HigurashiRuntimeCommand,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        mut seeds: impl FnMut() -> [u32; 4],
    ) -> Result<bool> {
        use HigurashiRuntimeCommand as C;
        match command {
            C::LAYERSELECT { args: [from, to] } => self.state.select(*from, *to),
            C::LAYERBACK { argument: id } => {
                let old_screen = self.state.screen_back;
                for t in self.state.start_back(*id)? {
                    self.owners[t.slot as usize][1] = self.owners[t.slot as usize][0]
                        .as_ref()
                        .and_then(HigurashiUserLayer::native_copy);
                }
                if old_screen == 0 && self.state.screen_back == 1 {
                    self.screen[1] = self.screen[0].clone();
                }
            }
            C::LAYERINIT { id } => {
                let targets = self.state.initialize(*id)?;
                if let Some(properties) = self.properties_mut(*id) {
                    properties.reset();
                }
                for t in targets {
                    if let Some(owner) = &mut self.owners[t.slot as usize][t.bank as usize] {
                        owner.properties.reset();
                    }
                }
            }
            C::LAYERCTRL {
                args: [id, property],
                params,
            } => {
                ensure!(
                    (params.2 as u32 & 0x3f) <= 3,
                    "Unsupported Higurashi easing {}",
                    params.2 & 0x3f
                );
                let targets = self
                    .state
                    .control(*id, *property, params.0, params.2 as u32)?;
                let apply = |p: &mut HigurashiProperties| {
                    p.control(
                        *property as usize,
                        params.0 as f32,
                        Ticks::from_f32(params.1 as f32),
                        params.2 as u32,
                    )
                };
                if let Some(properties) = self.properties_mut(*id) {
                    apply(properties)?;
                }
                for t in targets {
                    if let Some(owner) = &mut self.owners[t.slot as usize][t.bank as usize] {
                        apply(&mut owner.properties)?;
                    }
                }
            }
            C::LAYERLOAD {
                args: [id, kind, flags],
                params,
            } => {
                // Prepare first so an asset error cannot leave published state half changed.
                let plan = self.plan_load(*id, *kind, *flags, *params)?;
                let loaded = plan.load(assets, scenario).await?;
                self.publish_load(loaded, &mut seeds)?;
            }
            C::LAYERUNLOAD { args: [id, delay] } => {
                LayerUnload::capture(&mut self.state, *id, *delay)?.complete(self)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    #[cfg(test)]
    pub(crate) fn reference_group_entries(
        &self,
        bank: usize,
    ) -> Vec<(i32, u32, &HigurashiUserLayer)> {
        self.retiring[bank]
            .entries()
            .iter()
            .filter_map(|entry| Some((entry.key, entry.remaining.to_bits(), entry.owner.as_ref()?)))
            .chain(
                self.owners
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, owners)| Some((slot as i32, 1, owners[bank].as_ref()?))),
            )
            .collect()
    }
}

#[cfg(test)]
pub(super) fn replay_snapshot(state: &LayerBanks) -> u64 {
    let mut words = Vec::new();
    words.extend(state.free.map(u32::from));
    words.push(state.allocated as u32);
    words.extend(state.logical_to_slot.map(u32::from));
    words.extend(state.slot_to_logical.map(u32::from));
    words.extend(state.selected.map(u32::from));
    words.extend(state.back.map(u32::from));
    for (dirty, records) in state.dirty.iter().zip(&state.records) {
        words.extend(dirty.map(u32::from));
        for record in records {
            words.extend(record.words.map(|v| v as u32));
        }
    }
    words.extend(state.root.map(|v| v as u32));
    for screen in state.screen {
        words.extend(screen.map(|v| v as u32));
    }
    words.push(u32::from(state.screen_back));
    words.extend(state.page.map(|v| v as u32));
    words.push(state.loads_with_init);
    words.push(u32::from(state.back_started));
    words
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
}

#[cfg(test)]
mod live_native_tests;
#[cfg(test)]
mod live_temporary_integration_tests;
#[cfg(test)]
mod tests;
