pub fn clamp_page(page: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(page.min(len - 1))
}

pub fn clamp(page: usize, len: usize, spread: bool) -> Option<usize> {
    let page = clamp_page(page, len)?;
    Some(if spread { page - page % 2 } else { page })
}

pub fn step(current: usize, len: usize, spread: bool, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let step = if spread { 2 } else { 1 };
    let target = if delta > 0 {
        current.saturating_add(step)
    } else {
        current.saturating_sub(step)
    };

    if spread && target >= len {
        let parity = current % 2;
        let last = len - 1;
        let last = if last % 2 == parity {
            last
        } else {
            last.saturating_sub(1)
        };
        return clamp_page(last, len);
    }

    clamp_page(target, len)
}

pub fn step_single(current: usize, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let target = if delta > 0 {
        current.saturating_add(1)
    } else {
        current.saturating_sub(1)
    };
    clamp_page(target, len)
}
