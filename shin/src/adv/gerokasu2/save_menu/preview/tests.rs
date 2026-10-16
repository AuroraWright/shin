use super::*;

#[test]
fn desktop_replay_failure_releases_channel_and_slot_change_allows_retry() {
    let mut owner = Controller::default();
    let occupied = [true; 100];
    owner.frame(0., &occupied, Completions::default());
    let failed = owner.replay_request().unwrap();
    owner.replay_failed(failed);
    owner.frame(0., &occupied, Completions::default());
    assert_eq!(owner.replay_request().unwrap().slot, 1);
    owner.replay_failed(failed); // Stale completion cannot release another row.
    assert_eq!(owner.replay_request().unwrap().slot, 1);
    let other = owner.replay_request().unwrap();
    owner.invalidate(failed.slot);
    owner.replay_failed(other);
    owner.frame(0., &occupied, Completions::default());
    assert_eq!(owner.replay_request().unwrap().slot, failed.slot);

    let discarded = owner.replay_request().unwrap();
    owner.invalidate(discarded.slot);
    owner.replay_failed(discarded);
    owner.frame(0., &occupied, Completions::default());
    assert_eq!(owner.replay_request().unwrap().slot, discarded.slot);
}

#[test]
fn preview_replay_precedes_texture_and_hidden_snapshot_survives() {
    let mut owner = Controller::default();
    let mut occupied = [false; 100];
    occupied[0] = true;
    owner.frame(0., &occupied, Completions::default());
    let replay = owner.replay_request().unwrap();
    let events = owner.frame(
        0.,
        &occupied,
        Completions {
            replay: Some((replay, false)),
            texture: None,
        },
    );
    assert!(events.contains(&Event::ReplayPublished(replay)));
    assert!(owner.texture_request().is_none());
    owner.frame(0., &occupied, Completions::default());
    let texture = owner.texture_request().unwrap();
    let events = owner.frame(
        2000.,
        &occupied,
        Completions {
            replay: None,
            texture: Some(texture),
        },
    );
    assert!(events.contains(&Event::AbandonTexture(texture)));
    assert!(!owner.has_texture(0));
    assert!(owner.has_snapshot(0));
    owner.frame(0., &occupied, Completions::default());
    assert!(owner.replay_request().is_none());
    assert_ne!(owner.texture_request(), Some(texture));
}

#[test]
fn preview_invalidated_replay_is_discarded_without_blocking_later_retry() {
    let mut owner = Controller::default();
    let occupied = [true; 100];
    owner.frame(0., &occupied, Completions::default());
    let old = owner.replay_request().unwrap();
    owner.invalidate(old.slot);
    let events = owner.frame(
        0.,
        &occupied,
        Completions {
            replay: Some((old, true)),
            texture: None,
        },
    );
    assert!(events.contains(&Event::ReplayDiscarded(old)));
    assert!(!owner.has_snapshot(old.slot));
    owner.frame(0., &occupied, Completions::default());
    let new = owner.replay_request().unwrap();
    assert_ne!(old, new);
    owner.frame(
        0.,
        &occupied,
        Completions {
            replay: Some((old, false)),
            texture: None,
        },
    );
    assert!(!owner.has_snapshot(old.slot));
    assert_eq!(owner.replay_request(), Some(new));
}

#[test]
fn preview_movie_stills_and_downsampling_keep_native_arguments() {
    let request = Descriptor {
        kind: 6,
        parameters: [3, 99, -4, 5, 6, 7, 8, 9],
    };
    assert_eq!(
        scene_descriptor(request, |_| Some(-7)),
        Descriptor {
            kind: 2,
            parameters: [-7, 0, -4, 5, 6, 7, 8, 9]
        }
    );
    assert_eq!(
        scene_descriptor(request, |_| None),
        Descriptor { kind: 0, ..request }
    );
    assert_eq!(
        capture_extents(THUMBNAIL_SIZE),
        [[960, 540], [480, 270], [336, 189]]
    );
    assert_eq!(
        Labels::Saved {
            first_is_empty: false
        }
        .second_line(),
        Some((Font::Secondary, [564, 79]))
    );
    assert_eq!(
        Labels::Saved {
            first_is_empty: true
        }
        .second_line(),
        None
    );
    assert!(row_visible(0, 373.));
    assert!(!row_visible(0, 373.00003));
}
