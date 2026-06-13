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

/// Format seconds as `MM:SS` (minutes capped at 99).
fn fmt_mmss(total_secs: u64) -> String {
    let mins = (total_secs / 60).min(99);
    let secs = total_secs % 60;
    format!("{:02}:{:02}", mins, secs)
}

/// Render remaining seconds in `MM:SS` format (max 99:59), with the timer
/// `name` as a small label beneath when it is non-empty.
pub fn render_time_mmss(cx: &Context, ctx_id: &str, total_secs: u64, name: &str) {
    render_labeled(cx, ctx_id, &fmt_mmss(total_secs), Color::WHITE, name, Color::TRANSPARENT);
}

/// Render a "+/-" adjustment button: a signed, color-coded delta (green for
/// add, red for subtract) over the target timer `name`.
pub fn render_adjust(cx: &Context, ctx_id: &str, delta_secs: i64, name: &str) {
    let color = if delta_secs < 0 { SUB_COLOR } else { ADD_COLOR };
    let sign = if delta_secs < 0 { '-' } else { '+' };
    let abs = delta_secs.unsigned_abs();
    // No leading zero on minutes — reads as an adjustment ("+1:30"), not a clock.
    let text = format!("{sign}{}:{:02}", abs / 60, abs % 60);
    render_labeled(cx, ctx_id, &text, color, name, Color::TRANSPARENT);
}

/// Render elapsed seconds in `HH:MM:SS` format (for values ≥ 3600 s).
/// Falls back to `MM:SS` for shorter durations.
pub fn render_time_hhmmss(cx: &Context, ctx_id: &str, total_secs: u64) {
    let text = if total_secs >= 3600 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{:02}:{:02}", mins, secs)
    };
    render_centered_text(cx, ctx_id, &text);
}

/// Render the timer's "expired" state — a filled red background with "DONE"
/// so a finished timer is impossible to miss at a glance. The label beneath is
/// the timer `name` when set, otherwise the `reset_secs` it returns to on reset.
pub fn render_expired(cx: &Context, ctx_id: &str, name: &str, reset_secs: u64) {
    let reset_label;
    let label = if name.trim().is_empty() {
        reset_label = fmt_mmss(reset_secs);
        reset_label.as_str()
    } else {
        name
    };
    render_labeled(cx, ctx_id, "DONE", Color::WHITE, label, DONE_BG);
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
/// With a label, the primary is seated in the upper area and the label near
/// the bottom (truncated to fit). Without one, the primary is centered — so a
/// nameless timer looks exactly as it did before.
fn render_labeled(
    cx: &Context,
    ctx_id: &str,
    primary: &str,
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

    let label = label.trim();
    if label.is_empty() {
        // No label: center the primary, full size range (unchanged look).
        let size = fit_size(font, primary, MAX_WIDTH, &[56.0, 44.0, 36.0, 28.0, 20.0]);
        draw_line(&mut canvas, font, primary, size, primary_color, VAlign::Center);
    } else {
        // Primary in the upper area, slightly smaller to leave room.
        let size = fit_size(font, primary, MAX_WIDTH, &[52.0, 44.0, 36.0, 28.0, 20.0]);
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
