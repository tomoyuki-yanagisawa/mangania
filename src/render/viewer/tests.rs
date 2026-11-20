use std::path::PathBuf;
use std::sync::Arc;

use image::RgbaImage;

use super::{CacheLimits, Frame, Viewer, frame_for, layout_for, prefetch_targets, step_mixed};
use crate::archive::Page;
use crate::config::{DisplayMode, ScaleFilter};
use crate::render::image::prepare;
use crate::render::prefetch::Prefetch;
use crate::render::viewport::{Orientation, Viewport};

fn test_viewer(mode: DisplayMode, pages: usize) -> Viewer {
    test_viewer_scaled(mode, pages, true)
}

fn test_viewer_scaled(mode: DisplayMode, pages: usize, allow_upscale: bool) -> Viewer {
    let pages = (0..pages)
        .map(|entry| Page {
            name: format!("{entry}.png"),
            entry,
        })
        .collect();
    Viewer::new(
        PathBuf::from("test.zip"),
        pages,
        Prefetch::idle(),
        1,
        CacheLimits {
            entries: 8,
            bytes: usize::MAX,
        },
        Viewport::new(1200, 900, ScaleFilter::Triangle, mode, allow_upscale),
    )
}

fn portrait() -> Arc<RgbaImage> {
    Arc::new(RgbaImage::new(600, 900))
}

fn insert(viewer: &mut Viewer, page: usize, image: Arc<RgbaImage>) {
    let viewport = viewer.viewport;
    let prepared = prepare((*image).clone(), viewport).expect("prepare page");
    viewer.insert_prepared(page, viewport, prepared);
}

fn fail(viewer: &mut Viewer, page: usize, message: &str) {
    let viewport = viewer.viewport;
    viewer.fail_page(page, viewport, message.to_string());
}

fn frame_for_mode(
    mode: DisplayMode,
    has_next: bool,
    current: Option<Orientation>,
    next: Option<Orientation>,
    right: Arc<RgbaImage>,
    left: Option<Arc<RgbaImage>>,
    next_failed: bool,
) -> Frame {
    frame_for(
        layout_for(mode, has_next, current, next),
        right,
        left,
        !has_next || next.is_some(),
        next_failed,
    )
}

#[test]
fn request_display_orders_visible_then_prefetch() {
    let mut single = test_viewer(DisplayMode::Single, 10);
    single.request_display();
    assert_eq!(single.prefetch.queued_pages(), vec![0, 1]);

    single.go_to(2);
    single.request_display();
    assert_eq!(single.prefetch.queued_pages(), vec![2, 3]);

    let mut spread = test_viewer(DisplayMode::Spread, 10);
    spread.request_display();
    assert_eq!(spread.prefetch.queued_pages(), vec![0, 1, 2, 3]);
}

#[test]
fn rejected_visible_page_is_requested_again() {
    let mut viewer = test_viewer(DisplayMode::Single, 4);
    viewer.states[0].rejected = true;

    viewer.request_display();

    assert_eq!(viewer.prefetch.queued_pages(), vec![0, 1]);
}

#[test]
fn failed_page_draws_a_blank_frame() {
    let mut viewer = test_viewer(DisplayMode::Single, 4);
    assert!(matches!(viewer.frame(), Frame::Loading));

    fail(&mut viewer, 0, "failed");
    assert!(matches!(viewer.frame(), Frame::Failed));
    assert!(!viewer.is_loading());
}

#[test]
fn frame_draws_a_single_page_in_single_mode() {
    let right = portrait();

    match frame_for_mode(
        DisplayMode::Single,
        true,
        Some(Orientation::Portrait),
        Some(Orientation::Portrait),
        right.clone(),
        None,
        false,
    ) {
        Frame::Single(image) => assert!(Arc::ptr_eq(&image, &right)),
        _ => panic!("expected a single page"),
    }
}

#[test]
fn frame_waits_until_both_spread_pages_are_ready() {
    let right = portrait();
    let left = portrait();
    let portrait = Some(Orientation::Portrait);

    assert!(matches!(
        frame_for_mode(
            DisplayMode::Spread,
            true,
            portrait,
            portrait,
            right.clone(),
            None,
            false
        ),
        Frame::Loading
    ));
    match frame_for_mode(
        DisplayMode::Spread,
        true,
        portrait,
        portrait,
        right.clone(),
        Some(left.clone()),
        false,
    ) {
        Frame::Spread {
            left: drawn_left,
            right: drawn_right,
        } => {
            assert!(Arc::ptr_eq(&drawn_left, &left));
            assert!(Arc::ptr_eq(&drawn_right, &right));
        }
        _ => panic!("expected a spread"),
    }
    assert!(matches!(
        frame_for_mode(
            DisplayMode::Spread,
            true,
            portrait,
            portrait,
            right.clone(),
            None,
            true
        ),
        Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
    ));
}

