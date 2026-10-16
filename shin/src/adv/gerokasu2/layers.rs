//! Native integer layer state before drawable loading, publication and tweens.
//! Gerokasu2 keeps the shared 48-bank/four-plane allocator and 90 properties (Gerokasu 91),
//! but selected layers use address -6. No live draw owners are created here.
use anyhow::{Result, bail, ensure};
use enum_map::Enum;
use shin_core::vm::{
    command::{
        RuntimeCommand,
        types::{LAYERBANKS_COUNT, LayerId, LayerProperty, LayerbankId, PLANES_COUNT, PlaneId},
    },
    gerokasu2::Gerokasu2RuntimeCommand,
};

use crate::adv::vm_state::layers::{
    LayerOperationTarget, LayerOperationTargetList, LayerbankAllocator,
};

/// Native record words hold the shared 90 properties.
const PROPERTIES: usize = 90;
/// Gerokasu's vertical blur radius (`BlurRadiusY`) lives outside the words.
const ALL_PROPERTIES: usize = 91;

/// Property 90 exists only in Gerokasu (see `gerokasu_property_id`).
pub(crate) fn property_count() -> i32 {
    if crate::cli::Game::current() == crate::cli::Game::Gerokasu {
        ALL_PROPERTIES as i32
    } else {
        PROPERTIES as i32
    }
}
const RECORD_WORDS: usize = 0x1ac / 4;
const PROPERTY_WORD: usize = 15;

