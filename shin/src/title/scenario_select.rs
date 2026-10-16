//! Data owned by native Start's ScenarioSelect scene (constructor dc970).
//! The resource worker, transitions and episode factory are still separate work.
mod details;
#[cfg(test)]
mod frame_reference;
mod input;
mod opening;
#[cfg(test)]
mod render_reference;
pub(super) mod scene;
mod selection;
#[cfg(test)]
mod tab_reference;
#[cfg(test)]
mod unlock_reference;
mod view;
use shin_core::format::save::SaveData;

/// Six 96-byte records at1736a8, reached through native2254e0.
const GROUPS: [&[u32]; 6] = [
    &[
        0x00000001, 0x00104102, 0x00208203, 0x0030c304, 0x00410405, 0x00514506, 0x00618607,
        0x0071c708,
    ],
    &[
        0x00820009, 0x0092010a, 0x00a2020b, 0x00b2030c, 0x00c2040d, 0x00d2050e, 0x00e2060f,
        0x00f20710,
    ],
    &[
        0x01040011, 0x01140112, 0x01240213, 0x01340314, 0x01440415, 0x01540516, 0x01640617,
        0x01740718,
    ],
    &[
        0x01860819, 0x0196491a, 0x01a68a1b, 0x01b6cb1c, 0x01c70c1d, 0x01d74d1e, 0x01e78e1f,
        0x01f7cf20, 0x02081021, 0x02185122, 0x02289223, 0x0238d324, 0x02491425, 0x02595526,
        0x02699627, 0x0279d728, 0x028a1829, 0x029a592a, 0x02aa9a2b, 0x02badb2c,
    ],
    &[0x02cb1c2d, 0x02db5d2e],
    &[0x02eb9e2f, 0x02fbdf30],
];
const ENTRY_HEIGHT: i32 = 110;
const ENTRY_GAP: i32 = 2;
const VIEWPORT_HEIGHT: i32 = 670;
const ATLAS_PATH: &str = "/snrsel.txa";
const MASK_PATH: &str = "/curtain.msk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScenarioEntry {
    pub persistent_slot: i8,
    /// Cell in the four-column, eight-row menu texture (de680).
    pub menu_index: u8,
    /// Named details texture selected by ddf98..ddfb8.
    pub details_index: u8,
    /// Passed to native e4cd0 by the confirmed selection at df990.
    pub scenario_id: u8,
    pub state: i16,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScenarioGroup {
    /// Original metadata index, retained when earlier groups are omitted.
    pub id: u8,
    pub entries: Vec<ScenarioEntry>,
    pub has_new: bool,
    /// Native group430; includes the minimum visible height.
    pub content_height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScenarioSelectSetup {
    pub groups: Vec<ScenarioGroup>,
}

impl ScenarioSelectSetup {
    pub fn from_save(save: &SaveData) -> Self {
        let groups = GROUPS
            .iter()
            .enumerate()
            .filter_map(|(id, records)| {
                let entries: Vec<_> = records
                    .iter()
                    .enumerate()
                    .map(|(index, &packed)| {
                        let persistent_slot = packed as i8;
                        let mut state = save.persist_data.get(persistent_slot as usize) as i16;
                        if persistent_slot == 1 && state < 2 {
                            state = 2;
                        }
                        ScenarioEntry {
                            persistent_slot,
                            menu_index: ((packed >> 8) & 63) as u8,
                            details_index: ((packed >> 14) & 63) as u8,
                            scenario_id: ((packed >> 20) & 63) as u8,
                            state,
                            y: index as i32 * (ENTRY_HEIGHT + ENTRY_GAP),
                        }
                    })
                    .collect();
                // Native retains locked entries, but discards an entirely locked group.
                if entries.iter().all(|entry| entry.state < 1) {
                    return None;
                }
                Some(ScenarioGroup {
                    id: id as u8,
                    has_new: entries.iter().any(|entry| entry.state == 1),
                    content_height: (entries.len() as i32 * (ENTRY_HEIGHT + ENTRY_GAP) - ENTRY_GAP)
                        .max(VIEWPORT_HEIGHT),
                    entries,
                })
            })
            .collect();
        Self { groups }
    }
}

impl ScenarioEntry {
    pub fn details_name(&self) -> String {
        // Rendering uses state<1, although confirmation rejects exactly zero.
        format!(
            "details_{:02}",
            if self.state < 1 {
                48
            } else {
                self.details_index
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_chapter_is_available_and_locked_entries_are_retained() {
        let mut save = SaveData::default();
        save.persist_data.set(1, -32768);
        let setup = ScenarioSelectSetup::from_save(&save);
        assert_eq!(setup.groups.len(), 1);
        let group = &setup.groups[0];
        assert_eq!(group.id, 0);
        assert_eq!(group.entries.len(), 8);
        assert_eq!(group.entries[0].state, 2);
        assert_eq!(group.entries[1].state, 0);
        assert!(!group.has_new);
        assert_eq!(group.content_height, 894);
        assert_eq!(group.entries[7].y, 784);
        save.persist_data.set(1, 32767);
        assert_eq!(
            ScenarioSelectSetup::from_save(&save).groups[0].entries[0].state,
            32767
        );
    }

    #[test]
    fn sparse_group_visibility_preserves_authored_ids_and_signed_states() {
        let mut save = SaveData::default();
        save.persist_data.set(25, 1);
        save.persist_data.set(26, -3);
        save.persist_data.set(48, 2);
        let setup = ScenarioSelectSetup::from_save(&save);
        assert_eq!(
            setup
                .groups
                .iter()
                .map(|group| group.id)
                .collect::<Vec<_>>(),
            [0, 3, 5]
        );
        let group = &setup.groups[1];
        assert!(group.has_new);
        assert_eq!(group.entries.len(), 20);
        assert_eq!(group.entries[1].state, -3);
        assert_eq!(group.content_height, 2238);
        assert_eq!(group.entries[19].y, 2128);
        let last = &setup.groups[2];
        assert!(!last.has_new);
        assert_eq!(last.content_height, 670);
        assert_eq!(last.entries[1].scenario_id, 47);
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_start_group_records_and_availability_constructor() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word =
            |address| u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        let pointer = (0x11e040..0x152eb8)
            .step_by(24)
            .find_map(|offset| {
                let address =
                    u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap());
                (address == 0x2254e0).then(|| {
                    u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap())
                })
            })
            .unwrap();
        assert_eq!(pointer, 0x1736a8);
        for (address, path) in [(0x15cda9, ATLAS_PATH), (0x15ebbe, MASK_PATH)] {
            assert_eq!(
                &exe[address + 256..address + 256 + path.len()],
                path.as_bytes()
            );
        }
        for (id, records) in GROUPS.iter().enumerate() {
            let base = pointer as usize + id * 96;
            assert_eq!(
                [word(base), word(base + 4), word(base + 8), word(base + 12)],
                [
                    id as u32,
                    ENTRY_HEIGHT as u32,
                    ENTRY_GAP as u32,
                    records.len() as u32
                ]
            );
            for (index, &packed) in records.iter().enumerate() {
                assert_eq!(word(base + 16 + index * 4), packed);
            }
            for index in records.len()..20 {
                assert_eq!(word(base + 16 + index * 4), 0);
            }
        }
        for (address, expected) in [
            (0xdcf68, 0x13001e61),
            (0xdcf70, 0x97ffd118),
            (0xdcf74, 0x13003c09),
            (0xdcf80, 0x7100093f),
            (0xdcf84, 0x7a4ab100),
            (0xdcf8c, 0x1a890117),
            (0xdcfc4, 0x53083668),
            (0xdcfc8, 0x530e4e69),
            (0xdcfd4, 0x53146668),
            (0xdcff0, 0x390d13e8),
            (0xdcffc, 0x1a9fa7f3),
            (0xdd1f0, 0x0a13037b),
            (0xdd210, 0x710a7b9f),
            (0xdd220, 0x3600037b),
            (0xdf990, 0xb9400d00),
            (0xdf998, 0x940014ce),
            (0xde680, 0xb9400728),
            (0xde684, 0x53037d09),
            (0xde690, 0x12000908),
            (0xde698, 0x1e281003),
            (0xde730, 0xb9401328),
            (0xde734, 0x71000d1f),
            (0xde73c, 0x7100051f),
            (0xde744, 0x35000ae8),
            (0xde4a8, 0xd2e86e28),
            (0xde4b4, 0xf2c80008),
            (0xde4b8, 0xf2e88cc8),
            (0xddf90, 0x7100055f),
            (0xddf94, 0x540000ab),
            (0xddfa8, 0x321c07e2),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        for (address, expected) in [
            (0x159450, 440.0f32),
            (0x159670, 86.0),
            (0x1599b4, -246.0),
            (0x1598e4, -44.0),
            (0x1598a4, 44.0),
            (0x159360, -40.0),
            (0x159818, 96.0),
            (0x15997c, 40.0),
            (0x159864, -90.0),
            (0x1599e0, 90.0),
        ] {
            assert_eq!(f32::from_bits(word(address)), expected, "native{address:x}");
        }
        for (address, expected) in [
            (0x160656, "menu"),
            (0x161bde, "star"),
            (0x15c0c3, "new"),
            (0x160aa8, "lock"),
            (0x15be45, "details_%02d"),
        ] {
            assert_eq!(
                &exe[address + 256..address + 256 + expected.len()],
                expected.as_bytes()
            );
        }
        let mut save = SaveData::default();
        for slot in 1..=48 {
            save.persist_data.set(slot, 3);
        }
        let setup = ScenarioSelectSetup::from_save(&save);
        let entries: Vec<_> = setup
            .groups
            .iter()
            .flat_map(|group| &group.entries)
            .collect();
        assert_eq!(entries.len(), 48);
        for (index, entry) in entries.iter().enumerate() {
            assert_eq!(entry.persistent_slot, (index + 1) as i8);
            assert_eq!(entry.scenario_id, index as u8);
        }
        // Groups1/2 share one details index per group; other groups advance it.
        assert!(
            setup.groups[1]
                .entries
                .iter()
                .all(|entry| entry.details_index == 8)
        );
        assert!(
            setup.groups[2]
                .entries
                .iter()
                .all(|entry| entry.details_index == 16)
        );
        for group in [
            &setup.groups[0],
            &setup.groups[3],
            &setup.groups[4],
            &setup.groups[5],
        ] {
            assert!(
                group
                    .entries
                    .iter()
                    .all(|entry| entry.details_index == entry.scenario_id)
            );
        }
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); decodes Start picker artwork"]
    fn reference_rom_start_picker_atlas_and_curtain_decode() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut read = |path: &str| {
            let file = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        let atlas =
            shin_core::format::texture_archive::read_texture_archive(&read(ATLAS_PATH)).unwrap();
        assert!(!atlas.name_to_index.is_empty());
        let mut save = SaveData::default();
        for slot in 1..=48 {
            save.persist_data.set(slot, 3);
        }
        for group in ScenarioSelectSetup::from_save(&save).groups {
            for entry in group.entries {
                assert!(atlas.name_to_index.contains_key(&entry.details_name()));
            }
        }
        assert!(atlas.name_to_index.contains_key("details_48"));
        let mut names: Vec<_> = atlas.name_to_index.keys().collect();
        names.sort();
        println!("Start picker atlas entries: {names:?}");
        let mask = shin_core::format::mask::read_mask(&read(MASK_PATH)).unwrap();
        drop(mask);
    }
}
