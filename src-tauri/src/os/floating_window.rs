use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, Position, Runtime, WebviewWindow};

pub const FLOATING_WINDOW_LABEL: &str = "floating-card";
pub const DEFAULT_WINDOW_MARGIN: i32 = 12;
pub const DEFAULT_CURSOR_OFFSET_X: i32 = 14;
pub const DEFAULT_CURSOR_OFFSET_Y: i32 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(&self, point: Point) -> bool {
        let right = self.x.saturating_add(self.width.max(0));
        let bottom = self.y.saturating_add(self.height.max(0));
        point.x >= self.x && point.x < right && point.y >= self.y && point.y < bottom
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloatingWindowError {
    Tauri(String),
    MonitorUnavailable,
}

impl fmt::Display for FloatingWindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tauri(msg) => write!(f, "floating window error: {msg}"),
            Self::MonitorUnavailable => write!(f, "no active monitor available for floating window"),
        }
    }
}

impl std::error::Error for FloatingWindowError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatingWindowMetrics {
    pub position: Point,
    pub show_latency_ms: u64,
}

/// Computes the top-left position for a floating window near `cursor`, clamped
/// cleanly inside `monitor` with `margin` padding.
///
/// Handles negative coordinates on secondary monitors positioned left or above
/// the primary display, flips to the opposite side of the cursor when placing
/// below/right would overflow, and degrades gracefully if the window is wider
/// or taller than the padded monitor bounds.
pub fn compute_floating_position(
    cursor: Point,
    window_size: Size,
    monitor: Rect,
    margin: i32,
) -> Point {
    let safe_margin = margin.max(0);
    let win_w = window_size.width.max(0);
    let win_h = window_size.height.max(0);
    let mon_w = monitor.width.max(0);
    let mon_h = monitor.height.max(0);

    let x = clamp_axis(
        cursor.x,
        DEFAULT_CURSOR_OFFSET_X,
        win_w,
        monitor.x,
        mon_w,
        safe_margin,
    );
    let y = clamp_axis(
        cursor.y,
        DEFAULT_CURSOR_OFFSET_Y,
        win_h,
        monitor.y,
        mon_h,
        safe_margin,
    );

    Point { x, y }
}

fn clamp_axis(
    cursor_coord: i32,
    offset: i32,
    window_len: i32,
    monitor_origin: i32,
    monitor_len: i32,
    margin: i32,
) -> i32 {
    let monitor_end = monitor_origin.saturating_add(monitor_len);
    let min_padded = monitor_origin.saturating_add(margin);
    let max_padded = monitor_end
        .saturating_sub(margin)
        .saturating_sub(window_len);

    // Default placement: offset positively from cursor.
    let mut candidate = cursor_coord.saturating_add(offset);

    // If positive placement overflows the far edge, try flipping to the other side of the cursor.
    if min_padded <= max_padded && candidate > max_padded {
        candidate = cursor_coord
            .saturating_sub(offset)
            .saturating_sub(window_len);
    }

    if min_padded <= max_padded {
        candidate.clamp(min_padded, max_padded)
    } else if window_len <= monitor_len {
        // Window fits inside monitor, just not with full margin on both sides.
        let max_unpadded = monitor_end.saturating_sub(window_len);
        candidate.clamp(monitor_origin, max_unpadded)
    } else {
        // Window is larger than monitor; pin origin so the top-left stays visible.
        monitor_origin
    }
}

/// Configures a pre-warmed Tauri window as a frameless, always-on-top floating card.
pub fn configure_floating_window<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<(), FloatingWindowError> {
    window
        .set_decorations(false)
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;
    window
        .set_always_on_top(true)
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;
    window
        .set_skip_taskbar(true)
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;
    Ok(())
}

/// Positions and shows a pre-warmed floating window at the clamped cursor-near
/// location, returning the placed position and hotkey-to-visible latency.
pub fn show_floating_window_at<R: Runtime>(
    window: &WebviewWindow<R>,
    cursor: Point,
    window_size: Size,
    monitor: Rect,
    margin: i32,
    trigger_started: Instant,
) -> Result<FloatingWindowMetrics, FloatingWindowError> {
    configure_floating_window(window)?;
    let position = compute_floating_position(cursor, window_size, monitor, margin);

    window
        .set_position(Position::Physical(PhysicalPosition::new(
            position.x, position.y,
        )))
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;
    window
        .show()
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;
    window
        .set_focus()
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;

    let show_latency_ms = trigger_started.elapsed().as_millis() as u64;
    Ok(FloatingWindowMetrics {
        position,
        show_latency_ms,
    })
}