fn initial_values() -> [i32; ALL_PROPERTIES] {
    std::array::from_fn(|index| LayerProperty::from_usize(index).initial_value())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialLayer {
    Root,
    Screen,
    Page,
    Plane(PlaneId),
}

#[derive(Debug, Clone)]
pub struct LayerRecord {
    // Preserve the untouched native words until their consumers are verified.
    words: [i32; RECORD_WORDS],
    /// Full ADV banks insert this resource generation at word3. The isolated
    /// integer helper has no corresponding word and keeps its107-word layout.
    initialization_id: u32,
    blur_y: i32,
}
impl LayerRecord {
    fn empty() -> Self {
        let mut words = [0; RECORD_WORDS];
        words[0] = -1;
        Self {
            words,
            initialization_id: 0,
            blur_y: 0,
        }
    }
    pub fn kind(&self) -> i32 {
        self.words[0]
    }
    pub fn plane(&self) -> i32 {
        self.words[1]
    }
    pub fn logical_id(&self) -> i32 {
        self.words[2]
    }
    pub fn property(&self, property: LayerProperty) -> i32 {
        match property.into_usize() {
            index @ 0..PROPERTIES => self.words[PROPERTY_WORD + index],
            _ => self.blur_y,
        }
    }
    pub fn parameters(&self) -> &[i32] {
        &self.words[7..15]
    }
    pub fn interaction_completed(&self) -> bool {
        self.words[105] & 0xff != 0
    }
    pub fn interaction_result(&self) -> i32 {
        self.words[106]
    }
    pub fn initialization_id(&self) -> u32 {
        self.initialization_id
    }
    /// Original full VM bank at3DE4+bank*1B0: header0..7, params8..15,
    /// properties16..105, completion106 and result107. This is a numeric
    /// descriptor; its generation identifies a retained resource, not a handle.
    pub fn adv_words(&self) -> [i32; 108] {
        let mut words = [0; 108];
        words[..3].copy_from_slice(&self.words[..3]);
        words[3] = self.initialization_id as i32;
        words[4..].copy_from_slice(&self.words[3..]);
        words
    }
    /// Retain all108 full ADV words, including opaque header and stale fields.
    /// Allocator/plane consistency is the containing scene's responsibility;
    /// converting a record does not allocate or publish a live drawable.
    pub fn from_adv_words(adv: [i32; 108]) -> Self {
        let mut words = [0; RECORD_WORDS];
        words[..3].copy_from_slice(&adv[..3]);
        words[3..].copy_from_slice(&adv[4..]);
        Self {
            words,
            initialization_id: adv[3] as u32,
            blur_y: 0,
        }
    }
    fn reset_properties(&mut self) {
        self.words[PROPERTY_WORD..PROPERTY_WORD + PROPERTIES]
            .copy_from_slice(&initial_values()[..PROPERTIES]);
        self.blur_y = 0;
    }
}

#[derive(Debug, Clone)]
struct PlaneState {
    properties: [i32; ALL_PROPERTIES],
    mask: [i32; 2],
}

#[derive(Debug, Clone)]
pub struct Gerokasu2Layers {
    allocator: LayerbankAllocator,
    selected: [u16; 2],
    current_plane: PlaneId,
    back_started: bool,
    loads_with_init: u32,
    next_initialization_id: u32,
    root: [i32; ALL_PROPERTIES],
    screen: [i32; ALL_PROPERTIES],
    page: [i32; ALL_PROPERTIES],
    planes: [PlaneState; PLANES_COUNT],
    records: [LayerRecord; LAYERBANKS_COUNT],
}

impl Default for Gerokasu2Layers {
    fn default() -> Self {
        Self {
            allocator: LayerbankAllocator::new(),
            selected: [0, 0],
            current_plane: PlaneId::new(0),
            back_started: false,
            loads_with_init: 0,
            next_initialization_id: 0,
            root: initial_values(),
            screen: initial_values(),
            page: initial_values(),
            planes: std::array::from_fn(|_| PlaneState {
                properties: initial_values(),
                mask: [-1, 0],
            }),
            records: std::array::from_fn(|_| LayerRecord::empty()),
        }
    }
}

impl Gerokasu2Layers {
    pub fn selected_range(&self) -> [u16; 2] {
        self.selected
    }
    pub fn current_plane(&self) -> PlaneId {
        self.current_plane
    }
    pub fn loads_with_init(&self) -> u32 {
        self.loads_with_init
    }
    /// Original VM3DE0: next resource generation. Separate from3DDC's count of
    /// property-reset loads; matching requests and existing CardSelect reuse it.
    pub fn next_initialization_id(&self) -> u32 {
        self.next_initialization_id
    }
    pub fn back_started(&self) -> bool {
        self.back_started
    }
    pub fn plane_mask(&self, plane: PlaneId) -> [i32; 2] {
        self.planes[usize::from(plane.raw())].mask
    }
    pub fn begin_back(&mut self) {
        self.back_started = true;
    }
    pub fn record(&self, bank: LayerbankId) -> &LayerRecord {
        &self.records[usize::from(bank.raw())]
    }
    pub fn complete_interaction(&mut self, bank: LayerbankId, result: i32) {
        let record = &mut self.records[usize::from(bank.raw())];
        record.words[105] = (record.words[105] & !0xff) | 1;
        record.words[106] = result;
    }
    pub fn target(&self, logical: LayerId) -> Option<LayerOperationTarget> {
        self.allocator
            .get_layerbank_id(self.current_plane, logical)
            .map(|layerbank| LayerOperationTarget {
                layer: logical,
                layerbank,
            })
    }
    pub fn special_property(&self, layer: SpecialLayer, property: LayerProperty) -> i32 {
        self.special_values(layer)[property.into_usize()]
    }
    fn special(&self, id: i32) -> Option<SpecialLayer> {
        match id {
            -1 => Some(SpecialLayer::Root),
            -2 => Some(SpecialLayer::Screen),
            -3 => Some(SpecialLayer::Page),
            -4 => Some(SpecialLayer::Plane(self.current_plane)),
            _ => None,
        }
    }
    fn special_values(&self, layer: SpecialLayer) -> &[i32; ALL_PROPERTIES] {
        match layer {
            SpecialLayer::Root => &self.root,
            SpecialLayer::Screen => &self.screen,
            SpecialLayer::Page => &self.page,
            SpecialLayer::Plane(plane) => &self.planes[usize::from(plane.raw())].properties,
        }
    }
    fn special_values_mut(&mut self, layer: SpecialLayer) -> &mut [i32; ALL_PROPERTIES] {
        match layer {
            SpecialLayer::Root => &mut self.root,
            SpecialLayer::Screen => &mut self.screen,
            SpecialLayer::Page => &mut self.page,
            SpecialLayer::Plane(plane) => &mut self.planes[usize::from(plane.raw())].properties,
        }
    }
    fn logical_range(&self, id: i32) -> Result<Option<[u16; 2]>> {
        if self.special(id).is_some() {
            return Ok(None);
        }
        let range = if id == -6 {
            self.selected
        } else if (0..256).contains(&id) {
            [id as u16; 2]
        } else {
            bail!("Unsupported Gerokasu2 layer address {id}");
        };
        ensure!(
            range[0] <= range[1] && range[1] < 256,
            "Gerokasu2 selected range is outside logical layer storage"
        );
        Ok(Some(range))
    }
    pub fn targets(&mut self, id: i32) -> Result<LayerOperationTargetList> {
        let Some([from, to]) = self.logical_range(id)? else {
            return Ok(Default::default());
        };
        Ok(self
            .allocator
            .layers_in_range(self.current_plane, LayerId::new(from), LayerId::new(to)))
    }
    /// Native d897c sorts signed words, then retains their low16 bits.
    /// Subsequent storage operations check that the resulting range is valid.
    pub fn select(&mut self, from: i32, to: i32) {
        self.selected = [from.min(to) as u16, from.max(to) as u16];
    }
    pub fn select_plane(&mut self, plane: i32) -> Result<()> {
        ensure!(
            (0..PLANES_COUNT as i32).contains(&plane),
            "Invalid Gerokasu2 plane {plane}"
        );
        self.current_plane = PlaneId::new(plane as u8);
        Ok(())
    }
    pub fn initialize(&mut self, id: i32) -> Result<LayerOperationTargetList> {
        if let Some(special) = self.special(id) {
            *self.special_values_mut(special) = initial_values();
            return Ok(Default::default());
        }
        let targets = self.targets(id)?;
        for target in &targets {
            self.records[usize::from(target.layerbank.raw())].reset_properties();
        }
        Ok(targets)
    }
    pub fn load(
        &mut self,
        id: i32,
        kind: i32,
        flags: i32,
        params: [i32; 8],
    ) -> Result<LayerOperationTargetList> {
        let Some([from, to]) = self.logical_range(id)? else {
            return Ok(Default::default());
        };
        let mut targets = LayerOperationTargetList::new();
        for logical in from..=to {
            let logical = LayerId::new(logical);
            let Some(bank) = self.allocator.alloc_layerbank(self.current_plane, logical) else {
                continue;
            };
            let record = &mut self.records[usize::from(bank.raw())];
            let keep = record.kind() >= 0 && flags & 2 != 0;
            // Original8350: same kind plus all eight matching parameters keeps
            // the resource generation. Existing CardSelect always retains it;
            // changes still clear its completion byte below. Property reset
            // and loads_with_init are governed independently by flags2.
            let reusable =
                record.kind() == kind && (kind == 9 || record.parameters() == params.as_slice());
            if !reusable {
                record.initialization_id = self.next_initialization_id;
                self.next_initialization_id = self.next_initialization_id.wrapping_add(1);
            }
            record.words[0] = kind;
            record.words[1] = i32::from(self.current_plane.raw());
            record.words[7..15].copy_from_slice(&params);
            if kind == 9 {
                record.words[105] &= !0xff;
            }
            if !keep {
                record.reset_properties();
                record.words[2] = i32::from(logical.raw());
                record.words[3] = -1;
                self.loads_with_init = self.loads_with_init.wrapping_add(1);
            }
            targets
                .push(LayerOperationTarget {
                    layer: logical,
                    layerbank: bank,
                })
                .expect("at most48 banks");
        }
        Ok(targets)
    }
    pub fn unload(&mut self, id: i32) -> Result<LayerOperationTargetList> {
        let targets = self.targets(id)?;
        for target in &targets {
            self.records[usize::from(target.layerbank.raw())].words[0] = -1;
            self.allocator
                .free_layerbank(self.current_plane, target.layer);
        }
        Ok(targets)
    }
    pub fn control(
        &mut self,
        id: i32,
        property: i32,
        value: i32,
        flags: i32,
    ) -> Result<LayerOperationTargetList> {
        ensure!(
            (0..property_count()).contains(&property),
            "Invalid Gerokasu2 property {property}"
        );
        let index = property as usize;
        let apply = |old: &mut i32| {
            *old = if flags & 0x80 != 0 {
                old.wrapping_add(value)
            } else {
                value
            }
        };
        if let Some(special) = self.special(id) {
            apply(&mut self.special_values_mut(special)[index]);
            return Ok(Default::default());
        }
        let targets = self.targets(id)?;
        for target in &targets {
            let record = &mut self.records[usize::from(target.layerbank.raw())];
            apply(if index < PROPERTIES {
                &mut record.words[PROPERTY_WORD + index]
            } else {
                &mut record.blur_y
            });
        }
        Ok(targets)
    }
    pub fn swap(&mut self, from: i32, to: i32) {
        // d88fc..d8910 reject negative from, to>255 and equal endpoints.
        // Valid storage indices are required for both IDs by this host API.
        if !(0..256).contains(&from) || !(0..256).contains(&to) || from == to {
            return;
        }
        let from = LayerId::new(from as u16);
        let to = LayerId::new(to as u16);
        self.allocator.swap_layerbanks(self.current_plane, from, to);
        for id in [from, to] {
            if let Some(bank) = self.allocator.get_layerbank_id(self.current_plane, id) {
                self.records[usize::from(bank.raw())].words[2] = i32::from(id.raw());
            }
        }
    }
    pub fn clear_plane(&mut self) -> LayerOperationTargetList {
        let targets =
            self.allocator
                .layers_in_range(self.current_plane, LayerId::new(0), LayerId::new(255));
        for target in &targets {
            self.records[usize::from(target.layerbank.raw())].words[0] = -1;
            self.allocator
                .free_layerbank(self.current_plane, target.layer);
        }
        self.planes[usize::from(self.current_plane.raw())].mask[0] = -1;
        targets
    }
    pub fn set_mask(&mut self, mask: i32, flags: i32) {
        self.planes[usize::from(self.current_plane.raw())].mask = [mask, flags];
    }
    pub fn clear_mask(&mut self) {
        self.planes[usize::from(self.current_plane.raw())].mask[0] = -1;
    }
    /// Apply verified integer state only. Returning true does not execute the
    /// drawable loading, tween/wipe scheduling or command completion protocol.
    pub fn apply(&mut self, command: &Gerokasu2RuntimeCommand) -> Result<bool> {
        use Gerokasu2RuntimeCommand as C;
        match command {
            C::LayerSelect { start, end } => self.select(*start, *end),
            C::LayerInit { layer_id } => {
                self.initialize(*layer_id)?;
            }
            C::LayerLoad {
                layer_id,
                layer_type,
                flags,
                params,
            } => {
                self.load(
                    *layer_id,
                    *layer_type,
                    *flags,
                    [
                        params.0, params.1, params.2, params.3, params.4, params.5, params.6,
                        params.7,
                    ],
                )?;
            }
            C::LayerUnload { layer_id, .. } => {
                self.unload(*layer_id)?;
            }
            C::LayerCtrl {
                layer_id,
                property_id,
                params,
            } => {
                self.control(*layer_id, *property_id, params.0, params.2)?;
            }
            C::Shared(RuntimeCommand::LAYERSWAP(c)) => self.swap(c.arg1, c.arg2),
            C::PlaneSelect { plane } => self.select_plane(*plane)?,
            C::MaskLoad { mask_id, flags, .. } => self.set_mask(*mask_id, *flags),
            C::Shared(RuntimeCommand::MASKUNLOAD(_)) => self.clear_mask(),
            C::Shared(RuntimeCommand::PAGEBACK(_)) => self.begin_back(),
            C::Wipe { .. } => self.back_started = false,
            C::Shared(RuntimeCommand::PLANECLEAR(_)) => {
                self.clear_plane();
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    #[cfg(test)]
    pub(crate) fn reference_adv_bank_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + 48 * 0x1b0);
        bytes.extend(self.loads_with_init.to_le_bytes());
        bytes.extend(self.next_initialization_id.to_le_bytes());
        for record in &self.records {
            bytes.extend(record.adv_words().into_iter().flat_map(i32::to_le_bytes));
        }
        bytes
    }
    #[cfg(test)]
    pub(crate) fn reference_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0; 0x609c];
        let put_words = |bytes: &mut [u8], start: usize, words: &[i32]| {
            for (index, value) in words.iter().enumerate() {
                bytes[start + index * 4..start + index * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        };
        put_words(&mut bytes, 0x90, &self.root);
        put_words(&mut bytes, 0x1f8, &self.screen);
        put_words(&mut bytes, 0x360, &self.page);
        for (index, plane) in self.planes.iter().enumerate() {
            put_words(&mut bytes, 0x4c8 + index * 0x170, &plane.properties);
            put_words(&mut bytes, 0x630 + index * 0x170, &plane.mask);
        }
        bytes[0xa88..0x104c].copy_from_slice(&self.allocator.reference_bytes());
        for (index, value) in self.selected.iter().enumerate() {
            bytes[0x104c + index * 2..0x104e + index * 2].copy_from_slice(&value.to_le_bytes());
        }
        put_words(&mut bytes, 0x1050, &[i32::from(self.current_plane.raw())]);
        bytes[0x1054] = u8::from(self.back_started);
        put_words(&mut bytes, 0x1058, &[self.loads_with_init as i32]);
        for (index, record) in self.records.iter().enumerate() {
            put_words(&mut bytes, 0x105c + index * 0x1ac, &record.words);
        }
        bytes
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod adv_banks_tests;
