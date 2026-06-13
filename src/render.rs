use std::sync::OnceLock;

use streamdeck_lib::Context;
use streamdeck_render::{
    Canvas, Color, FontHandle, FontRegistry, HAlign, TextOptions, VAlign, WrapOptions,
    measure_line, wrap_text,
};

// ── Palette ────────────────────────────────────────────────────────────────
/// Positive (add) adjustment — green.
const ADD_COLOR: Color = Color::rgb(64, 200, 110);
/// Negative (subtract) adjustment — red.
const SUB_COLOR: Color = Color::rgb(231, 90, 76);
/// Background fill for the expired ("DONE") timer state — unmistakable red.
const DONE_BG: Color = Color::rgb(184, 50, 40);
/// Secondary label (the timer name) — dimmed white, readable on dark or red.
const LABEL_COLOR: Color = Color::rgba(255, 255, 255, 190);

// Embed the font at compile time — no runtime file I/O needed.
static FONT: OnceLock<FontHandle> = OnceLock::new();

fn font() -> &'static FontHandle {
    FONT.get_or_init(|| {
        let mut reg = FontRegistry::new();
        reg.load_bytes(
            "mono",
            include_bytes!(
                "../icu.veelume.counter.sdPlugin/fonts/UAV-OSD-Sans-Mono.ttf"
            ),
        )
        .expect("embedded font must load")
    })
}

/// Render an integer value onto a Stream Deck button (144×144 PNG).
///
/// The font size scales down automatically for long numbers so they always fit.
pub fn render_number(cx: &Context, ctx_id: &str, value: i64) {
    let text = value.to_string();
    render_centered_text(cx, ctx_id, &text);
}

/// All time values are sized against this reference so `01:45`, `05:00`, and
/// `59:59` render at one consistent font size. The mono font makes every glyph
/// the same width, so any zero-padded `NN:NN` value matches this width exactly.
const TIME_SIZE_REF: &str = "00:00";

/// Format a duration for a button: `MM:SS` below one hour, `HH:MM` (hours and
/// minutes, no seconds) from one hour up. Both forms are zero-padded to five
/// glyphs so successive renders stay aligned. Short durations keep ticking by
/// the second; long ones display without the old 99:59 truncation. The lack of
/// a ticking seconds field (plus the blinking colon) is what reads as "hours".
///
/// `sep` is the minutes separator — normally `':'`, but the caller passes `' '`
/// to blink the colon off for the running "heartbeat".
fn fmt_duration(total_secs: u64, sep: char) -> String {
    if total_secs < 3600 {
        format!("{:02}{sep}{:02}", total_secs / 60, total_secs % 60)
    } else {
        format!("{:02}{sep}{:02}", total_secs / 3600, (total_secs % 3600) / 60)
    }
}

/// Render a time value (timer remaining or stopwatch elapsed) with the optional
/// `name` as a small label beneath when non-empty. See [`fmt_duration`].
///
/// While `running`, the colon blinks once per second (visible on even seconds)
/// so a long timer reads as alive even when its minutes aren't changing; when
/// paused it stays solid. Sized against [`TIME_SIZE_REF`] for a stable size.
pub fn render_time(cx: &Context, ctx_id: &str, total_secs: u64, name: &str, running: bool) {
    let sep = if running && !total_secs.is_multiple_of(2) { ' ' } else { ':' };
    render_labeled(
        cx,
        ctx_id,
        &fmt_duration(total_secs, sep),
        TIME_SIZE_REF,
        Color::WHITE,
        name,
        Color::TRANSPARENT,
    );
}

/// Render a "+/-" adjustment button: a signed, color-coded delta (green for
/// add, red for subtract) over the target timer `name`.
pub fn render_adjust(cx: &Context, ctx_id: &str, delta_secs: i64, name: &str) {
    let color = if delta_secs < 0 { SUB_COLOR } else { ADD_COLOR };
    let sign = if delta_secs < 0 { '-' } else { '+' };
    let abs = delta_secs.unsigned_abs();
    // No leading zero on minutes — reads as an adjustment ("+1:30"), not a clock.
    let text = format!("{sign}{}:{:02}", abs / 60, abs % 60);
    render_labeled(cx, ctx_id, &text, &text, color, name, Color::TRANSPARENT);
}

/// Render the timer's "expired" state — a filled red background with "DONE"
/// so a finished timer is impossible to miss at a glance. The label beneath is
/// the timer `name` when set, otherwise the `reset_secs` it returns to on reset.
pub fn render_expired(cx: &Context, ctx_id: &str, name: &str, reset_secs: u64) {
    let reset_label;
    let label = if name.trim().is_empty() {
        reset_label = fmt_duration(reset_secs, ':');
        reset_label.as_str()
    } else {
        name
    };
    render_labeled(cx, ctx_id, "DONE", "DONE", Color::WHITE, label, DONE_BG);
}

/// Render any short text string centered on a button, with auto-scaling font size.
fn render_centered_text(cx: &Context, ctx_id: &str, text: &str) {
    let font = font();

    // Try font sizes from largest to smallest until the text fits in one line.
    let sizes = [56.0_f32, 44.0, 36.0, 28.0, 20.0];
    let max_width = 136.0_f32; // leave a small margin inside 144px

    let chosen_size = sizes
        .iter()
        .copied()
        .find(|&size| {
            let opts = WrapOptions { max_width, max_lines: 1 };
            let lines = wrap_text(font, size, text, &opts);
            lines.len() == 1 && lines[0].width_px <= max_width
        })
        .unwrap_or(20.0); // fallback: always render at minimum size

    let opts = WrapOptions { max_width, max_lines: 1 };
    let lines = wrap_text(font, chosen_size, text, &opts);

    let mut canvas = Canvas::key_icon();
    if !lines.is_empty() {
        canvas
            .draw_text(&lines, &TextOptions::new(font.clone(), chosen_size))
            .ok();
    }

    if let Ok(data_url) = canvas.finish().to_data_url() {
        cx.sd().set_image(ctx_id, Some(data_url), None, None);
    }
}