#[test]
fn frame_uses_only_the_right_cell_on_the_last_page() {
    let right = portrait();

    assert!(matches!(
        frame_for_mode(
            DisplayMode::Spread,
            false,
            Some(Orientation::Portrait),
            None,
            right.clone(),
            None,
            false
        ),
        Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
    ));
}

#[test]
fn frame_shows_a_landscape_page_alone_in_mixed_mode() {
    let right = portrait();

    match frame_for_mode(
        DisplayMode::Mixed,
        true,
        Some(Orientation::Landscape),
        Some(Orientation::Portrait),
        right.clone(),
        None,
        false,
    ) {
        Frame::Single(image) => assert!(Arc::ptr_eq(&image, &right)),
        _ => panic!("expected a single page"),
    }
}

#[test]
fn frame_waits_for_a_mixed_portrait_pair() {
    let right = portrait();
    let left = portrait();
    let portrait = Some(Orientation::Portrait);

    assert!(matches!(
        frame_for_mode(
            DisplayMode::Mixed,
            true,
            portrait,
            portrait,
            right.clone(),
            None,
            false
        ),
        Frame::Loading
    ));
    match frame_for_mode(
        DisplayMode::Mixed,
        true,
        portrait,
        portrait,
        right.clone(),
        Some(left.clone()),
        false,
    ) {
        Frame::Spread {
            left: drawn_left,
            right: drawn_right,
        } => {
            assert!(Arc::ptr_eq(&drawn_left, &left));
            assert!(Arc::ptr_eq(&drawn_right, &right));
        }
        _ => panic!("expected a spread"),
    }
    assert!(matches!(
        frame_for_mode(
            DisplayMode::Mixed,
            true,
            portrait,
            portrait,
            right.clone(),
            None,
            true
        ),
        Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
    ));
}

#[test]
fn frame_waits_for_an_unknown_mixed_neighbour() {
    let right = portrait();
    let portrait = Some(Orientation::Portrait);

    assert!(matches!(
        frame_for_mode(
            DisplayMode::Mixed,
            true,
            portrait,
            None,
            right.clone(),
            None,
            false
        ),
        Frame::Loading
    ));
    assert!(matches!(
        frame_for_mode(
            DisplayMode::Mixed,
            true,
            portrait,
            Some(Orientation::Landscape),
            right.clone(),
            None,
            false
        ),
        Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
    ));
    assert!(matches!(
        frame_for_mode(
            DisplayMode::Mixed,
            false,
            portrait,
            None,
            right.clone(),
            None,
            false
        ),
        Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
    ));
    assert!(matches!(
        frame_for_mode(
            DisplayMode::Mixed,
            true,
            portrait,
            None,
            right.clone(),
            None,
            true
        ),
        Frame::RightOnly(image) if Arc::ptr_eq(&image, &right)
    ));
}

#[test]
fn loading_follows_the_display_state() {
    let mut viewer = test_viewer(DisplayMode::Spread, 4);
    assert!(viewer.is_loading());

    insert(&mut viewer, 0, portrait());
    assert!(viewer.is_loading());

    insert(&mut viewer, 1, portrait());
    assert!(!viewer.is_loading());
}

#[test]
fn rejected_left_page_still_waits_for_its_image() {
    let mut viewer = test_viewer(DisplayMode::Spread, 4);
    insert(&mut viewer, 0, portrait());
    viewer.states[1].rejected = true;

    assert!(matches!(viewer.frame(), Frame::Loading));

    fail(&mut viewer, 1, "failed");
    assert!(matches!(viewer.frame(), Frame::RightOnly(_)));
}

#[test]
fn failed_page_is_not_requested_again() {
    let mut viewer = test_viewer(DisplayMode::Spread, 4);
    fail(&mut viewer, 0, "cannot fit");

    assert!(viewer.error(0).is_some());
    assert!(!viewer.states[0].rejected);
    assert!(!viewer.needs_request(0));
}

#[test]
fn last_spread_page_uses_only_the_right_cell() {
    let mut viewer = test_viewer(DisplayMode::Spread, 1);
    insert(&mut viewer, 0, portrait());

    assert!(matches!(viewer.frame(), Frame::RightOnly(_)));
}

#[test]
fn prefetch_follows_pages_or_pairs() {
    assert_eq!(prefetch_targets(0, 10, 2, DisplayMode::Single), vec![1, 2]);
    assert_eq!(
        prefetch_targets(0, 10, 2, DisplayMode::Spread),
        vec![2, 3, 4, 5]
    );
    assert_eq!(prefetch_targets(5, 10, 1, DisplayMode::Mixed), vec![7, 8]);
}

