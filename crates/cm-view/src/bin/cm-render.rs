//! `cm-render <file> <out.png> [--width W] [--height H] [--top-line N] [--wrap]
//!           [--select L:C-L:C] [--font-size PX]`
//!
//! Renders one frame of the editor offscreen (no window) and writes a PNG.

use std::path::PathBuf;

use cm_core::Pos;
use cm_view::app::{App, AppOptions, MemClipboard};
use cm_view::render::{FrameState, Gpu, Renderer, render_offscreen, write_png};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut pos: Vec<String> = Vec::new();
    let (mut w, mut h, mut top, mut font) = (1000u32, 720u32, 1i64, 14.0f32);
    let mut opts = AppOptions::default();
    let mut select: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--width" => w = args.next().and_then(|v| v.parse().ok()).expect("--width N"),
            "--height" => {
                h = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--height N")
            }
            "--top-line" => {
                top = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--top-line N")
            }
            "--font-size" => {
                font = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--font-size PX")
            }
            "--wrap" => opts.line_wrapping = true,
            "--select" => select = args.next(),
            "--mode" => opts.mode = args.next(),
            _ => pos.push(a),
        }
    }
    if pos.len() != 2 {
        eprintln!(
            "usage: cm-render <file> <out.png> [--width W] [--height H] [--top-line N] [--wrap] [--select L:C-L:C] [--font-size PX]"
        );
        std::process::exit(2);
    }
    let gpu = Gpu::headless().unwrap_or_else(|e| {
        eprintln!("cm-render: {e}");
        std::process::exit(1)
    });
    let info = gpu.adapter.get_info();
    eprintln!("adapter: {} ({:?})", info.name, info.backend);
    let mut renderer = Renderer::new(
        &gpu,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        font,
        (font * 1.35).round(),
    );
    let metrics = renderer.metrics();
    let mut app = App::open(
        &PathBuf::from(&pos[0]),
        metrics,
        &opts,
        Box::new(MemClipboard::default()),
    )
    .unwrap_or_else(|e| {
        eprintln!("cm-render: {}: {e}", pos[0]);
        std::process::exit(1)
    });
    let ed = &mut app.editor;
    ed.set_size(w as f64, h as f64);
    let y =
        ed.height_at_line((top - 1).max(0)) + if top > 1 { ed.metrics.padding_top } else { 0.0 };
    ed.scroll_to(None, Some(y));
    if let Some(s) = select {
        let p = |t: &str| -> Pos {
            let (l, c) = t.split_once(':').expect("L:C");
            Pos::new(l.parse::<i64>().unwrap() - 1, c.parse::<i64>().unwrap())
        };
        let (a, b) = s.split_once('-').expect("L:C-L:C");
        ed.doc.set_selection(p(a), Some(p(b)), None);
    }
    let px = render_offscreen(&gpu, &mut renderer, ed, w, h, FrameState::default());
    write_png(&PathBuf::from(&pos[1]), w, h, &px).expect("write png");
    println!(
        "{} ({} lines, mode {}) -> {}",
        pos[0],
        ed.doc.line_count(),
        ed.mode_name(),
        pos[1]
    );
}
