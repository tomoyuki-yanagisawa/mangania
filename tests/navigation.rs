use mangania::navigation::{clamp, step};

#[test]
fn clamps_and_aligns_pages() {
    assert_eq!(clamp(0, 5, false), Some(0));
    assert_eq!(clamp(usize::MAX, 5, false), Some(4));
    assert_eq!(clamp(4, 5, true), Some(4));
    assert_eq!(clamp(3, 4, true), Some(2));
    assert_eq!(clamp(0, 0, false), None);
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
