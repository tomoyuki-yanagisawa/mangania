use mangania::navigation::{clamp, clamp_page, step, step_single};

#[test]
fn clamps_and_aligns_pages() {
    assert_eq!(clamp(0, 5, false), Some(0));
    assert_eq!(clamp(usize::MAX, 5, false), Some(4));
    assert_eq!(clamp(4, 5, true), Some(4));
    assert_eq!(clamp(3, 4, true), Some(2));
    assert_eq!(clamp(0, 0, false), None);

    assert_eq!(clamp_page(usize::MAX, 5), Some(4));
    assert_eq!(clamp_page(0, 0), None);
}

#[test]
fn steps_within_single_and_spread_modes() {
    assert_eq!(step(0, 5, false, 1), Some(1));
    assert_eq!(step(0, 5, true, 1), Some(2));
    assert_eq!(step(4, 5, false, 1), Some(4));
    assert_eq!(step(4, 5, true, 1), Some(4));
    assert_eq!(step(2, 5, true, -1), Some(0));
    assert_eq!(step(0, 5, true, -1), Some(0));
    assert_eq!(step(0, 0, false, 1), None);
}

#[test]
fn preserves_spread_parity_after_a_single_page_step() {
    assert_eq!(step(1, 6, true, 1), Some(3));
    assert_eq!(step(3, 6, true, 1), Some(5));
    assert_eq!(step(5, 6, true, 1), Some(5));
    assert_eq!(step(4, 6, true, 1), Some(4));
    assert_eq!(step(3, 5, true, -1), Some(1));
}

#[test]
fn steps_one_page_in_both_modes() {
    assert_eq!(step_single(0, 5, 1), Some(1));
    assert_eq!(step_single(4, 5, 1), Some(4));
    assert_eq!(step_single(0, 5, -1), Some(0));
    assert_eq!(step_single(0, 0, 1), None);
}
