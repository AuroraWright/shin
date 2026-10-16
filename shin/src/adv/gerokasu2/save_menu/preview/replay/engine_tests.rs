use super::*;
use crate::adv::gerokasu2::layers::SpecialLayer;
use binrw::BinWrite;
use enum_map::Enum;
use shin_core::{
    format::{
        save::gerokasu2::{Progress, ProgressEntry, SaveDate},
        scenario::instruction_elements::{NumberSpec, Register},
    },
    vm::command::types::{LayerProperty, LayerbankId, PlaneId},
};
use std::{
    io::{Cursor, Write},
    process::{Command, Stdio},
};

fn num(d: &mut Vec<u8>, value: i32) {
    let mut out = Cursor::new(Vec::new());
    NumberSpec::constant(value).write_le(&mut out).unwrap();
    d.extend(out.into_inner())
}
fn reg(d: &mut Vec<u8>, value: u16) {
    let mut out = Cursor::new(Vec::new());
    NumberSpec::register(Register::from_regular_register(value))
        .write_le(&mut out)
        .unwrap();
    d.extend(out.into_inner())
}
fn text(d: &mut Vec<u8>, value: &str) {
    d.extend(((value.len() + 1) as u16).to_le_bytes());
    d.extend(value.as_bytes());
    d.push(0)
}
fn info(d: &mut Vec<u8>, level: i32, value: &str) {
    d.push(0xa0);
    num(d, level);
    text(d, value)
}
fn message(d: &mut Vec<u8>) {
    d.push(0x86);
    d.extend(0x01000001u32.to_le_bytes());
    num(d, 0);
    text(d, "本文")
}
fn layer(d: &mut Vec<u8>, id: i32, kind: i32, register: Option<u16>) {
    d.push(0xc1);
    for n in [id, kind, 0] {
        num(d, n)
    }
    d.push(1);
    if let Some(r) = register {
        reg(d, r)
    } else {
        num(d, 15)
    }
}
fn fixture() -> (Arc<Gerokasu2Scenario>, Vec<u8>, u32) {
    // Same minimal ten-table dialect fixture as the existing replay tests.
    let mut d = b"SNR ".to_vec();
    for w in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        d.extend(w.to_le_bytes())
    }
    let mut tables = vec![];
    for i in 0..10 {
        d.extend((0x4c + tables.len() as u32).to_le_bytes());
        if i < 7 {
            tables.extend(4u32.to_le_bytes())
        }
        tables.extend(0u32.to_le_bytes())
    }
    d.extend(tables);
    let start = d.len() as u32;
    d[32..36].copy_from_slice(&start.to_le_bytes());
    info(&mut d, 0, "章");
    info(&mut d, 1, "最初");
    d.push(0x81);
    d.extend(7u16.to_le_bytes());
    num(&mut d, 99);
    layer(&mut d, 0, 1, Some(7));
    d.push(0xcb);
    message(&mut d);
    d.push(0x8e);
    for n in [-1, 0, 30] {
        num(&mut d, n)
    }
    d.push(0);
    d.push(0xa2);
    num(&mut d, 7);
    layer(&mut d, 1, 2, Some(7));
    d.push(0xa2);
    num(&mut d, 8);
    message(&mut d);
    d.push(0xa3);
    layer(&mut d, 9, 9, None);
    d.push(0xe0);
    d.extend(8u16.to_le_bytes());
    num(&mut d, 9);
    d.push(0xc3);
    for n in [9, 0] {
        num(&mut d, n)
    }
    d.push(1);
    reg(&mut d, 8);
    info(&mut d, 1, "保存した見出し");
    d.push(0xa1);
    message(&mut d);
    d.push(0x85);
    num(&mut d, -1);
    d.push(0x87);
    num(&mut d, -1);
    d.extend([0x8a, 0]);
    d.push(0x8d);
    d.extend([0, 0, 0, 0, 7, 0]);
    num(&mut d, 3);
    text(&mut d, "選択");
    d.extend(5u16.to_le_bytes());
    d.extend(b"A\0B\0\0");
    layer(&mut d, 2, 1, Some(7));
    // All four native Message-target integer operations leave the scene alone.
    d.push(0xc0);
    num(&mut d, -5);
    layer(&mut d, -5, 1, None);
    d.push(0xc2);
    num(&mut d, -5);
    num(&mut d, 0);
    d.push(0xc3);
    num(&mut d, -5);
    num(&mut d, 0);
    d.push(1);
    num(&mut d, 33);
    d.push(0xa4);
    d.push(0xa5);
    let size = d.len() as u32;
    d[4..8].copy_from_slice(&size.to_le_bytes());
    (
        Arc::new(Gerokasu2Scenario::new(d.clone().into()).unwrap()),
        d,
        start,
    )
}
fn data(target: u32, seed: u32, active: usize, route: Vec<u8>) -> GameData {
    let mut entries = [ProgressEntry::default(), ProgressEntry::default()];
    entries[active] = ProgressEntry {
        scenario_id: 0,
        random_seed: seed,
        save_position: target,
        replay_target: 99,
        responses: route,
    };
    GameData {
        date: SaveDate::default(),
        progress: Progress::new(active, entries).unwrap(),
    }
}

#[test]
fn ger_save_preview_specialized_collect_stops_inside_events_and_ignores_back_messages() {
    let (scenario, _, _) = fixture();
    let snapshot = collect(scenario, data(1, 42, 0, vec![]), || false).unwrap();
    assert_eq!(snapshot.layers.record(LayerbankId::new(0)).kind(), 1);
    assert_eq!(
        snapshot.layers.record(LayerbankId::new(1)).kind(),
        -1,
        "stop at first EVBEGIN before the next load"
    );
    assert_eq!(
        snapshot.labels.map(|s| s.to_string()),
        ["".to_string(), "".to_string()]
    );
    assert!(collect(fixture().0, data(12, 42, 0, vec![]), || true).is_err());
}

