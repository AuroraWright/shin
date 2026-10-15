use super::*;

fn fixture() -> Vec<u8> {
    let mut data = b"PIC4".to_vec();
    for word in [2u32, 1104, 0, 0x0001_0001, 1, 1, 42] {
        data.extend_from_slice(&word.to_le_bytes());
    }
    // One descriptor; the fragment starts directly after the 32-byte header.
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&44u32.to_le_bytes());
    data.extend_from_slice(&1060u32.to_le_bytes());
    // Dictionary/inline alpha, no rectangles, 12 bytes of padding, 1x1 image.
    for word in [3u16, 0, 0, 6, 0, 0, 1, 1, 0, 0] {
        data.extend_from_slice(&word.to_le_bytes());
    }
    data.extend_from_slice(&[0; 12]);
    data.extend_from_slice(&[9, 17, 33, 255]);
    data.resize(data.len() + 1020, 0);
    data.extend_from_slice(&[0; 4]);
    assert_eq!(data.len(), 1104);
    data
}

#[test]
fn higurashi_picture_uses_32_byte_header_and_raw_fragment() {
    let data = fixture();
    let header = read_picture_header_with_format(&data, PictureFormat::HigurashiV2).unwrap();
    assert_eq!(
        (header.picture_id, header.block_count, header.scale),
        (42, 1, 4096)
    );
    let prefix = read_picture_header_prefix_with_format(
        &data[..32],
        data.len() as u64,
        PictureFormat::HigurashiV2,
    )
    .unwrap();
    assert_eq!((prefix.picture_id, prefix.scale), (42, 4096));
    assert!(
        read_picture_header_prefix_with_format(&data[..32], 32, PictureFormat::HigurashiV2)
            .is_err()
    );
    let picture =
        read_picture_with_format::<SimpleMergedPicture>(&data, (), PictureFormat::HigurashiV2)
            .unwrap();
    assert_eq!(picture.image.dimensions(), (1, 1));
    assert_eq!(picture.image.as_raw(), &[9, 17, 33, 255]);
    for length in [0, 16, 31] {
        assert!(
            read_picture_with_format::<SimpleMergedPicture>(
                &data[..length],
                (),
                PictureFormat::HigurashiV2
            )
            .is_err()
        );
    }
    let mut invalid = data;
    invalid[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        read_picture_with_format::<SimpleMergedPicture>(&invalid, (), PictureFormat::HigurashiV2)
            .is_err()
    );
}

#[test]
fn higurashi_fragment_preserves_compressed_length_above_64kib() {
    let width = 256u16;
    let height = 64u16;
    let mut raw = vec![0u8; width as usize * 4 * height as usize];
    for (i, b) in raw[..width as usize * 4].iter_mut().enumerate() {
        *b = (i.wrapping_mul(37) % 256) as u8;
    }
    let mut compressed = Vec::new();
    for chunk in raw.chunks(8) {
        compressed.push(0);
        compressed.extend_from_slice(chunk);
    }
    assert!(compressed.len() > u16::MAX as usize);
    let mut block = Vec::new();
    for word in [0u16, 1, 0, 2, 0, 0, width, height] {
        block.extend_from_slice(&word.to_le_bytes());
    }
    block.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    for word in [0u16, 0, width, height] {
        block.extend_from_slice(&word.to_le_bytes());
    }
    block.extend_from_slice(&[0; 4]);
    block.extend_from_slice(&compressed);
    let decoded = read_picture_block_with_format(&block, PictureFormat::HigurashiV2).unwrap();
    assert_eq!(
        decoded.data.dimensions(),
        (u32::from(width), u32::from(height))
    );
    for row in decoded.data.as_raw().chunks_exact(width as usize * 4) {
        assert_eq!(row, &raw[..width as usize * 4]);
    }
}

#[test]
fn umineko_header_policy_stays_fixed_when_version_word_changes() {
    let mut data = b"PIC4".to_vec();
    for word in [2u32, 999, 0, 0x0001_0001, 1, 0, 42, 12345] {
        data.extend_from_slice(&word.to_le_bytes());
    }
    let header = read_picture_header(&data).unwrap();
    assert_eq!(header.scale, 12345);
    let explicit = read_picture_header_with_format(&data, PictureFormat::Umineko).unwrap();
    assert_eq!(explicit.scale, header.scale);
    assert!(read_picture_header_with_format(&data, PictureFormat::HigurashiV2).is_err());
    // The existing decoder still ignores declared version and size.
    assert_eq!(
        read_picture::<SimpleMergedPicture>(&data, ())
            .unwrap()
            .picture_id,
        42
    );
}

#[test]
fn higurashi_overlapping_fragments_follow_descriptor_order() {
    let original = fixture();
    let mut data = original[..32].to_vec();
    data[24..28].copy_from_slice(&3u32.to_le_bytes());
    // Physical fragment offsets run backwards, and the first block is placed
    // again last. Neither offset order nor task completion order is sufficient.
    for offset in [1128u32, 68, 1128] {
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&offset.to_le_bytes());
        data.extend_from_slice(&1060u32.to_le_bytes());
    }
    let mut other = original[44..].to_vec();
    other[32..36].copy_from_slice(&[200, 30, 10, 255]);
    data.extend_from_slice(&other);
    data.extend_from_slice(&original[44..]);
    let size = data.len() as u32;
    data[8..12].copy_from_slice(&size.to_le_bytes());
    let picture =
        read_picture_with_format::<SimpleMergedPicture>(&data, (), PictureFormat::HigurashiV2)
            .unwrap();
    assert_eq!(picture.image.as_raw(), &[9, 17, 33, 255]);
}
