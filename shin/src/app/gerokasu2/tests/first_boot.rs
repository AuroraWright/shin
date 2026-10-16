use super::{system_menu::driver, *};

/// (mean RGB, pixels that are strongly red)
fn summary(pixels: &[u8]) -> ([u32; 3], usize) {
    let mut sum = [0u64; 3];
    let mut red = 0;
    for p in pixels.chunks_exact(4) {
        for c in 0..3 {
            sum[c] += u64::from(p[c]);
        }
        red += usize::from(p[0] > 180 && p[1] < 60 && p[2] < 60);
    }
    let n = (pixels.len() / 4) as u64;
    (sum.map(|s| (s / n) as u32), red)
}

#[test]
#[ignore = "actual /splash.txa FirstBoot caution and logo crossfades before the boot gate on Metal/exact Loopback Audio"]
fn ger_app_first_boot_shows_caution_and_logos_then_publishes_title() {
    let gpu = Offscreen::new();
    let (_temporary, _path, mut d) = driver(&gpu, false);
    d.app.host = None;
    let boot = shin_tasks::block_on(crate::app::gerokasu2::first_boot::FirstBoot::load(
        d.app.assets.clone(),
    ))
    .unwrap();
    d.app.first_boot = Some(boot);

    // Update numbers where each entry (then the null end) starts. The state
    // machine spends one update leaving Hold, as 5BE40 does.
    let mut starts = Vec::new();
    let mut frames = Vec::new();
    let mut current = None;
    let mut update = 0;
    while d.app.first_boot.is_some() && d.app.loading_title.is_none() {
        update += 1;
        d.app.frame.advance();
        d.app.poll_title_destination();
        d.app.update_first_boot(Ticks::from_f32(1.0));
        assert!(d.app.error.is_none(), "{:?}", d.app.error);
        let Some(boot) = d.app.first_boot.as_ref() else {
            break;
        };
        let (index, name, overlay) = boot.reference_state();
        if update == 1 || name != current {
            starts.push((update, index, name));
            current = name;
        }
        if update == 1 {
            assert!((overlay - 59. / 60.).abs() < 1e-6, "{overlay}");
        }
        // Fully revealed frame of each entry, then the black end.
        if overlay == 0.0 && frames.len() < starts.len() {
            frames.push((name, summary(&d.capture_any())));
        }
        assert!(update < 800, "FirstBoot did not finish");
    }
    assert_eq!(
        starts,
        vec![
            (1, 0, Some("caution")),
            (242, 1, Some("black")),
            (313, 2, Some("07thExpansion")),
            (494, 3, Some("entergram")),
            (675, 4, None),
        ]
    );
    assert_eq!(update, 735, "the null entry fades 60 ticks, then the gate runs");
    for (name, (mean, red)) in &frames {
        println!("GEROKASU2_FIRST_BOOT_FRAME {name:?} mean={mean:?} red={red}");
    }
    let mean = |i: usize| frames[i].1.0;
    assert!(frames[0].1.1 > 50, "caution draws its red WARNING");
    assert!(mean(0).iter().all(|c| (30..170).contains(c)), "caution backdrop");
    assert_eq!(mean(1), [0, 0, 0], "black");
    assert!(mean(2).iter().all(|&c| c > 200), "07th Expansion is white");
    assert!(mean(3).iter().all(|&c| (5..80).contains(&c)), "Entergram on black");
    assert_eq!(mean(4), [0, 0, 0], "black end");

    // DF9B0 without the opening bit publishes Title.
    assert!(d.app.loading_title.is_some());
    let started = Instant::now();
    while d.app.title.is_none() {
        d.app.frame.advance();
        d.app.poll_title_destination();
        if d.app.first_boot.is_some() {
            d.app.update_first_boot(Ticks::from_f32(1.0));
        }
        assert!(d.app.error.is_none(), "{:?}", d.app.error);
        assert!(started.elapsed() < Duration::from_secs(30), "Title publication");
    }
    assert!(d.app.first_boot.is_none() && d.app.title_demo.is_none());
    println!("GEROKASU2_FIRST_BOOT updates={update} title_publication={:?}", started.elapsed());
}
