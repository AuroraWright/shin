use super::*;

#[test]
fn eviction_retains_pinned_and_recent_entries_and_releases_oldest_unowned() {
    let mut cache = Residency::default();
    let owners: Vec<_> = (0..1056)
        .map(|key| {
            let value = Arc::new(key);
            assert!(cache.insert(key, value.clone()).is_empty());
            value
        })
        .collect();
    assert_eq!(
        cache.entries.len(),
        1056,
        "Native limit cannot retire pinned glyphs"
    );
    let pinned = owners[0].clone();
    let recent = Arc::downgrade(&owners[1]);
    let oldest = Arc::downgrade(&owners[2]);
    drop(owners);
    cache.touch(1);
    let incoming = Arc::new(1056);
    assert_eq!(
        cache.insert(1056, incoming.clone()),
        (2..34).collect::<Vec<_>>()
    );
    assert_eq!(cache.entries.len(), 1025);
    assert!(recent.upgrade().is_some());
    assert!(oldest.upgrade().is_none());
    assert_eq!(*pinned, 0);
    assert!(Arc::ptr_eq(&incoming, &cache.entries[&1056].value));
}

#[test]
fn age_compaction_preserves_recent_order_and_consecutive_hits_do_not_advance_age() {
    let mut cache = Residency::default();
    for key in 0..10 {
        let value = Arc::new(key);
        cache.insert(key, value);
    }
    cache.touch(2);
    let last = cache.age;
    cache.touch(2);
    assert_eq!(cache.age, last);
    cache.age = 65535;
    cache.insert(10, Arc::new(10));
    assert_eq!(cache.age, 11);
    assert_eq!(cache.entries[&2].age, 10);
    assert_eq!(cache.entries[&10].age, 11);
    assert_eq!(cache.entries[&0].age, 1);
}
