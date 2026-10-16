use super::*;

#[test]
fn centered_scale_clips_to_extended_viewport_and_maps_uv_without_rounding() {
    assert_eq!(thumbnail(0, 0.0, 0.0), vec4(150.0, 203.0, 534.0, 419.0));
    let r = thumbnail(0, 23.25, 1.0);
    let (crop, uv) = clipped(r, vec4(0.0, 0.0, 1.0, 1.0)).unwrap();
    assert_eq!(crop.y, 185.0);
    assert_eq!(crop.x, r.x);
    assert!((uv.y - (185.0 - r.y) / (r.w - r.y)).abs() < 0.000001);
    assert_eq!(uv.w, 1.0);
    assert!(clipped(thumbnail(100, 0.0, 0.0), Vec4::ONE).is_none());
}

#[test]
fn partial_variant_count_uses_original_number_glyphs_and_thumbnail_transform() {
    let rect = thumbnail(0, 0.0, 0.0);
    let glyphs = variant_glyphs(1, 12, rect);
    assert_eq!(glyphs.len(), 4);
    assert_eq!(glyphs[0].0, vec4(447.5, 383.0, 471.5, 415.0));
    assert_eq!(glyphs[1].1, vec4(240.0 / 512.0, 0.0, 264.0 / 512.0, 0.4));
    assert_eq!(glyphs[3].0.z, 523.0 + 5.5);
    let scaled = variant_glyphs(1, 12, thumbnail(0, 0.0, 1.0));
    assert!((scaled[0].0.x - (342.0 + (447.5 - 342.0) * 1.05)).abs() < 0.0001);
}

#[test]
fn cg_scrollbar_uses_726_pixel_extent_and_truncated_scroll() {
    assert!(scrollbar(726.0, 0.0).is_empty());
    assert!(scrollbar(1000.0, 275.0).is_empty());
    let slices = scrollbar(1000.0, 274.9);
    assert_eq!(slices.len(), 6);
    assert_eq!(slices[0].0, vec4(1788.0, 190.0, 1804.0, 198.0));
    assert_eq!(slices[5].0.w, 916.0);
    assert_eq!(slices, scrollbar(999.2, 274.1));
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_cg_artwork_programs_glyphs_footer_and_geometry() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x48514, 0x94021ce7),
        (0x48524, 0x94021d1f),
        (0x4860c, 0x1e200920),
        (0x48614, 0x1e282809),
        (0x487ac, 0x94021bf5),
        (0x4880c, 0x9401f9c1),
        (0x48818, 0xb94042c2),
        (0x48828, 0x540002e2),
        (0x48854, 0x94000107),
        (0x48880, 0x9401f964),
        (0x488ac, 0x1e200823),
        (0x488cc, 0x940037dd),
        (0x488e0, 0x9401f91c),
        (0x4890c, 0x940037cd),
        (0x48920, 0x9401f90c),
        (0x489a4, 0x9401f8c3),
        (0x489d4, 0x9401f8cb),
        (0x489f8, 0x9401f906),
        (0x48a18, 0x9401f8a6),
        (0x48a38, 0x9401f8b2),
        (0x48a50, 0x9401f8f0),
        (0x48a84, 0x940298f3),
        (0x48ba0, 0x940000c8),
        (0x48bd4, 0x5a9f0101),
        (0x48be0, 0x940000b8),
        (0x48bf4, 0x940000b3),
        (0x48d48, 0x4b140273),
        (0x48e04, 0x1e292800),
        (0x48e18, 0x1e232903),
        (0x48e50, 0x1e2a0842),
        (0x48e6c, 0x51001508),
        (0x490d0, 0x1e202940),
    ] {
        assert_eq!(word(a), expected, "native CG artwork at {a:x}");
    }
    for (a, expected) in [
        (0x159724, 0.05f32),
        (0x159508, -384.0),
        (0x1596a0, -216.0),
        (0x159548, 384.0),
        (0x15982c, 216.0),
        (0x15998c, 185.0),
        (0x159ac8, 921.0),
        (0x159580, 1788.0),
        (0x1599d0, 1872.0),
        (0x15965c, 1012.0),
        (0x15944c, 1.0 / 512.0),
        (0x159a68, 80.0),
        (0x159444, 320.0),
        (0x159a1c, 192.0),
    ] {
        assert_eq!(f32::from_bits(word(a)), expected);
    }
    let record = |a: usize| std::array::from_fn::<_, 4, _>(|i| word(a + i * 4) as i32);
    for digit in 0..10 {
        assert_eq!(
            record(0x16d5e0 + digit * 16),
            [digit as i32 * 24, 0, 24, 32]
        );
    }
    assert_eq!(record(0x16d680), [264, 0, 14, 32]);
    assert_eq!(record(0x16d690), [240, 0, 24, 32]);
    for (i, expected) in [[7, 0, 0, 206], [4, -1, 48, 72], [3, -1, 96, 74]]
        .into_iter()
        .enumerate()
    {
        assert_eq!(record(0x16d6a0 + i * 16), expected);
    }
    for (a, name) in [
        (0x160cc2, "num"),
        (0x16133a, "help_cgmode"),
        (0x162f78, "%d/%d"),
    ] {
        assert_eq!(&exe[a + 256..a + 256 + name.len()], name.as_bytes());
        assert_eq!(exe[a + 256 + name.len()], 0);
    }
}
