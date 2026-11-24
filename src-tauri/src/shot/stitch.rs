//! 垂直长截图拼接。整宽中带做 SAD，横条各自投票，和多数不一致的条丢掉。
//! 拼上去时只追加新行，接缝直接切开，避免两帧文字叠出重影。已知滚动距离时只在期望值附近搜索。

pub(in crate::shot) const MAX_STITCH_HEIGHT: u32 = 20_000;
pub(in crate::shot) const MAX_STITCH_FRAMES: usize = 40;
pub(in crate::shot) const BORDER: i32 = 4;
pub(in crate::shot) const BAR_W: i32 = 520;
pub(in crate::shot) const BAR_H: i32 = 44;

const HEADER_PERCENT: u32 = 10;
const FOOTER_PERCENT: u32 = 5;
const MIN_OVERLAP_PERCENT: u32 = 20;
const ACCEPT_SAD: u32 = 16;
const STILL_SAD: u32 = 6;
const AMBIGUOUS_GAP: u32 = 5;
const SEAM: u32 = 16;
const OUTLIER_PX: u32 = 6;
const MAX_STRIPS: usize = 5;

#[derive(Clone, Copy)]
pub(in crate::shot) struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub(in crate::shot) struct LiveLayout {
    pub capture: ScreenRect,
    pub border: ScreenRect,
    pub bar: ScreenRect,
}

