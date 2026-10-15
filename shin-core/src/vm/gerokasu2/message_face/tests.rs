use super::*;
use crate::format::text::utf8::U16Utf8String;

fn table() -> Vec<BustupInfoItem> {
    vec![
        BustupInfoItem {
            name: U16Utf8String::new("コロネ立ち絵A_ドレス"),
            emotion: U16Utf8String::new("通常_なし"),
            lipsync_character_id: u16::MAX,
            raw_placement: [-164, 80, 800],
        },
        BustupInfoItem {
            name: U16Utf8String::new("日本"),
            emotion: U16Utf8String::new("悲しみ"),
            lipsync_character_id: 3,
            raw_placement: [i16::MIN, i16::MAX, -1000],
        },
    ]
}

#[test]
fn retained_face_and_async_publication_follow_message_boundaries() {
    let table = table();
    let mut state = MessageFaceState::new(-1);
    let portrait = MessagePortrait::<u32>::new();
    assert_eq!(
        state.on_msgface(0, false, &table),
        PortraitChange::Unchanged
    );
    assert_eq!(state.retained_index(), 0);
    assert_eq!(portrait.generation(), 0);
    let job = portrait.apply(state.on_message_installed(&table)).unwrap();
    assert_eq!(job.request().path, "/bustup/M/コロネ立ち絵A_ドレス.bup");
    assert_eq!(job.request().expression, "通常_なし");
    assert_eq!(job.request().lipsync_character_id, -1);
    assert_eq!(
        job.request()
            .properties()
            .map(|(property, value)| (property as u32, value)),
        [
            (0, -960),
            (1, 540),
            (20, -164),
            (21, 80),
            (12, 800),
            (13, 800)
        ]
    );
    assert!(job.publish(0));
    let old = portrait.apply(state.on_msgface(1, true, &table)).unwrap();
    assert_eq!(portrait.visible(), Some(0));
    let current = portrait.apply(state.on_msgface(0, true, &table)).unwrap();
    assert!(!old.is_current());
    assert!(!old.publish(1));
    assert_eq!(portrait.visible(), Some(0));
    assert!(current.publish(2));
    assert_eq!(portrait.visible(), Some(2));
    let pending = portrait.apply(state.on_msgface(0, true, &table)).unwrap();
    assert!(portrait.apply(state.on_msgface(-1, true, &table)).is_none());
    assert_eq!(portrait.visible(), None);
    assert!(!pending.publish(3));
    assert_eq!(state.on_msgface(2, true, &table), PortraitChange::Clear);
    let pending = portrait.apply(state.on_msgface(0, true, &table)).unwrap();
    drop(portrait);
    assert!(!pending.is_current());
    assert!(!pending.publish(4));
}

