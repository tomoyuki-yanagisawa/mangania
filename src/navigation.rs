pub fn clamp(page: usize, len: usize, spread: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let page = page.min(len - 1);
    Some(if spread { page - page % 2 } else { page })
}

pub fn step(current: usize, len: usize, spread: bool, delta: isize) -> Option<usize> {
    let step = if spread { 2 } else { 1 };
    let target = if delta > 0 {
        current.saturating_add(step)
    } else {
        current.saturating_sub(step)
    };
    clamp(target, len, spread)
}