#[derive(Clone)]
pub(in crate::shot) struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub(in crate::shot) struct Composed {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

struct Stored {
    frame: Frame,
    scroll: u32,
    blend: bool,
}

pub(in crate::shot) struct Stitcher {
    frames: Vec<Stored>,
    image: Composed,
}

pub(in crate::shot) enum Push {
    Added {
        frames: usize,
        height: u32,
    },
    /// 滚了，但还不到一段，先不写进长图。
    Short,
    Still,
    Weak,
    Limit,
}

enum ScrollMatch {
    Moved { scroll: u32, sad: u32 },
    Still,
    Weak,
}

pub(in crate::shot) fn live_layout(
    selection: ScreenRect,
    monitor: ScreenRect,
) -> Option<LiveLayout> {
    live_layout_bar(selection, monitor, BAR_W, BAR_H)
}

pub(in crate::shot) fn live_layout_bar(
    selection: ScreenRect,
    monitor: ScreenRect,
    bar_w: i32,
    bar_h: i32,
) -> Option<LiveLayout> {
    let selection = clamp_rect(selection, monitor)?;
    let mut capture = hole_from_selection(selection, monitor)?;
    let bar = place_bar(capture, monitor, bar_w, bar_h);
    if intersects(bar, capture) {
        capture = shrink_for_bar(capture, bar)?;
    }
    if intersects(bar, capture) {
        return None;
    }
    let border = border_window(capture, monitor);
    Some(LiveLayout {
        capture,
        border,
        bar,
    })
}

pub(in crate::shot) fn sample_sad(
    width: u32,
    height: u32,
    left: &[u8],
    right: &[u8],
) -> Option<u32> {
    if !same_buffer(width, height, left) || !same_buffer(width, height, right) {
        return None;
    }
    let y0 = height * 20 / 100;
    let y1 = (height * 80 / 100).max(y0.saturating_add(1));
    let left = gray_plane(width, left);
    let right = gray_plane(width, right);
    band_mean(&left, &right, width, y0, y1, 0, 1, 2, u32::MAX)
}

impl Stitcher {
    pub(in crate::shot) fn start(frame: Frame) -> Option<Self> {
        if !usable(&frame) || frame.height > MAX_STITCH_HEIGHT {
            return None;
        }
        let image = Composed {
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels.clone(),
        };
        Some(Self {
            frames: vec![Stored {
                frame,
                scroll: 0,
                blend: false,
            }],
            image,
        })
    }

    pub(in crate::shot) fn push(&mut self, frame: Frame) -> Push {
        self.accept(frame, 1, u32::MAX, 1)
    }

    /// 接收大约 8%–80% 屏高。更短先留着，更长当成没有重叠。
    pub(in crate::shot) fn push_step(&mut self, frame: Frame) -> Push {
        let height = self
            .frames
            .last()
            .map(|item| item.frame.height)
            .unwrap_or(0);
        let min_scroll = (height * 8 / 100).max(16);
        let max_scroll = (height * 80 / 100).max(min_scroll);
        self.accept(frame, 8, max_scroll, min_scroll)
    }

    /// 已知大约滚了 `expected` 像素时，只在 ±slack 里找，对不上就不动已拼的图。
    pub(in crate::shot) fn push_near(&mut self, frame: Frame, expected: u32, slack: u32) -> Push {
        if expected == 0 {
            return Push::Weak;
        }
        let lo = expected.saturating_sub(slack).max(1);
        let hi = expected.saturating_add(slack);
        self.accept(frame, lo, hi, 1)
    }

    pub(in crate::shot) fn last_scroll(&self) -> u32 {
        self.frames.last().map(|item| item.scroll).unwrap_or(0)
    }

    fn accept(&mut self, frame: Frame, lo: u32, hi: u32, min_scroll: u32) -> Push {
        if self.frames.len() >= MAX_STITCH_FRAMES {
            return Push::Limit;
        }
        let previous = &self.frames.last().expect("拼接至少有一帧").frame;
        if frame.width != previous.width || frame.height != previous.height || !usable(&frame) {
            return Push::Weak;
        }
        let (scroll, _sad) = match match_scroll(previous, &frame, lo, hi) {
            ScrollMatch::Moved { scroll, sad } => (scroll, sad),
            ScrollMatch::Still => return Push::Still,
            ScrollMatch::Weak => return Push::Weak,
        };
        if scroll < min_scroll {
            return Push::Short;
        }
        if self.image.height.saturating_add(scroll) > MAX_STITCH_HEIGHT {
            return Push::Limit;
        }
        self.frames.push(Stored {
            frame,
            scroll,
            blend: false,
        });
        self.image = compose(&self.frames);
        Push::Added {
            frames: self.frames.len(),
            height: self.image.height,
        }
    }

    pub(in crate::shot) fn undo(&mut self) -> bool {
        if self.frames.len() <= 1 {
            return false;
        }
        self.frames.pop();
        self.image = compose(&self.frames);
        true
    }

    pub(in crate::shot) fn frames(&self) -> usize {
        self.frames.len()
    }

    pub(in crate::shot) fn image(&self) -> &Composed {
        &self.image
    }
}

pub(in crate::shot) fn crop_vertical(image: &Composed, top: u32, bottom: u32) -> Option<Composed> {
    if top.saturating_add(bottom) >= image.height {
        return None;
    }
    let height = image.height - top - bottom;
    if height < 32 {
        return None;
    }
    let stride = image.width as usize * 4;
    let start = top as usize * stride;
    let end = start + height as usize * stride;
    if end > image.pixels.len() {
        return None;
    }
    Some(Composed {
        width: image.width,
        height,
        pixels: image.pixels[start..end].to_vec(),
    })
}

fn usable(frame: &Frame) -> bool {
    frame.width >= 32 && frame.height >= 32 && same_buffer(frame.width, frame.height, &frame.pixels)
}

fn same_buffer(width: u32, height: u32, pixels: &[u8]) -> bool {
    let Some(bytes) = (width as usize)
        .checked_mul(height as usize)
        .and_then(|count| count.checked_mul(4))
    else {
        return false;
    };
    pixels.len() == bytes
}

#[derive(Clone, Copy)]
struct Vote {
    scroll: u32,
    y0: u32,
    y1: u32,
}

fn match_scroll(previous: &Frame, next: &Frame, lo: u32, hi: u32) -> ScrollMatch {
    let width = previous.width;
    let height = previous.height;
    let prev = gray_plane(width, &previous.pixels);
    let next_g = gray_plane(width, &next.pixels);
    let strips = voting_strips(height);
    if strips.is_empty() {
        return ScrollMatch::Weak;
    }
    let still = rows_mean(&prev, &next_g, width, &strips, 0, u32::MAX).unwrap_or(u32::MAX);
    if still <= STILL_SAD {
        return ScrollMatch::Still;
    }
    let max_scroll = height - height * MIN_OVERLAP_PERCENT / 100;
    let hi = hi.min(max_scroll);
    if lo > hi {
        return ScrollMatch::Weak;
    }
    let mut votes = Vec::new();
    for (y0, y1) in strips {
        let reach = strip_reach(height, y1);
        let strip_hi = hi.min(reach);
        if lo > strip_hi {
            continue;
        }
        if let Some(vote) = vote_strip(&prev, &next_g, width, y0, y1, lo, strip_hi) {
            votes.push(vote);
        }
    }
    if votes.is_empty() {
        return ScrollMatch::Weak;
    }
    votes.sort_by_key(|vote| vote.scroll);
    let median = votes[votes.len() / 2].scroll;
    let need = (votes.len() + 1) / 2;
    let inliers: Vec<Vote> = votes
        .into_iter()
        .filter(|vote| vote.scroll.abs_diff(median) <= OUTLIER_PX)
        .collect();
    if inliers.len() < need {
        return ScrollMatch::Weak;
    }
    let scroll = inliers[inliers.len() / 2].scroll;
    let rows: Vec<(u32, u32)> = inliers
        .into_iter()
        .filter(|vote| vote.scroll.abs_diff(scroll) <= 2 && strip_reach(height, vote.y1) >= scroll)
        .map(|vote| (vote.y0, vote.y1))
        .collect();
    let Some(confirm) = rows_mean(&prev, &next_g, width, &rows, scroll, u32::MAX) else {
        return ScrollMatch::Weak;
    };
    if confirm > ACCEPT_SAD || rows.iter().map(|(y0, y1)| y1 - y0).sum::<u32>() < 12 {
        return ScrollMatch::Weak;
    }
    let mut rival = u32::MAX;
    for shifted in [scroll.saturating_sub(12), scroll.saturating_add(12)] {
        if shifted == 0 || shifted == scroll || shifted > max_scroll {
            continue;
        }
        if let Some(sad) = rows_mean(&prev, &next_g, width, &rows, shifted, u32::MAX) {
            rival = rival.min(sad);
        }
    }
    if confirm.saturating_add(AMBIGUOUS_GAP) >= rival {
        return ScrollMatch::Weak;
    }
    ScrollMatch::Moved {
        scroll,
        sad: confirm,
    }
}

fn vote_strip(
    previous: &[u8],
    next: &[u8],
    width: u32,
    y0: u32,
    y1: u32,
    lo: u32,
    hi: u32,
) -> Option<Vote> {
    let (scroll, sad, rival) = search_strip(previous, next, width, y0, y1, lo, hi)?;
    if sad > ACCEPT_SAD || sad.saturating_add(AMBIGUOUS_GAP) >= rival {
        return None;
    }
    // 最优落在搜索边上，说明真值还在窗外，这票不能算。
    if sad > 2 && (scroll == hi || (scroll == lo && lo > 1)) {
        return None;
    }
    Some(Vote { scroll, y0, y1 })
}

fn search_strip(
    previous: &[u8],
    next: &[u8],
    width: u32,
    y0: u32,
    y1: u32,
    lo: u32,
    hi: u32,
) -> Option<(u32, u32, u32)> {
    let step = if hi.saturating_sub(lo) < 16 { 1 } else { 4 };
    let mut best_scroll = 0u32;
    let mut best = u32::MAX;
    let mut rival_scroll = 0u32;
    let mut rival = u32::MAX;
    let mut scroll = lo;
    while scroll <= hi {
        let abort = if best == u32::MAX {
            64
        } else {
            best.saturating_add(24)
        };
        if let Some(sad) = band_mean(previous, next, width, y0, y1, scroll, 2, 2, abort) {
            note_candidate(
                &mut best,
                &mut best_scroll,
                &mut rival,
                &mut rival_scroll,
                scroll,
                sad,
            );
        }
        scroll += step;
    }
    if best == u32::MAX {
        return None;
    }
    let fine_lo = best_scroll.saturating_sub(step).max(lo);
    let fine_hi = best_scroll.saturating_add(step).min(hi);
    let mut scroll = fine_lo;
    while scroll <= fine_hi {
        if let Some(sad) = band_mean(previous, next, width, y0, y1, scroll, 1, 2, u32::MAX) {
            note_candidate(
                &mut best,
                &mut best_scroll,
                &mut rival,
                &mut rival_scroll,
                scroll,
                sad,
            );
        }
        scroll += 1;
    }
    let sad = band_mean(previous, next, width, y0, y1, best_scroll, 1, 2, u32::MAX)?;
    let rival = if rival_scroll == 0 {
        u32::MAX
    } else {
        band_mean(previous, next, width, y0, y1, rival_scroll, 1, 2, u32::MAX).unwrap_or(u32::MAX)
    };
    Some((best_scroll, sad, rival))
}

fn note_candidate(
    best: &mut u32,
    best_scroll: &mut u32,
    rival: &mut u32,
    rival_scroll: &mut u32,
    scroll: u32,
    sad: u32,
) {
    if sad < *best {
        if scroll.abs_diff(*best_scroll) > OUTLIER_PX && *best < u32::MAX && *best < *rival {
            *rival = *best;
            *rival_scroll = *best_scroll;
        }
        *best = sad;
        *best_scroll = scroll;
    } else if scroll.abs_diff(*best_scroll) > OUTLIER_PX && sad < *rival {
        *rival = sad;
        *rival_scroll = scroll;
    }
}

fn rows_mean(
    previous: &[u8],
    next: &[u8],
    width: u32,
    rows: &[(u32, u32)],
    scroll: u32,
    abort_at: u32,
) -> Option<u32> {
    let mut sum = 0u64;
    let mut count = 0u64;
    for (y0, y1) in rows {
        let Some((part, part_count)) =
            band_sum(previous, next, width, *y0, *y1, scroll, 1, 2, abort_at)
        else {
            continue;
        };
        sum += part;
        count += part_count;
        if count >= 64 && sum / count > u64::from(abort_at) {
            return None;
        }
    }
    if count == 0 {
        None
    } else {
        Some((sum / count) as u32)
    }
}

fn band_mean(
    previous: &[u8],
    next: &[u8],
    width: u32,
    y0: u32,
    y1: u32,
    scroll: u32,
    y_step: u32,
    x_step: u32,
    abort_at: u32,
) -> Option<u32> {
    let (sum, count) = band_sum(
        previous, next, width, y0, y1, scroll, y_step, x_step, abort_at,
    )?;
    if count == 0 {
        None
    } else {
        Some((sum / count) as u32)
    }
}

fn band_sum(
    previous: &[u8],
    next: &[u8],
    width: u32,
    y0: u32,
    y1: u32,
    scroll: u32,
    y_step: u32,
    x_step: u32,
    abort_at: u32,
) -> Option<(u64, u64)> {
    if y1 <= y0 || y_step == 0 || x_step == 0 {
        return None;
    }
    let width_us = width as usize;
    let step = x_step as usize;
    let mut sum = 0u64;
    let mut count = 0u64;
    let mut y = y0;
    while y < y1 {
        let prev_row = (y as usize + scroll as usize) * width_us;
        let next_row = y as usize * width_us;
        if prev_row + width_us > previous.len() || next_row + width_us > next.len() {
            return None;
        }
        let mut x = 0usize;
        while x < width_us {
            sum += u64::from(previous[prev_row + x].abs_diff(next[next_row + x]));
            count += 1;
            x += step;
        }
        if count >= 64 && sum / count > u64::from(abort_at) {
            return None;
        }
        y += y_step;
    }
    Some((sum, count))
}

fn gray_plane(width: u32, pixels: &[u8]) -> Vec<u8> {
    let mut plane = vec![0u8; width as usize * (pixels.len() / (width as usize * 4))];
    for (index, pixel) in pixels.chunks_exact(4).enumerate() {
        plane[index] =
            ((u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])) / 3) as u8;
    }
    plane
}

