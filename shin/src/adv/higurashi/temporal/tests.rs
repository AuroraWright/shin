//! Original temporal ownership, complete-destruction order and actual afterimage
//! packets. No device/window/audio is initialized by this CPU test.
use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Write as _,
    process::{Command, Stdio},
    rc::Rc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Drop(u32),
    Update(u32, u32),
    Skip(u32),
    Copy(u32, u32),
    ImageDrop(u32),
}
type Events = Rc<RefCell<Vec<Event>>>;
struct Leaf {
    id: u32,
    updates: u32,
    skips: u32,
    elapsed: f32,
    null_copy: bool,
    events: Events,
}
impl Drop for Leaf {
    fn drop(&mut self) {
        self.events.borrow_mut().push(Event::Drop(self.id));
    }
}
fn leaf(id: u32, events: &Events) -> Leaf {
    Leaf {
        id,
        updates: 0,
        skips: 0,
        elapsed: 0.0,
        null_copy: false,
        events: events.clone(),
    }
}
struct Image {
    id: u32,
    extent: [u32; 2],
    events: Events,
    retired: Rc<Cell<u32>>,
}
impl Drop for Image {
    fn drop(&mut self) {
        self.retired.set(self.retired.get() + 1);
        self.events.borrow_mut().push(Event::ImageDrop(self.id));
    }
}
fn image(next: &Cell<u32>, retired: &Rc<Cell<u32>>, events: &Events, extent: [u32; 2]) -> Image {
    let id = next.get() + 1;
    next.set(id);
    Image {
        id,
        extent,
        events: events.clone(),
        retired: retired.clone(),
    }
}
fn word(t: &mut std::str::SplitWhitespace<'_>) -> u32 {
    t.next().unwrap().parse().unwrap()
}
fn signed(t: &mut std::str::SplitWhitespace<'_>) -> i32 {
    t.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(t: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(t))
}

