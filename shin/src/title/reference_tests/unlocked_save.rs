//! Generate and validate a separate fully unlocked test save from supplied 1.03 tables.
#[test]
#[ignore = "requires supplied patch ROM and explicit new output path"]
fn reference_generate_fully_unlocked_103_progress() {
    use shin_core::{
        format::{rom::RomReader, save::SaveData, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::{Read, Write};
    let patch = std::env::var("SHIN_REFERENCE_PATCH_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(patch).unwrap()).unwrap()).unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes.len(), 9_691_136, "Supplied updated scenario required");
    let fingerprint = bytes.iter().fold(0xcbf29ce484222325u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    assert_eq!(
        fingerprint, 0x7a006c4e2505e117,
        "Exact supplied 1.03 scenario required"
    );
    let scenario = Scenario::new(bytes.into()).unwrap();
    let info = scenario.info_tables();
    let mut save = SaveData::default();
    save.persist_data.set(0, 31);
    for slot in 1..=48 {
        save.persist_data.set(slot, 3);
    }
    for id in 0..info.picture_info.len() {
        save.save_vectors.unlocked_cgs.add(id);
    }
    for id in 0..info.bgm_info.len() {
        save.save_vectors.unlocked_bgms.add(id);
    }
    for id in 0..info.movie_info.len() {
        save.save_vectors.unlocked_movies.add(id);
    }
    let encoded = save.encode();
    let decoded = SaveData::decode(&encoded).unwrap();
    assert_eq!(decoded.encode_raw(), save.encode_raw());
    let setup = crate::title::scenario_select::ScenarioSelectSetup::from_save(&decoded);
    assert_eq!(setup.groups.len(), 6);
    assert_eq!(
        setup.groups.iter().map(|g| g.entries.len()).sum::<usize>(),
        48
    );
    assert!(
        setup
            .groups
            .iter()
            .all(|g| g.entries.iter().all(|e| e.state == 3))
    );
    let title = crate::title::TitleSetup::from_save(&decoded, -1);
    assert_eq!(title.completed_episodes, 0xffff);
    for item in [
        crate::title::TitleItem::Characters,
        crate::title::TitleItem::Tips,
        crate::title::TitleItem::Cg,
        crate::title::TitleItem::Bgm,
        crate::title::TitleItem::Movie,
        crate::title::TitleItem::Bustup,
    ] {
        assert!(title.items.contains(&item));
    }
    let cgs = crate::title::cg_gallery::CgGalleryList::new(&info.picture_box_info, &decoded).unwrap();
    let music =
        crate::title::music_gallery::MusicGalleryList::new(&info.music_box_info, &decoded).unwrap();
    let movies = crate::title::movie_gallery::MovieGalleryList::new(&info.movie_info, &decoded).unwrap();
    assert!(cgs.all_unlocked() && music.all_unlocked());
    assert_eq!(movies.completion_tenths(), 1000);
    assert_eq!(cgs.completion_tenths(), 1000);
    assert_eq!(music.completion_tenths(), 1000);
    assert!(
        decoded.auto_save_slot.is_none() && decoded.manual_save_slots.iter().all(Option::is_none)
    );
    let output = std::env::var("SHIN_REFERENCE_UNLOCKED_SAVE_OUTPUT").unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .unwrap();
    file.write_all(&encoded).unwrap();
    file.sync_all().unwrap();
    assert_eq!(
        SaveData::decode(&std::fs::read(&output).unwrap())
            .unwrap()
            .encode_raw(),
        save.encode_raw()
    );
    println!(
        "Fully unlocked 1.03 save:48 scenarios/six groups, all Characters/Tips episodes, Bustup, CG{} rows/music{} rows/movie{} rows all100%;{} bytes;{output}",
        cgs.rows().len(),
        music.rows().len(),
        movies.rows().len(),
        encoded.len()
    );
}