fn voting_strips(height: u32) -> Vec<(u32, u32)> {
    let (header, footer) = bands(height);
    let strip_h = (height / 8).clamp(12, 48);
    let floor = (height * 15 / 100).max(8);
    let mut strips = Vec::new();
    let mut y = header;
    while strips.len() < MAX_STRIPS {
        let y1 = y.saturating_add(strip_h);
        if y1.saturating_add(footer) >= height {
            break;
        }
        if strip_reach(height, y1) < floor {
            break;
        }
        strips.push((y, y1));
        y = y1;
    }
    if strips.is_empty() {
        let y1 =
            (header.saturating_add(strip_h)).min(height.saturating_sub(footer.saturating_add(8)));
        if y1 > header.saturating_add(8) {
            strips.push((header, y1));
        }
    }
    strips
}

fn strip_reach(height: u32, y1: u32) -> u32 {
    let (_, footer) = bands(height);
    height.saturating_sub(footer.saturating_add(y1))
}

fn bands(height: u32) -> (u32, u32) {
    if height < 40 {
        return (0, 0);
    }
    let header = height * HEADER_PERCENT / 100;
    let footer = height * FOOTER_PERCENT / 100;
    if header + footer + 16 >= height {
        return (0, 0);
    }
    (header, footer)
}

