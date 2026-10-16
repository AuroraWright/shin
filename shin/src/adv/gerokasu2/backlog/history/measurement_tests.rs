use super::*;

fn entries(n: usize) -> Vec<Entry> {
    (0..n)
        .map(|i| Entry {
            boundary: i as u32,
            item: if i % 7 == 3 {
                super::super::Item::Checkpoint {
                    title: "章".into(),
                    text: "節".into(),
                }
            } else {
                super::super::Item::Message {
                    message_id: i as u32,
                    message_arg: if i % 3 == 0 { -1 } else { 0 },
                    text: "名前@r本文".into(),
                }
            },
        })
        .collect()
}

#[test]
fn ger_backlog_measurement_outward_order_pending_defaults_and_fixed_origin() {
    let e = entries(7);
    let mut m = Measurement::new(&e, Some(3)).unwrap();
    assert_eq!(m.geometry_ref().allowed, 3..3);
    assert!(
        m.geometry_ref()
            .rows
            .iter()
            .all(|r| r.top == 0. && r.height == 0. && r.voice_count == 0)
    );
    assert!(m.geometry_ref().rows[3].marker);
    let mut published: Vec<usize> = vec![];
    for row in [3, 2, 4, 1, 5, 0, 6] {
        assert_eq!(m.next_row(), Some(row));
        let old: Vec<_> = published
            .iter()
            .map(|&i| (i, m.geometry_ref().rows[i].clone()))
            .collect();
        m.publish(row, 54.25 + row as f32 * 0.125, (row % 2) as u8)
            .unwrap();
        for (i, previous) in old {
            assert_eq!(m.geometry_ref().rows[i], previous);
        }
        published.push(row);
        assert_eq!(m.geometry_ref().allowed.len(), published.len());
        assert_eq!(m.geometry_ref().rows[3].top.to_bits(), 0f32.to_bits());
    }
    assert!(m.complete());
    assert_eq!(m.next_row(), None);
    assert_eq!(m.geometry_ref().allowed, 0..7);
    for (pivot, want) in [(0, vec![0, 1, 2, 3]), (3, vec![3, 2, 1, 0])] {
        let mut m = Measurement::new(&entries(4), Some(pivot)).unwrap();
        let mut actual = vec![];
        while let Some(row) = m.next_row() {
            actual.push(row);
            m.publish(row, 0., 0).unwrap();
        }
        assert_eq!(actual, want);
    }
    assert!(Measurement::new(&[], None).is_none());
}

#[test]
fn ger_backlog_measurement_portrait_floor_and_fractional_accumulation() {
    let e = entries(8);
    for pivot in 0..e.len() {
        let mut m = Measurement::new(&e, Some(pivot)).unwrap();
        let heights = [
            0., 195.99998, 196., 0.125, 196.00002, 54.375, 999.25, 31.625,
        ];
        while let Some(row) = m.next_row() {
            m.publish(row, heights[row], 1).unwrap();
        }
        let g = m.geometry_ref();
        for (i, row) in g.rows.iter().enumerate() {
            let h = if e[i].kind() == 0 && e[i].message_arg() >= 0 {
                heights[i].max(196.)
            } else {
                heights[i]
            };
            assert_eq!(row.height.to_bits(), (h + 36.).to_bits());
        }
        let mut top = 0f32;
        for i in (0..pivot).rev() {
            top -= 8.;
            top -= g.rows[i].height;
            assert_eq!(g.rows[i].top.to_bits(), top.to_bits());
        }
    }
}

#[test]
fn ger_backlog_measurement_ready_at_100_and_cancelled_snapshot_is_atomic() {
    let mut m = Measurement::new(&entries(151), Some(75)).unwrap();
    for count in 0..=100 {
        assert_eq!(m.published_count(), count);
        assert_eq!(m.ready(false), count >= 100);
        assert!(m.ready(true));
        let before = m.geometry();
        assert!(m.publish(usize::MAX, 54., 0).is_err());
        let next = m.next_row().unwrap();
        for height in [f32::NAN, f32::INFINITY, -0.25] {
            assert!(m.publish(next, height, 0).is_err());
            assert_eq!(m.geometry(), before);
            assert_eq!(m.next_row(), Some(next));
        }
        if count != 100 {
            m.publish(next, 54.125, 0).unwrap();
        }
    }
    // Cancellation is a host-owned stop: retaining a snapshot does not advance
    // either the requested row or the measured half-open range.
    let cancelled = m.geometry();
    assert_eq!(m.geometry(), cancelled);
    assert_eq!(m.geometry_ref().allowed, 25..125);
    let short = Measurement::new(&entries(1), None).unwrap();
    assert!(!short.ready(false));
    assert!(short.ready(true));
}

#[test]
fn ger_backlog_measurement_complete_delegate_preserves_prior_geometry_bits() {
    for n in [1, 2, 7, 100, 151] {
        let e = entries(n);
        let measured: Vec<_> = (0..n)
            .map(|i| {
                (
                    [0., 31.125, 54.375, 196.00002, 997.625][i % 5],
                    (i % 4) as u8,
                )
            })
            .collect();
        for pivot in [0, n / 2, n - 1] {
            let actual = super::super::geometry(&e, Some(pivot), &measured)
                .unwrap()
                .unwrap();
            // Independent former all-at-once forward/backward calculation.
            let heights: Vec<_> = e
                .iter()
                .zip(&measured)
                .map(|(entry, &(h, v))| {
                    (if entry.kind() == 0 && v != 0 && entry.message_arg() >= 0 && h < 196. {
                        196.
                    } else {
                        h
                    }) + 36.
                })
                .collect();
            let mut bottom = 0f32;
            for i in pivot..n {
                if i != pivot {
                    bottom += 8.;
                }
                assert_eq!(actual.rows[i].top.to_bits(), bottom.to_bits());
                bottom += heights[i];
            }
            let mut minimum = 0f32;
            for i in (0..pivot).rev() {
                minimum -= 8.;
                minimum -= heights[i];
                assert_eq!(actual.rows[i].top.to_bits(), minimum.to_bits());
            }
            assert_eq!(actual.bottom.to_bits(), bottom.to_bits());
            assert_eq!(actual.minimum.to_bits(), minimum.to_bits());
            assert_eq!(
                actual.maximum.to_bits(),
                (bottom - 832.).max(minimum).to_bits()
            );
            assert_eq!(
                actual.initial_scroll.to_bits(),
                (heights[pivot] - 832.)
                    .min(bottom - 832.)
                    .max(minimum)
                    .to_bits()
            );
        }
    }
}
