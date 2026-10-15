//! Original64-entry internal capture visits, independently retained outputs.
//! Synthetic native leaves record callbacks; SDK rasterization and stencil
//! truncation beyond255 are not part of this CPU comparison.
use super::*;

const PROFILES: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-temporal-group-render-capture/profiles.json"
));
const ORIGINAL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/live-temporal-group-render-capture/output.txt"
));

#[test]
fn higurashi_temporal_group_matches_56_original_internal_capture_orders_and_5222_visits() {
    let profiles: serde_json::Value = serde_json::from_str(PROFILES).unwrap();
    let profiles = profiles.as_array().unwrap();
    assert_eq!(profiles.len(), 56);
    assert_eq!(ORIGINAL.lines().count(), profiles.len());
    let mut visited = 0;
    let mut prepared = 0;
    let mut maximum = 0;
    for (index, (profile, original)) in profiles.iter().zip(ORIGINAL.lines()).enumerate() {
        let rows = profile["rows"].as_array().unwrap();
        let count = profile["count"].as_u64().unwrap() as usize;
        assert_eq!(rows.len(), count);
        let mut group_properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
        for (i, value) in profile["clip"].as_array().unwrap().iter().enumerate() {
            group_properties
                .tweener_mut(44 + i)
                .unwrap()
                .fast_forward_to(f32::from_bits(value.as_u64().unwrap() as u32));
        }
        // Original6d200 samples none of44..47 and calls no mask service.
        // This property owner is intentionally not substituted for child
        // clipping, or for a separately captured outer framebuffer draw.
        let _authored_group_clip = group_properties;
        let owners: Vec<_> = rows
            .iter()
            .map(|row| {
                let mut layer = HigurashiUserLayer::null([1, 2, 3, 4]);
                layer.set_owner_id(row[1].as_i64().unwrap() as i32);
                layer
                    .properties
                    .tweener_mut(5)
                    .unwrap()
                    .fast_forward_to(f32::from_bits(row[2].as_u64().unwrap() as u32));
                layer
            })
            .collect();
        let children = || {
            owners
                .iter()
                .enumerate()
                .map(|(slot, layer)| Child {
                    owner_id: layer.owner_id(),
                    slot: slot as u8,
                    layer,
                })
                .collect::<Vec<_>>()
        };
        let sorted = ordered(children());
        let mut expected = format!("G {index} P {count}");
        for child in &sorted {
            expected.push_str(&format!(" {}", child.slot));
        }
        let plan = render_plan(children()).unwrap();
        expected.push_str(&format!(" D {}", plan.len()));
        for draw in &plan {
            expected.push_str(&format!(
                " {} {} {}",
                draw.child.slot,
                draw.stencil,
                if draw.pass == PassKind::Opaque { 1 } else { 0 }
            ));
        }
        expected.push_str(" R 0 0 0 0 E 2 1 0");
        assert_eq!(
            expected, original,
            "original6d200 preparation/686d8 pass visits profile{index}"
        );
        prepared += count;
        visited += plan.len();
        maximum = maximum.max(count);
    }
    assert_eq!((prepared, visited, maximum), (2611, 5222, 64));
    eprintln!(
        "Temporal group:56 original internal capture profiles/2611 preparation callbacks/5222 exact child pass-stencil visits through64entries;retiring-prefix input,duplicate owner IDs,finite priorities,authored full/reversed/partial/degenerate/exterior/NaN group44..47 all unread;no>255 wrap,external framebuffer or GPUpixel claim"
    );
}