fn compose(frames: &[Stored]) -> Composed {
    let width = frames[0].frame.width;
    let (_, footer) = bands(frames[0].frame.height);
    let mut pixels = frames[0].frame.pixels.clone();
    let mut height = frames[0].frame.height;
    for item in frames.iter().skip(1) {
        let frame = &item.frame;
        let scroll = item.scroll;
        let frame_h = frame.height;
        if footer > 0 && height > footer {
            height -= footer;
            pixels.truncate(height as usize * width as usize * 4);
        }
        let body_end = frame_h - footer;
        let body_start = body_end - scroll;
        let seam = if item.blend {
            SEAM.min(body_start).min(height)
        } else {
            0
        };
        if seam > 0 {
            blend_rows(
                &mut pixels,
                &frame.pixels,
                width,
                height - seam,
                body_start - seam,
                seam,
            );
        }
        append_rows(&mut pixels, &frame.pixels, width, body_start, body_end);
        height += scroll;
        if footer > 0 {
            append_rows(&mut pixels, &frame.pixels, width, frame_h - footer, frame_h);
            height += footer;
        }
    }
    Composed {
        width,
        height,
        pixels,
    }
}

fn append_rows(dst: &mut Vec<u8>, src: &[u8], width: u32, y0: u32, y1: u32) {
    let stride = width as usize * 4;
    let start = y0 as usize * stride;
    let end = y1 as usize * stride;
    dst.extend_from_slice(&src[start..end]);
}

