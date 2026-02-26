use std::sync::OnceLock;

use streamdeck_lib::Context;
use streamdeck_render::{Canvas, FontHandle, FontRegistry, TextOptions, WrapOptions, wrap_text};

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

/// Render elapsed/remaining seconds in `MM:SS` format (max 99:59).
pub fn render_time_mmss(cx: &Context, ctx_id: &str, total_secs: u64) {
    let mins = (total_secs / 60).min(99);
    let secs = total_secs % 60;
    let text = format!("{:02}:{:02}", mins, secs);
    render_centered_text(cx, ctx_id, &text);
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