fn compare_native(table: &[BustupInfoItem], requests: &[[u32; 4]]) {
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let mut child = Command::new(
        std::env::var_os("SHIN_GEROKASU2_MESSAGE_FACE_ORACLE")
            .expect("SHIN_GEROKASU2_MESSAGE_FACE_ORACLE"),
    )
    .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("SHIN_GEROKASU2_EXECUTABLE"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()
    .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    input.write_all(b"NMF1").unwrap();
    input
        .write_all(&(table.len() as u32).to_le_bytes())
        .unwrap();
    for item in table {
        for text in [item.name.as_str(), item.emotion.as_str()] {
            input.write_all(&(text.len() as u16).to_le_bytes()).unwrap();
            input.write_all(text.as_bytes()).unwrap();
        }
        input
            .write_all(&item.lipsync_character_id.to_le_bytes())
            .unwrap();
        for word in item.raw_placement {
            input.write_all(&word.to_le_bytes()).unwrap();
        }
    }
    let mut state = MessageFaceState::new(-1);
    let portrait = MessagePortrait::<u32>::new();
    let mut deferred = BTreeMap::<u32, PortraitJob<u32>>::new();
    let mut queued_count = 0;
    for (case, &[op, active, word, cancel]) in requests.iter().enumerate() {
        for word in requests[case] {
            input.write_all(&word.to_le_bytes()).unwrap();
        }
        let change = match op {
            0 => state.on_msgface(word as i32, active != 0, table),
            1 => PortraitChange::Unchanged,
            2 => state.on_message_installed(table),
            _ => unreachable!(),
        };
        let expected_action = match change {
            PortraitChange::Unchanged => 0,
            PortraitChange::Clear => 1,
            PortraitChange::Load(_) => 2,
        };
        let (job, queued) = if op == 1 {
            (Some(deferred.remove(&word).unwrap()), u32::MAX)
        } else {
            let job = portrait.apply(change);
            let queued = if job.is_some() {
                let id = queued_count;
                queued_count += 1;
                id
            } else {
                u32::MAX
            };
            (job, queued)
        };
        let mut expected_properties = [0u32; 6];
        let mut expected_constructed = 0;
        let (path, expression, lip) = if let Some(job) = job {
            let request = job.request().clone();
            if cancel == 4 {
                deferred.insert(queued, job);
            } else {
                if cancel == 1 {
                    portrait.clear();
                }
                if job.is_current() {
                    if cancel == 2 {
                        portrait.clear();
                    }
                    if job.is_current() {
                        expected_constructed = 1;
                        expected_properties = request
                            .properties()
                            .map(|(_, value)| (value as f32).to_bits());
                        if cancel == 3 {
                            portrait.clear();
                        }
                        job.publish(request.bustup_index as u32);
                    }
                }
            }
            (
                request.path,
                request.expression,
                request.lipsync_character_id as u32,
            )
        } else {
            (String::new(), String::new(), 0)
        };
        let mut header = [0u8; 68];
        output
            .read_exact(&mut header)
            .unwrap_or_else(|e| panic!("native face case {case}: {e}"));
        let get = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        assert_eq!(&header[..4], b"NMO1");
        assert_eq!(
            (
                get(4),
                get(8),
                get(12),
                get(16),
                get(20),
                get(24),
                get(52),
                get(64)
            ),
            (
                state.retained_index() as u32,
                expected_action,
                portrait.generation(),
                portrait.visible().unwrap_or(u32::MAX),
                expected_constructed,
                if expected_constructed != 0 { 63 } else { 0 },
                queued,
                lip
            ),
            "case {case}"
        );
        for (index, &value) in expected_properties.iter().enumerate() {
            assert_eq!(get(28 + index * 4), value, "property {index} case {case}");
        }
        assert_eq!(
            (get(56), get(60)),
            (path.len() as u32, expression.len() as u32)
        );
        let mut bytes = vec![0; path.len() + expression.len()];
        output.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes[..path.len()], path.as_bytes());
        assert_eq!(&bytes[path.len()..], expression.as_bytes());
    }
    drop(input);
    assert_eq!(output.read(&mut [0u8; 1]).unwrap(), 0);
    assert!(child.wait().unwrap().success());
}

fn synthetic_requests() -> Vec<[u32; 4]> {
    vec![
        [0, 0, 0, 0],
        [2, 0, 0, 0], // inactive factory retains selection for next MSGSET
        [0, 1, 1, 4],
        [0, 1, 0, 4],
        [1, 0, 1, 0],
        [1, 0, 2, 0], // out-of-order completion
        [0, 1, 1, 4],
        [0, 1, u32::MAX, 0],
        [1, 0, 3, 0], // clear invalidates work
        [0, 1, 0, 0],
        [0, 0, 1, 0],
        [2, 0, 0, 0], // inactive keeps visible until next text
        [0, 1, 0, 1],
        [0, 1, 0, 2],
        [0, 1, 0, 3], // cancellation at worker stages
        [0, 1, 2, 0],
        [0, 1, i32::MIN as u32, 0],
        [0, 1, 0, 0],
        [0, 1, 0, 0], // bounds, repeated selection
    ]
}

#[test]
#[ignore = "requires supplied executable and guarded message-face CPU oracle"]
fn gerokasu2_message_face_native_control_and_publication() {
    let requests = synthetic_requests();
    compare_native(&table(), &requests);
    println!("GEROKASU2_MSGFACE_SYNTHETIC cases={}", requests.len());
}

#[test]
#[ignore = "requires supplied Gerokasu2 ROM/executable and guarded CPU oracle"]
fn gerokasu2_all_scenario_portraits_match_native_requests_and_placement() {
    use crate::format::{rom::RomReader, scenario::gerokasu2::Gerokasu2Scenario};
    use shin_primitives::stateless_reader::StatelessFile;
    use std::io::Read;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(
                std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM"),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Gerokasu2Scenario::new(bytes.into()).unwrap();
    let requests = (0..scenario.bustup_info.len())
        .map(|index| [0, 1, index as u32, 0])
        .collect::<Vec<_>>();
    compare_native(&scenario.bustup_info, &requests);
    assert_eq!(requests.len(), 644);
    println!(
        "GEROKASU2_MSGFACE_SUPPLIED cases={} UTF8_paths_expressions=verified signed_lipsync=verified properties=verified",
        requests.len()
    );
}