fn blend_rows(dst: &mut [u8], src: &[u8], width: u32, dst_y: u32, src_y: u32, rows: u32) {
    let stride = width as usize * 4;
    for i in 0..rows {
        let weight_new = (i + 1) * 255 / (rows + 1);
        let weight_old = 255 - weight_new;
        let d = (dst_y + i) as usize * stride;
        let s = (src_y + i) as usize * stride;
        for channel in 0..stride {
            if channel % 4 == 3 {
                dst[d + channel] = 255;
                continue;
            }
            let old = u32::from(dst[d + channel]);
            let new = u32::from(src[s + channel]);
            dst[d + channel] = ((old * weight_old + new * weight_new) / 255) as u8;
        }
    }
}

fn clamp_rect(rect: ScreenRect, monitor: ScreenRect) -> Option<ScreenRect> {
    let x0 = rect.x.max(monitor.x);
    let y0 = rect.y.max(monitor.y);
    let x1 = rect_right(rect).min(rect_right(monitor));
    let y1 = rect_bottom(rect).min(rect_bottom(monitor));
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width < 32 || height < 32 {
        return None;
    }
    Some(ScreenRect {
        x: x0,
        y: y0,
        width: width as u32,
        height: height as u32,
    })
}

fn hole_from_selection(selection: ScreenRect, monitor: ScreenRect) -> Option<ScreenRect> {
    let left = if selection.x - BORDER < monitor.x {
        BORDER
    } else {
        0
    };
    let top = if selection.y - BORDER < monitor.y {
        BORDER
    } else {
        0
    };
    let right = if rect_right(selection) + BORDER > rect_right(monitor) {
        BORDER
    } else {
        0
    };
    let bottom = if rect_bottom(selection) + BORDER > rect_bottom(monitor) {
        BORDER
    } else {
        0
    };
    let width = selection.width as i32 - left - right;
    let height = selection.height as i32 - top - bottom;
    if width < 32 || height < 32 {
        return None;
    }
    Some(ScreenRect {
        x: selection.x + left,
        y: selection.y + top,
        width: width as u32,
        height: height as u32,
    })
}