#[test]
#[ignore = "requires original updated executable/font and temporal owner oracle"]
fn higurashi_temporal_owners_match_original_swaps_retirement_and_afterimage_packets() {
    let input = std::fs::read_to_string(
        std::env::var("SHIN_HIGURASHI_TEMPORAL_OWNERS_INPUT")
            .unwrap_or_else(|_| "/tmp/higurashi-temporal-owners-input.txt".into()),
    )
    .unwrap();
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TEMPORAL_OWNERS").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copied = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copied.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), input.lines().count() + 1);
    let events: Events = Rc::new(RefCell::new(vec![]));
    let next = Cell::new(0);
    let retired = Rc::new(Cell::new(0));
    let mut depth: Option<Image> = None;
    let mut afterimages: Option<Afterimages<Image>> = None;
    let mut properties = HigurashiProperties::from_seeds([0; 4]);
    let mut groups: [Group<Leaf>; 2] = [Group::new(vec![]), Group::new(vec![])];
    let mut allocated = BTreeMap::new();
    let mut selected = [0, 0];
    let mut packets = 0;
    let mut updates = 0;
    let mut copied = 0;
    let mut maximum_group = 0;
    for (index, (request, response)) in input
        .lines()
        .chain(std::iter::once("E"))
        .zip(output.lines())
        .enumerate()
    {
        events.borrow_mut().clear();
        let mut request = request.split_whitespace();
        let op = request.next().unwrap();
        let mut original = response.split_whitespace();
        assert_eq!(original.next(), Some("O"));
        assert_eq!(word(&mut original), index as u32);
        assert_eq!(original.next(), Some(op));
        let mut captured_targets = vec![];
        let mut original_events = vec![];
        let mut original_packet = None;
        let mut snapshots = [vec![], vec![]];
        let mut screen_snapshot = None;
        while let Some(tag) = original.next() {
            match tag {
                "N" => {
                    let _ = words::<4>(&mut original);
                }
                "R" | "I" => {
                    let _ = words::<2>(&mut original);
                }
                "X" => {
                    let [id, count] = words::<2>(&mut original);
                    if count == 0 {
                        original_events.push(Event::ImageDrop(id));
                    }
                }
                "F" => original_events.push(Event::Drop(word(&mut original))),
                "U" => {
                    let [id, delta] = words::<2>(&mut original);
                    original_events.push(Event::Update(id, delta));
                    updates += 1;
                }
                "K" => original_events.push(Event::Skip(word(&mut original))),
                "C" => {
                    let [id, null] = words::<2>(&mut original);
                    original_events.push(Event::Copy(id, null));
                    copied += 1;
                }
                "D" => {
                    original_packet = Some(words::<42>(&mut original));
                    packets += 1;
                }
                "E" => assert_eq!(words::<2>(&mut original), [16, 0]),
                "B" => {
                    let n = word(&mut original);
                    for _ in 0..n {
                        captured_targets.push(words::<3>(&mut original));
                    }
                }
                "G" => {
                    assert!(word(&mut original) < 2);
                }
                "W" => assert_eq!(word(&mut original), 1),
                "P" => assert_eq!(word(&mut original) != 0, original_packet.is_some()),
                "H" => assert_eq!(word(&mut original), 0),
                "S" => screen_snapshot = Some(words::<7>(&mut original)),
                "V" => {
                    let b = word(&mut original) as usize;
                    let n = word(&mut original);
                    maximum_group = maximum_group.max(n);
                    for _ in 0..n {
                        let key = signed(&mut original) as u32;
                        let rest = words::<5>(&mut original);
                        snapshots[b].push([key, rest[0], rest[1], rest[2], rest[3], rest[4]]);
                    }
                }
                other => panic!("row{index} unknown{other}"),
            }
        }
        match op {
            "N" => {
                depth = None;
                afterimages = None;
                let extent = words::<2>(&mut request);
                let current = image(&next, &retired, &events, extent);
                depth = Some(image(&next, &retired, &events, extent));
                afterimages = Some(Afterimages::new(extent, current));
                properties = HigurashiProperties::from_seeds([0; 4]);
            }
            "T" => properties
                .tweener_mut(39)
                .unwrap()
                .fast_forward_to(f32::from_bits(word(&mut request))),
            "P" => {
                let i = word(&mut request) as usize;
                properties
                    .tweener_mut(i)
                    .unwrap()
                    .fast_forward_to(f32::from_bits(word(&mut request)));
            }
            "Q" => {
                let [target, time] = words::<2>(&mut request).map(f32::from_bits);
                let flags = word(&mut request);
                properties
                    .control(39, target, shin_core::time::Ticks::from_f32(time), flags)
                    .unwrap();
            }
            "A" => {
                let delta = f32::from_bits(word(&mut request));
                properties.advance(shin_core::time::Ticks::from_f32(delta));
                let sampled = properties.values()[39];
                afterimages
                    .as_mut()
                    .unwrap()
                    .after_update(sampled, |extent| {
                        Ok(image(&next, &retired, &events, extent))
                    })
                    .unwrap();
            }
            "Z" => properties.fast_forward(),
            "D" => {
                let packet = afterimages.as_ref().unwrap().packet(&properties);
                assert_eq!(
                    packet.is_some(),
                    original_packet.is_some(),
                    "row{index} native afterimage drawgate"
                );
                if let Some(packet) = packet {
                    let actual = original_packet.unwrap();
                    assert_eq!(&actual[..5], &[6, 5, 1, 4, 0]);
                    assert_eq!(
                        actual[5],
                        afterimages.as_ref().unwrap().previous().unwrap().id
                    );
                    assert_eq!(
                        &actual[6..22],
                        &packet.projection.to_cols_array().map(f32::to_bits),
                        "row{index}694f4/afterimage matrix"
                    );
                    assert_eq!(
                        &actual[22..26],
                        &packet.color.to_array().map(f32::to_bits),
                        "row{index}native truncated weight"
                    );
                    let vertices: Vec<_> = packet
                        .vertices
                        .into_iter()
                        .flat_map(|v| {
                            let p = v.position;
                            let uv = v.texture_position;
                            [p.x.to_bits(), p.y.to_bits(), uv.x.to_bits(), uv.y.to_bits()]
                        })
                        .collect();
                    assert_eq!(&actual[26..42], vertices.as_slice());
                }
            }
            "C" => {} // Original Screen virtual38 NULL was asserted by the C fixture.
            "F" => {
                depth = None;
                afterimages = None;
            }
            "G" => {
                groups = [Group::new(vec![]), Group::new(vec![])];
                allocated.clear();
                selected = [0, 0];
                let n = word(&mut request);
                for b in 0..2 {
                    groups[b] = Group::new(
                        (0..n)
                            .map(|i| Entry {
                                key: i as i32,
                                remaining: 0.,
                                owner: Some(leaf(100 + b as u32 * 100 + i, &events)),
                            })
                            .collect(),
                    );
                }
                for i in 0..n {
                    allocated.insert((i * 2) as i32, i);
                }
            }
            "S" => selected = [signed(&mut request), signed(&mut request)],
            "L" => {
                let id = signed(&mut request);
                let delay = signed(&mut request);
                let bank = word(&mut request);
                let expected: Vec<_> = if id >= 0 {
                    allocated
                        .get(&id)
                        .map(|&slot| vec![[id as u32, slot, bank]])
                        .unwrap_or_default()
                } else if id == -4 {
                    allocated
                        .iter()
                        .filter(|(id, _)| **id >= selected[0] && **id <= selected[1])
                        .map(|(&id, &slot)| [id as u32, slot, bank])
                        .collect()
                } else {
                    vec![]
                };
                assert_eq!(
                    captured_targets, expected,
                    "row{index}actual9678 selected coords"
                );
                for [id, slot, b] in expected {
                    if b == 0 {
                        allocated.remove(&(id as i32));
                    }
                    groups[b as usize].unload(slot as i32, delay as f32);
                }
            }
            "I" => {
                let [b, key, id, null] = words::<4>(&mut request);
                let mut l = leaf(id, &events);
                l.null_copy = null != 0;
                groups[b as usize].publish(key as i32, l);
            }
            "R" => {
                let b = word(&mut request) as usize;
                let key = signed(&mut request);
                let delay = f32::from_bits(word(&mut request));
                groups[b].unload(key, delay);
            }
            "U" => {
                let b = word(&mut request) as usize;
                let delta = f32::from_bits(word(&mut request));
                groups[b]
                    .advance(delta, |l, d| {
                        l.updates += 1;
                        l.elapsed += d;
                        l.events.borrow_mut().push(Event::Update(l.id, d.to_bits()));
                        Ok(())
                    })
                    .unwrap();
            }
            "K" => {
                let b = word(&mut request) as usize;
                groups[b]
                    .fast_forward(|l| {
                        l.skips += 1;
                        l.events.borrow_mut().push(Event::Skip(l.id));
                        Ok(())
                    })
                    .unwrap();
            }
            "J" => {
                let b = word(&mut request) as usize;
                groups[b].entries_mut()[0].owner.as_mut().unwrap().null_copy = true;
            }
            "M" => {
                let (front, back) = groups.split_at_mut(1);
                front[0]
                    .copy_retired_from(&back[0], |source| {
                        source
                            .events
                            .borrow_mut()
                            .push(Event::Copy(source.id, source.null_copy as u32));
                        if source.null_copy {
                            return Ok(None);
                        }
                        let mut l = leaf(source.id + 10000, &events);
                        l.elapsed = source.elapsed;
                        Ok(Some(l))
                    })
                    .unwrap();
            }
            "E" => {
                depth = None;
                afterimages = None;
                groups = [Group::new(vec![]), Group::new(vec![])];
            }
            unknown => panic!("request{unknown}"),
        }
        assert!(request.next().is_none());
        if let Some(original) = screen_snapshot {
            let actual = if let Some(a) = &afterimages {
                assert_eq!(a.current().extent, depth.as_ref().unwrap().extent);
                [
                    a.current().extent[0],
                    a.current().extent[1],
                    a.current().id,
                    a.previous().map_or(0, |t| t.id),
                    next.get(),
                    retired.get(),
                    properties.values()[39].to_bits(),
                ]
            } else {
                [0, 0, 0, 0, next.get(), retired.get(), 0]
            };
            assert_eq!(
                actual, original,
                "row{index}native1498/14a0/allocation/retirement"
            );
            for b in 0..2 {
                let actual: Vec<_> = groups[b]
                    .entries()
                    .iter()
                    .map(|e| {
                        let l = e.owner.as_ref();
                        [
                            e.key as u32,
                            e.remaining.to_bits(),
                            l.map_or(0, |l| l.id),
                            l.map_or(0, |l| l.updates),
                            l.map_or(0, |l| l.skips),
                            l.map_or(0, |l| l.elapsed.to_bits()),
                        ]
                    })
                    .collect();
                assert_eq!(
                    actual, snapshots[b],
                    "row{index}nativegroup{b}retained vector"
                );
            }
        }
        assert_eq!(
            *events.borrow(),
            original_events,
            "row{index}complete destruction/update/skip/copy order"
        );
    }
    assert_eq!(
        (packets, updates, copied, maximum_group),
        (128, 809, 38, 64)
    );
    assert_eq!(next.get(), retired.get());
    eprintln!(
        "Higurashi temporal owners:1374 original actions/128 exact afterimage packets,809 leafupdates,38 retired nativecopy calls,64 simultaneous groupchildren;swap/no-renderadvance/complete-droporder/NULLholes pass;no GPU/audio/windows/user saves"
    );
}