#[test]
fn prefetch_stops_at_the_last_page() {
    assert_eq!(prefetch_targets(0, 3, 2, DisplayMode::Spread), vec![2]);
    assert_eq!(
        prefetch_targets(8, 10, 2, DisplayMode::Spread),
        Vec::<usize>::new()
    );
    assert_eq!(
        prefetch_targets(9, 10, 2, DisplayMode::Single),
        Vec::<usize>::new()
    );
}

#[test]
fn upscale_setting_controls_small_pages() {
    // 100x60 is smaller than the 1200x900 viewport.
    let mut enlarged = test_viewer_scaled(DisplayMode::Single, 1, true);
    insert(&mut enlarged, 0, Arc::new(RgbaImage::new(100, 60)));
    assert_eq!(
        enlarged.cache.get(0).unwrap().dimensions(),
        (1200, 720),
        "upscaling enabled must enlarge the page to the viewport"
    );

    let mut kept = test_viewer_scaled(DisplayMode::Single, 1, false);
    insert(&mut kept, 0, Arc::new(RgbaImage::new(100, 60)));
    assert_eq!(
        kept.cache.get(0).unwrap().dimensions(),
        (100, 60),
        "upscaling disabled must keep the page at its original size"
    );
}

#[test]
fn results_for_another_viewport_are_ignored() {
    let mut viewer = test_viewer(DisplayMode::Single, 2);
    let old = viewer.viewport;
    let prepared = prepare(RgbaImage::new(600, 900), old).expect("prepare page");

    viewer.set_viewport(Viewport::new(
        800,
        600,
        ScaleFilter::Triangle,
        DisplayMode::Single,
        true,
    ));

    assert!(!viewer.insert_prepared(0, old, prepared));
    assert!(!viewer.has_page(0));

    assert!(!viewer.fail_page(0, old, "failed".to_string()));
    assert!(viewer.error(0).is_none());
}

#[test]
fn transient_cache_failure_retries_once_then_errors() {
    let mut viewer = test_viewer(DisplayMode::Single, 2);
    let viewport = viewer.viewport;

    assert!(viewer.retry_page(0, viewport, "cache read failed".to_string()));
    assert!(viewer.error(0).is_none());
    assert!(viewer.states[0].rejected);

    assert!(viewer.retry_page(0, viewport, "cache read failed".to_string()));
    assert!(viewer.error(0).is_some());
}

#[test]
fn displayed_partner_follows_the_shown_spread() {
    let mut spread = test_viewer(DisplayMode::Spread, 2);
    spread.states[0].orientation = Some(Orientation::Portrait);
    spread.states[1].orientation = Some(Orientation::Portrait);
    assert_eq!(spread.displayed_partner(), Some(1));
    assert_eq!(spread.page_range(), (1, 2));

    assert_eq!(
        test_viewer(DisplayMode::Single, 2).displayed_partner(),
        None
    );
    assert_eq!(
        test_viewer(DisplayMode::Spread, 1).displayed_partner(),
        None
    );

    let mut mixed = test_viewer(DisplayMode::Mixed, 2);
    mixed.states[0].orientation = Some(Orientation::Portrait);
    mixed.states[1].orientation = Some(Orientation::Landscape);
    assert_eq!(mixed.displayed_partner(), None);

    mixed.states[1].orientation = Some(Orientation::Portrait);
    assert_eq!(mixed.displayed_partner(), Some(1));
}

#[test]
fn mixed_steps_skip_paired_portrait_pages() {
    // Portrait: 0, 1, 3, 4. Landscape: 2, 5.
    let is_portrait = |page: usize| matches!(page, 0 | 1 | 3 | 4);

    assert_eq!(step_mixed(0, 6, 1, is_portrait), Some(2));
    assert_eq!(step_mixed(2, 6, 1, is_portrait), Some(3));
    assert_eq!(step_mixed(3, 6, 1, is_portrait), Some(5));
    assert_eq!(step_mixed(4, 6, 1, is_portrait), Some(5));

    assert_eq!(step_mixed(2, 6, -1, is_portrait), Some(0));
    assert_eq!(step_mixed(3, 6, -1, is_portrait), Some(2));
    assert_eq!(step_mixed(4, 6, -1, is_portrait), Some(3));
    assert_eq!(step_mixed(5, 6, -1, is_portrait), Some(3));
    assert_eq!(step_mixed(0, 6, -1, is_portrait), Some(0));
}