/// Resolves the monitor containing `cursor` (falling back to the window's
/// current monitor) and displays the floating window clamped within its bounds.
pub fn show_floating_window_near_cursor<R: Runtime>(
    window: &WebviewWindow<R>,
    cursor: Point,
    margin: i32,
    trigger_started: Instant,
) -> Result<FloatingWindowMetrics, FloatingWindowError> {
    let outer = window
        .outer_size()
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;
    let window_size = Size {
        width: outer.width as i32,
        height: outer.height as i32,
    };

    let monitor_rect = resolve_monitor_for_cursor(window, cursor)?;
    show_floating_window_at(
        window,
        cursor,
        window_size,
        monitor_rect,
        margin,
        trigger_started,
    )
}

fn resolve_monitor_for_cursor<R: Runtime>(
    window: &WebviewWindow<R>,
    cursor: Point,
) -> Result<Rect, FloatingWindowError> {
    let monitors = window
        .available_monitors()
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?;

    for m in &monitors {
        let rect = Rect {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width as i32,
            height: m.size().height as i32,
        };
        if rect.contains(cursor) {
            return Ok(rect);
        }
    }

    if let Some(m) = window
        .current_monitor()
        .map_err(|e| FloatingWindowError::Tauri(e.to_string()))?
    {
        return Ok(Rect {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width as i32,
            height: m.size().height as i32,
        });
    }

    Err(FloatingWindowError::MonitorUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const CARD: Size = Size {
        width: 560,
        height: 420,
    };

    #[test]
    fn places_near_cursor_inside_primary_monitor() {
        let pos = compute_floating_position(Point { x: 400, y: 300 }, CARD, PRIMARY, 12);
        assert_eq!(
            pos,
            Point {
                x: 400 + DEFAULT_CURSOR_OFFSET_X,
                y: 300 + DEFAULT_CURSOR_OFFSET_Y,
            }
        );
    }

    #[test]
    fn handles_negative_origin_left_secondary_monitor() {
        let left_monitor = Rect {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pos = compute_floating_position(Point { x: -1000, y: 300 }, CARD, left_monitor, 12);
        assert_eq!(
            pos,
            Point {
                x: -1000 + DEFAULT_CURSOR_OFFSET_X,
                y: 300 + DEFAULT_CURSOR_OFFSET_Y,
            }
        );

        // Near the right edge of the left monitor (x = -10) should clamp inside [-1908, -572].
        let edge_pos = compute_floating_position(Point { x: -10, y: 300 }, CARD, left_monitor, 12);
        assert!(edge_pos.x >= left_monitor.x + 12);
        assert!(edge_pos.x + CARD.width <= left_monitor.x + left_monitor.width - 12);
    }

    #[test]
    fn handles_negative_origin_top_secondary_monitor() {
        let top_monitor = Rect {
            x: 0,
            y: -1080,
            width: 1920,
            height: 1080,
        };
        let pos = compute_floating_position(Point { x: 500, y: -1090 }, CARD, top_monitor, 12);
        assert_eq!(pos.y, -1080 + 12);
        assert!(pos.y + CARD.height <= 0 - 12);
    }

    #[test]
    fn clamps_bottom_right_overflow() {
        let pos = compute_floating_position(Point { x: 1900, y: 1060 }, CARD, PRIMARY, 12);
        assert!(pos.x >= 12);
        assert!(pos.y >= 12);
        assert!(pos.x + CARD.width <= PRIMARY.width - 12);
        assert!(pos.y + CARD.height <= PRIMARY.height - 12);
    }

    #[test]
    fn clamps_top_left_overflow() {
        let pos = compute_floating_position(Point { x: -40, y: -30 }, CARD, PRIMARY, 12);
        assert_eq!(pos, Point { x: 12, y: 12 });
    }

    #[test]
    fn handles_window_larger_than_padded_margin_or_monitor() {
        // Window fits monitor (580x440) but not with 20px margins on both sides.
        let tight_monitor = Rect {
            x: 100,
            y: 50,
            width: 580,
            height: 440,
        };
        let pos = compute_floating_position(Point { x: 300, y: 200 }, CARD, tight_monitor, 20);
        assert!(pos.x >= tight_monitor.x);
        assert!(pos.x + CARD.width <= tight_monitor.x + tight_monitor.width);
        assert!(pos.y >= tight_monitor.y);
        assert!(pos.y + CARD.height <= tight_monitor.y + tight_monitor.height);

        // Window larger than monitor pins to monitor origin.
        let tiny_monitor = Rect {
            x: -800,
            y: -600,
            width: 400,
            height: 300,
        };
        let pinned = compute_floating_position(Point { x: -500, y: -400 }, CARD, tiny_monitor, 12);
        assert_eq!(pinned, Point { x: -800, y: -600 });
    }
}