fn place_bar(hole: ScreenRect, monitor: ScreenRect, bar_w: i32, bar_h: i32) -> ScreenRect {
    let gap = 8;
    let max_x = rect_right(monitor) - bar_w;
    let x = hole.x.min(max_x).max(monitor.x);
    let below = sized_bar(x, rect_bottom(hole) + gap, bar_w, bar_h);
    if fits(monitor, below) && !intersects(below, hole) {
        return below;
    }
    let above = sized_bar(x, hole.y - gap - bar_h, bar_w, bar_h);
    if fits(monitor, above) && !intersects(above, hole) {
        return above;
    }
    let y = hole.y.min(rect_bottom(monitor) - bar_h).max(monitor.y);
    let right = sized_bar(rect_right(hole) + gap, y, bar_w, bar_h);
    if fits(monitor, right) && !intersects(right, hole) {
        return right;
    }
    let left = sized_bar(hole.x - gap - bar_w, y, bar_w, bar_h);
    if fits(monitor, left) && !intersects(left, hole) {
        return left;
    }
    sized_bar(x, (rect_bottom(monitor) - bar_h).max(monitor.y), bar_w, bar_h)
}

fn shrink_for_bar(hole: ScreenRect, bar: ScreenRect) -> Option<ScreenRect> {
    if bar.y >= hole.y {
        let height = bar.y.saturating_sub(hole.y);
        if height < 32 {
            return None;
        }
        return Some(ScreenRect {
            height: height as u32,
            ..hole
        });
    }
    let top = rect_bottom(bar);
    let height = rect_bottom(hole).saturating_sub(top);
    if height < 32 {
        return None;
    }
    Some(ScreenRect {
        y: top,
        height: height as u32,
        ..hole
    })
}

fn border_window(hole: ScreenRect, monitor: ScreenRect) -> ScreenRect {
    let x = (hole.x - BORDER).max(monitor.x);
    let y = (hole.y - BORDER).max(monitor.y);
    let right = (rect_right(hole) + BORDER).min(rect_right(monitor));
    let bottom = (rect_bottom(hole) + BORDER).min(rect_bottom(monitor));
    ScreenRect {
        x,
        y,
        width: right.saturating_sub(x) as u32,
        height: bottom.saturating_sub(y) as u32,
    }
}

fn sized_bar(x: i32, y: i32, bar_w: i32, bar_h: i32) -> ScreenRect {
    ScreenRect {
        x,
        y,
        width: bar_w.max(1) as u32,
        height: bar_h.max(1) as u32,
    }
}

fn fits(monitor: ScreenRect, rect: ScreenRect) -> bool {
    rect.x >= monitor.x
        && rect.y >= monitor.y
        && rect_right(rect) <= rect_right(monitor)
        && rect_bottom(rect) <= rect_bottom(monitor)
}

fn intersects(left: ScreenRect, right: ScreenRect) -> bool {
    left.x < rect_right(right)
        && rect_right(left) > right.x
        && left.y < rect_bottom(right)
        && rect_bottom(left) > right.y
}

fn rect_right(rect: ScreenRect) -> i32 {
    rect.x.saturating_add(rect.width as i32)
}

