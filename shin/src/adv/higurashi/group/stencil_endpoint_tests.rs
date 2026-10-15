//! Ordinary reserve1 groups compared to original686d8→697f0, including wrap.
use super::*;
use shin_core::time::Ticks;
#[test]
fn higurashi_ordinary_group_stencil_wrap_matches_original_endpoint() {
    let profiles: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/group-stencil-ordinary-capture/profiles.json"
    )))
    .unwrap();
    let output = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/group-stencil-ordinary-capture/output.txt"
    ));
    let mut visits = 0;
    for (profile, line) in profiles.as_array().unwrap().iter().zip(output.lines()) {
        let rows = profile["rows"].as_array().unwrap();
        let layers: Vec<_> = rows
            .iter()
            .map(|r| {
                let mut layer = HigurashiUserLayer::null([1, 2, 3, 4]);
                layer
                    .properties
                    .control(
                        5,
                        f32::from_bits(r[1].as_u64().unwrap() as u32),
                        Ticks::ZERO,
                        0,
                    )
                    .unwrap();
                layer
            })
            .collect();
        let children = layers
            .iter()
            .enumerate()
            .map(|(i, layer)| Child {
                owner_id: rows[i][0].as_i64().unwrap() as i32,
                slot: (i % 32) as u8,
                layer,
            })
            .collect();
        let plan = render_plan(children).unwrap();
        let tokens: Vec<_> = line.split_whitespace().collect();
        let mut at = 7 + rows.len();
        assert_eq!(plan.len(), rows.len() * 2);
        for draw in plan {
            assert_eq!(tokens[at], "D");
            let slot = layers
                .iter()
                .position(|l| std::ptr::eq(l, draw.child.layer))
                .unwrap();
            let raw = tokens[at + 2].parse::<u32>().unwrap();
            assert_eq!(slot, tokens[at + 1].parse::<usize>().unwrap());
            assert_eq!(draw.stencil, raw as u8);
            assert_eq!(
                u8::from(draw.pass == PassKind::Opaque),
                tokens[at + 3].parse::<u8>().unwrap()
            );
            assert_eq!(
                tokens[at + 10].parse::<u32>().unwrap(),
                ((draw.stencil as u32) << 16) | 0xffff
            );
            at += 11;
            visits += 1;
        }
        assert_eq!(tokens[at], "E");
    }
    assert_eq!(visits, 3034);
    eprintln!(
        "8 actual ordinary group profiles/{visits} original draw visits, including255/256/257/300/384 children; signed duplicate IDs, fractional priorities, two passes and effective stencil byte wrap"
    );
}