/// Render a large primary string with an optional small `label` beneath it,
/// over an optional background fill (`Color::TRANSPARENT` for none).
///
/// The primary is sized to fit the wider of itself and `size_ref` — pass a
/// fixed reference (e.g. `TIME_SIZE_REF`) to keep a changing value at a stable
/// size, or pass `primary` itself for plain content-fit sizing.
///
/// With a label, the primary is seated in the upper area and the label near
/// the bottom (truncated to fit). Without one, the primary is centered — so a
/// nameless timer looks exactly as it did before.
fn render_labeled(
    cx: &Context,
    ctx_id: &str,
    primary: &str,
    size_ref: &str,
    primary_color: Color,
    label: &str,
    bg: Color,
) {
    const MAX_WIDTH: f32 = 136.0; // small margin inside the 144px icon

    let font = font();
    let mut canvas = Canvas::key_icon();
    if bg.a > 0 {
        canvas.fill(bg);
    }

    // Size against whichever is wider so a fixed reference can pin the size.
    let sizing = wider(font, primary, size_ref);

    let label = label.trim();
    if label.is_empty() {
        // No label: center the primary, full size range (unchanged look).
        let size = fit_size(font, sizing, MAX_WIDTH, &[56.0, 44.0, 36.0, 28.0, 20.0]);
        draw_line(&mut canvas, font, primary, size, primary_color, VAlign::Center);
    } else {
        // Primary in the upper area, slightly smaller to leave room.
        let size = fit_size(font, sizing, MAX_WIDTH, &[52.0, 44.0, 36.0, 28.0, 20.0]);
        draw_line(
            &mut canvas,
            font,
            primary,
            size,
            primary_color,
            VAlign::Baseline(84.0),
        );

        // Label near the bottom, auto-sized then truncated to fit.
        let lsize = fit_size(font, label, MAX_WIDTH, &[24.0, 20.0, 16.0]);
        let text = truncate_to_width(font, label, lsize, MAX_WIDTH);
        draw_line(
            &mut canvas,
            font,
            &text,
            lsize,
            LABEL_COLOR,
            VAlign::Baseline(128.0),
        );
    }

    if let Ok(data_url) = canvas.finish().to_data_url() {
        cx.sd().set_image(ctx_id, Some(data_url), None, None);
    }
}

/// Return whichever of `a` / `b` renders wider (measured at a common scale).
fn wider<'a>(font: &FontHandle, a: &'a str, b: &'a str) -> &'a str {
    if measure_line(font, 10.0, a) >= measure_line(font, 10.0, b) {
        a
    } else {
        b
    }
}

/// Pick the largest size from `sizes` at which `text` fits within `max_width`,
/// falling back to the smallest provided size.
fn fit_size(font: &FontHandle, text: &str, max_width: f32, sizes: &[f32]) -> f32 {
    sizes
        .iter()
        .copied()
        .find(|&s| measure_line(font, s, text) <= max_width)
        .unwrap_or_else(|| sizes.last().copied().unwrap_or(20.0))
}

/// Trim `text` (appending "..") until it fits `max_width` at `size`. Uses ".."
/// rather than an ellipsis glyph, which the OSD mono font may not carry.
fn truncate_to_width(font: &FontHandle, text: &str, size: f32, max_width: f32) -> String {
    if measure_line(font, size, text) <= max_width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "..";
        if measure_line(font, size, &candidate) <= max_width {
            return candidate;
        }
    }
    "..".to_string()
}

/// Draw a single centered line of `text` at `size`/`color`/`valign`.
fn draw_line(
    canvas: &mut Canvas,
    font: &FontHandle,
    text: &str,
    size: f32,
    color: Color,
    valign: VAlign,
) {
    // max_lines: 1 forces everything onto one line regardless of width.
    let opts = WrapOptions { max_width: 144.0, max_lines: 1 };
    let lines = wrap_text(font, size, text, &opts);
    if lines.is_empty() {
        return;
    }
    let topts = TextOptions::new(font.clone(), size)
        .color(color)
        .h_align(HAlign::Center)
        .v_align(valign);
    canvas.draw_text(&lines, &topts).ok();
}

#[cfg(test)]
mod tests {
    use super::fmt_duration;

    #[test]
    fn under_an_hour_is_mm_ss() {
        assert_eq!(fmt_duration(0, ':'), "00:00");
        assert_eq!(fmt_duration(59, ':'), "00:59");
        assert_eq!(fmt_duration(60, ':'), "01:00");
        assert_eq!(fmt_duration(3599, ':'), "59:59"); // one second under an hour
    }

    #[test]
    fn from_an_hour_up_is_hh_mm() {
        assert_eq!(fmt_duration(3600, ':'), "01:00"); // exactly one hour, zero-padded
        assert_eq!(fmt_duration(3660, ':'), "01:01");
        assert_eq!(fmt_duration(5400, ':'), "01:30"); // 1h30m
        assert_eq!(fmt_duration(6000, ':'), "01:40"); // would have been 100:00 before
        assert_eq!(fmt_duration(359_940, ':'), "99:59"); // bump clamp ceiling
    }

    #[test]
    fn blink_separator_swaps_the_colon() {
        assert_eq!(fmt_duration(90, ' '), "01 30"); // colon blinked off
        assert_eq!(fmt_duration(5400, ' '), "01 30"); // same in HH:MM mode
    }
}
