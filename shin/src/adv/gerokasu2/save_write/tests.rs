use super::*;

#[test]
fn save_write_waits_for_question_poll_and_final_callback() {
    for flags in [0, 1, 2, 3, 0xffff_fffe, 0xffff_ffff] {
        let mut owner = Controller::new(flags);
        assert_eq!(owner.flags(), (flags & 1) as u8);
        assert!(!owner.poll_write());
        owner.io_finished();
        assert!(owner.take_events().is_empty());
        owner.update();
        assert_eq!(owner.take_events(), [Event::ShowWait]);
        for _ in 0..20 {
            owner.update();
        }
        assert!(owner.take_events().is_empty());
        assert!(!owner.poll_write());
        assert_eq!(owner.take_events(), [Event::RequestWrite]);
        assert!(!owner.poll_write());
        owner.io_finished();
        assert!(owner.poll_write());
        owner.question_response(false);
        owner.update();
        assert!(owner.take_events().is_empty());
        owner.question_response(true);
        assert!(!owner.is_done());
        assert!(owner.take_events().is_empty());
        owner.update();
        assert_eq!(owner.take_events(), [Event::Completed { success: true }]);
        assert!(owner.is_done());
        owner.question_response(true);
        owner.io_finished();
        owner.update();
        assert!(owner.take_events().is_empty());
    }
}

#[test]
fn save_write_desktop_failure_closes_only_after_rollback_and_question() {
    let mut owner = Controller::new(0);
    owner.update();
    owner.take_events();
    owner.poll_write();
    owner.take_events();
    owner.desktop_io_failed();
    assert!(owner.poll_write());
    owner.update();
    assert!(owner.take_events().is_empty());
    owner.question_response(true);
    owner.update();
    assert_eq!(owner.take_events(), [Event::Completed { success: false }]);
}