fn rect_bottom(rect: ScreenRect) -> i32 {
    rect.y.saturating_add(rect.height as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor() -> ScreenRect {
        ScreenRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    fn page(width: u32, height: u32, content_at: u32) -> Frame {
        let (header, footer) = bands(height);
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        for y in 0..height {
            let marker = (if y < header {
                1u32
            } else if y >= height - footer {
                2
            } else {
                10 + content_at + (y - header)
            }) as u8;
            for x in 0..width {
                let index = (y as usize * width as usize + x as usize) * 4;
                pixels[index] = marker;
                pixels[index + 1] = marker;
                pixels[index + 2] = marker;
                pixels[index + 3] = 255;
            }
        }
        Frame {
            width,
            height,
            pixels,
        }
    }

    fn red_at(image: &Composed, y: u32) -> u8 {
        image.pixels[(y as usize * image.width as usize) * 4 + 2]
    }

    #[test]
    fn scroll_appends_new_rows_once() {
        let mut stitcher = Stitcher::start(page(48, 100, 0)).expect("首帧");
        let pushed = stitcher.push(page(48, 100, 30));
        match pushed {
            Push::Added { frames, height } => {
                assert_eq!(frames, 2);
                assert_eq!(height, 130);
            }
            Push::Still => panic!("被当成没有滚动"),
            Push::Short => panic!("不该停在半步"),
            Push::Weak => panic!("没有对上"),
            Push::Limit => panic!("不该到上限"),
        }
        let image = stitcher.image();
        assert_eq!(red_at(image, 0), 1);
        assert_eq!(red_at(image, 10), 10);
        assert_eq!(red_at(image, 94), 94);
        assert_eq!(red_at(image, 95), 95);
        assert_eq!(red_at(image, 124), 124);
        assert_eq!(red_at(image, 125), 2);
        assert!(stitcher.undo());
        assert_eq!(stitcher.frames(), 1);
        assert_eq!(stitcher.image().height, 100);
        assert!(!stitcher.undo());
    }

    #[test]
    fn identical_frame_is_still() {
        let mut stitcher = Stitcher::start(page(48, 100, 0)).expect("首帧");
        assert!(matches!(stitcher.push(page(48, 100, 0)), Push::Still));
        assert_eq!(stitcher.frames(), 1);
    }

    #[test]
    fn unrelated_frame_is_weak() {
        let mut stitcher = Stitcher::start(page(48, 80, 0)).expect("首帧");
        let mut other = page(48, 80, 0);
        for pixel in other.pixels.chunks_exact_mut(4) {
            pixel[0] = 220;
            pixel[1] = 220;
            pixel[2] = 220;
        }
        assert!(matches!(stitcher.push(other), Push::Weak));
    }

    #[test]
    fn frame_cap_stops_the_stitch() {
        let mut stitcher = Stitcher::start(page(48, 80, 0)).expect("首帧");
        for index in 1..MAX_STITCH_FRAMES {
            let pushed = stitcher.push(page(48, 80, (index as u32) * 40));
            assert!(matches!(pushed, Push::Added { .. }), "第 {index} 屏");
        }
        assert!(matches!(stitcher.push(page(48, 80, 800)), Push::Limit));
        assert_eq!(stitcher.frames(), MAX_STITCH_FRAMES);
    }

    #[test]
    fn bar_sits_under_a_free_selection() {
        let layout = live_layout(
            ScreenRect {
                x: 100,
                y: 100,
                width: 400,
                height: 300,
            },
            monitor(),
        )
        .expect("布局");
        assert_eq!(layout.capture.x, 100);
        assert_eq!(layout.capture.y, 100);
        assert_eq!(layout.capture.width, 400);
        assert_eq!(layout.bar.y, 408);
        assert!(!intersects(layout.bar, layout.capture));
    }

    #[test]
    fn edge_selection_keeps_the_border_out_of_the_capture() {
        let layout = live_layout(
            ScreenRect {
                x: 0,
                y: 0,
                width: 400,
                height: 300,
            },
            monitor(),
        )
        .expect("布局");
        assert_eq!(layout.capture.x, BORDER);
        assert_eq!(layout.capture.y, BORDER);
        assert!(!intersects(layout.bar, layout.capture));
    }

    #[test]
    fn header_change_still_joins_on_the_content() {
        let mut stitcher = Stitcher::start(page(48, 100, 0)).expect("首帧");
        let mut next = page(48, 100, 30);
        let (header, _) = bands(100);
        for y in 0..header {
            for x in 0..48 {
                let index = (y as usize * 48 + x as usize) * 4;
                next.pixels[index] = 255;
                next.pixels[index + 1] = 255;
                next.pixels[index + 2] = 255;
            }
        }
        match stitcher.push(next) {
            Push::Added { height, .. } => assert_eq!(height, 130),
            Push::Still => panic!("被当成没有滚动"),
            Push::Short => panic!("不该停在半步"),
            Push::Weak => panic!("吸顶栏把整屏带偏了"),
            Push::Limit => panic!("不该到上限"),
        }
    }

    #[test]
    fn noisy_band_is_outvoted() {
        let mut stitcher = Stitcher::start(page(64, 160, 0)).expect("首帧");
        let mut next = page(64, 160, 40);
        let strips = voting_strips(160);
        assert!(strips.len() >= 3);
        let (y0, y1) = strips[1];
        for y in y0..y1 {
            for x in 0..64 {
                let index = (y as usize * 64 + x as usize) * 4;
                next.pixels[index] = 240;
                next.pixels[index + 1] = 240;
                next.pixels[index + 2] = 240;
            }
        }
        match stitcher.push(next) {
            Push::Added { height, .. } => assert_eq!(height, 200),
            Push::Still => panic!("被当成没有滚动"),
            Push::Short => panic!("不该停在半步"),
            Push::Weak => panic!("一条噪声把整屏否掉了"),
            Push::Limit => panic!("不该到上限"),
        }
        assert_eq!(stitcher.last_scroll(), 40);
    }

    #[test]
    fn seam_keeps_a_hard_join() {
        let mut first = page(48, 100, 0);
        for y in 79..95 {
            for x in 0..48 {
                let index = (y as usize * 48 + x as usize) * 4;
                for channel in 0..3 {
                    first.pixels[index + channel] =
                        first.pixels[index + channel].saturating_add(40);
                }
            }
        }
        let mut stitcher = Stitcher::start(first).expect("首帧");
        match stitcher.push(page(48, 100, 30)) {
            Push::Added { height, .. } => assert_eq!(height, 130),
            Push::Still => panic!("被当成没有滚动"),
            Push::Short => panic!("不该停在半步"),
            Push::Weak => panic!("接缝附近的色差把整屏否掉了"),
            Push::Limit => panic!("不该到上限"),
        }
        let image = stitcher.image();
        assert_eq!(red_at(image, 94), 134, "接缝不该把两帧混在一起");
        assert_eq!(red_at(image, 95), 95);
    }

    #[test]
    fn expected_window_accepts_a_close_scroll_and_rejects_a_far_one() {
        let mut stitcher = Stitcher::start(page(48, 100, 0)).expect("首帧");
        assert!(matches!(
            stitcher.push_near(page(48, 100, 30), 8, 4),
            Push::Weak
        ));
        assert_eq!(stitcher.frames(), 1);
        match stitcher.push_near(page(48, 100, 30), 30, 8) {
            Push::Added { height, .. } => assert_eq!(height, 130),
            Push::Still => panic!("被当成没有滚动"),
            Push::Short => panic!("不该停在半步"),
            Push::Weak => panic!("期望距离附近没有对上"),
            Push::Limit => panic!("不该到上限"),
        }
        assert_eq!(stitcher.last_scroll(), 30);
    }

    #[test]
    fn step_keeps_a_modest_scroll_and_drops_a_long_jump() {
        let mut stitcher = Stitcher::start(page(64, 160, 0)).expect("首帧");
        assert!(matches!(stitcher.push_step(page(64, 160, 8)), Push::Short));
        assert_eq!(stitcher.frames(), 1);
        assert!(matches!(
            stitcher.push_step(page(64, 160, 140)),
            Push::Weak
        ));
        assert_eq!(stitcher.frames(), 1);
        match stitcher.push_step(page(64, 160, 40)) {
            Push::Added { height, .. } => assert_eq!(height, 200),
            Push::Short => panic!("这一段不该被留下"),
            Push::Still => panic!("被当成没有滚动"),
            Push::Weak => panic!("正常的一段没有接上"),
            Push::Limit => panic!("不该到上限"),
        }
        match stitcher.push_step(page(64, 160, 80)) {
            Push::Added { height, .. } => assert_eq!(height, 240),
            Push::Short => panic!("这一段不该被留下"),
            Push::Still => panic!("被当成没有滚动"),
            Push::Weak => panic!("下一段没有接上"),
            Push::Limit => panic!("不该到上限"),
        }
    }

    #[test]
    fn full_monitor_selection_still_leaves_the_bar_outside() {
        let layout = live_layout(
            ScreenRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            monitor(),
        )
        .expect("布局");
        assert!(!intersects(layout.bar, layout.capture));
        assert!(layout.capture.height > 32);
    }
}
