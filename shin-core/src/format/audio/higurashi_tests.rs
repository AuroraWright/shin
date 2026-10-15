use super::*;
use crate::{
    format::rom::{IndexEntry, RomReader},
    primitives::stateless_reader::StatelessFile,
};
use std::{io::Read, sync::Arc};

#[test]
fn higurashi_nxa_version_selection_preserves_umineko_contract() {
    let mut bytes = vec![0; 48];
    bytes[..4].copy_from_slice(b"NXA1");
    bytes[8..12].copy_from_slice(&48u32.to_le_bytes());
    bytes[18..20].copy_from_slice(&9u16.to_le_bytes());
    for version in [0u32, 1, 2, 3] {
        bytes[4..8].copy_from_slice(&version.to_le_bytes());
        assert_eq!(read_audio(&bytes).is_ok(), version == 2);
        assert_eq!(
            read_higurashi_audio(&bytes).is_ok(),
            matches!(version, 1 | 2)
        );
    }
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&49u32.to_le_bytes());
    assert!(read_higurashi_audio(&bytes).is_err());
}

#[test]
#[ignore = "reads supplied Higurashi ROM headers and decodes authored voice NXA1 v1 files"]
fn higurashi_nxa_headers_and_authored_pcm16_segments() {
    let root = std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let mut count = 0;
    let mut voices = Vec::new();
    let mut partial = 0;
    let mut versions = [0usize; 3];
    for name in ["data.rom", "append.rom", "patch.rom"] {
        let mut rom = RomReader::new(
            StatelessFile::new(std::fs::File::open(root.join(name)).unwrap()).unwrap(),
        )
        .unwrap();
        let entries: Vec<_> = rom
            .traverse()
            .filter_map(|(path, entry)| match entry {
                IndexEntry::File(entry) if path.ends_with(".nxa") => Some((path, *entry)),
                _ => None,
            })
            .collect();
        for (path, entry) in entries {
            let mut reader = rom.open_file(entry).unwrap();
            let mut header = [0; 48];
            reader.read_exact(&mut header).unwrap();
            let header = NxaHeader::read_le(&mut std::io::Cursor::new(header)).unwrap();
            assert!(matches!(header.version, 1 | 2), "{name}/{path}");
            versions[header.version as usize] += 1;
            assert_eq!(header.file_size, entry.size(), "{name}/{path}");
            assert_eq!(header.info.sample_rate, 48000, "{name}/{path}");
            assert!(matches!(header.info.channel_count, 1 | 2));
            assert!(header.info.frame_size > 0 && header.info.frame_samples > 0);
            partial +=
                usize::from((header.file_size - 48) % u32::from(header.info.frame_size) != 0);
            count += 1;
            if name == "data.rom"
                && matches!(
                    path.as_str(),
                    "/voice/s01/11/vtf_oishi_0001.nxa" | "/voice/s01/11/vtf_oishi_0002.nxa"
                )
            {
                let mut bytes = Vec::new();
                rom.open_file(entry)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                let file = Arc::new(read_higurashi_audio(&bytes).unwrap());
                let mut source = AudioSource::new(AudioDecoder::new_pcm16(file.clone()).unwrap());
                let mut decoded = 0;
                let mut nonzero = false;
                while let Some((left, right)) = source.read_sample() {
                    assert_eq!(left, right, "authored voices are mono");
                    nonzero |= left != 0.0;
                    decoded += 1;
                }
                assert!(nonzero);
                assert_eq!(decoded, file.info().num_samples);
                source.samples_seek(480).unwrap();
                assert!(source.read_sample().is_some());
                source.samples_seek(file.info().num_samples).unwrap();
                assert_eq!(source.read_sample(), None);
                voices.push((path, decoded));
            }
        }
    }
    assert_eq!(voices.len(), 2);
    println!(
        "Higurashi NXA1:{count} supplied headers pass;versions {versions:?};{partial} partial packet payloads; authored voice PCM16 samples {voices:?}; logical EOF/seek pass; no audio hardware"
    );
}