#[test]
#[ignore = "whole original specialized preview replay scene/labels versus engine collect; CPU only"]
fn ger_native_save_preview_collect_layers_labels_pageback_events_and_quiz_match() {
    let root = std::path::PathBuf::from(std::env::var_os("SHIN_GEROKASU2_ROOT").unwrap());
    let out = root.join("gerokasu2/.local/save-preview-replay");
    let binary = std::env::var_os("SHIN_GEROKASU2_SAVE_PREVIEW_REPLAY_ORACLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| out.join("native-save-preview-replay"));
    let (scenario, code, start) = fixture();
    let mut input = vec![];
    let mut expected = vec![];
    for active in 0..2 {
        for seed in [0, u32::MAX] {
            for target in 0..12 {
                for route in [vec![], vec![0xa5, 0x5a, 0xff, 0x80]] {
                    for n in [
                        0x31525053,
                        code.len() as u32,
                        start,
                        0,
                        seed,
                        target,
                        99,
                        active as u32,
                        route.len() as u32,
                        0,
                    ] {
                        input.extend(n.to_le_bytes())
                    }
                    input.extend(&code);
                    input.extend(&route);
                    expected.push(
                        collect(scenario.clone(), data(target, seed, active, route), || {
                            false
                        })
                        .unwrap(),
                    );
                }
            }
        }
    }
    std::fs::write(out.join("engine.input"), &input).unwrap();
    let mut child = Command::new(binary)
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("gerokasu2/.local/system.fnt"),
            root.join("gerokasu2/.local/newrodin-medium.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(out.join("engine.output"), &result.stdout).unwrap();
    std::fs::write(out.join("engine.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    let mut lines = output.lines();
    let mut banks = 0;
    let label = |s: &str| {
        let (_, hex) = s.split_once(':').unwrap();
        String::from_utf8(
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect(),
        )
        .unwrap()
    };
    for (case, snapshot) in expected.iter().enumerate() {
        assert_eq!(lines.next().unwrap(), format!("N {case}"));
        while lines.next().unwrap() != "S" {}
        for (owner, special) in [
            SpecialLayer::Root,
            SpecialLayer::Screen,
            SpecialLayer::Page,
            SpecialLayer::Plane(PlaneId::new(0)),
            SpecialLayer::Plane(PlaneId::new(1)),
            SpecialLayer::Plane(PlaneId::new(2)),
            SpecialLayer::Plane(PlaneId::new(3)),
        ]
        .into_iter()
        .enumerate()
        {
            let f: Vec<u32> = lines
                .next()
                .unwrap()
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse().unwrap())
                .collect();
            assert_eq!(f[0], owner as u32);
            for index in 0..90 {
                assert_eq!(
                    f[index + 1],
                    snapshot
                        .layers
                        .special_property(special, LayerProperty::from_usize(index))
                        as u32,
                    "native preview owner{owner} property{index} case{case}"
                )
            }
        }
        let f: Vec<u32> = lines
            .next()
            .unwrap()
            .split_whitespace()
            .skip(1)
            .map(|s| s.parse().unwrap())
            .collect();
        for plane in 0..4 {
            let m = snapshot.layers.plane_mask(PlaneId::new(plane));
            assert_eq!(
                &f[plane as usize * 2..plane as usize * 2 + 2],
                &[m[0] as u32, m[1] as u32]
            )
        }
        let mut present = [false; 48];
        loop {
            let line = lines.next().unwrap();
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields[0] == "L" {
                assert_eq!(
                    [label(fields[1]), label(fields[2])],
                    snapshot.labels.clone().map(|s| s.to_string()),
                    "native preview exported labels case{case}"
                );
                break;
            }
            assert_eq!(fields[0], "B");
            let f: Vec<u32> = fields[1..].iter().map(|s| s.parse().unwrap()).collect();
            let bank = f[0] as usize;
            present[bank] = true;
            let record = snapshot.layers.record(LayerbankId::new(bank as u8));
            assert_eq!(
                (f[1], f[2], f[3]),
                (
                    record.kind() as u32,
                    record.plane() as u32,
                    record.logical_id() as u32
                ),
                "native preview bank header case{case}"
            );
            assert_eq!(
                &f[8..16],
                &record
                    .parameters()
                    .iter()
                    .map(|v| *v as u32)
                    .collect::<Vec<_>>(),
                "native preview bank params case{case}"
            );
            for index in 0..90 {
                assert_eq!(
                    f[16 + index],
                    record.property(LayerProperty::from_usize(index)) as u32,
                    "native preview bank{bank} property{index} case{case}"
                )
            }
            if record.kind() == 9 {
                assert_eq!(
                    f[106] != 0,
                    record.interaction_completed(),
                    "native preview quiz completion case{case}"
                );
                if record.interaction_completed() {
                    assert_eq!(
                        f[107] as i32,
                        record.interaction_result(),
                        "native preview quiz result case{case}"
                    );
                }
            };
            banks += 1;
        }
        for (bank, found) in present.into_iter().enumerate() {
            assert_eq!(
                found,
                snapshot.layers.record(LayerbankId::new(bank as u8)).kind() >= 0
            )
        }
        assert!(lines.next().unwrap().starts_with("X "));
    }
    assert!(lines.next().is_none());
    println!(
        "GER_SAVE_PREVIEW_COLLECT_MATCH profiles={} banks={banks} all630special_properties_allactive_bank_properties_params_quiz_labels",
        expected.len()
    );
}
