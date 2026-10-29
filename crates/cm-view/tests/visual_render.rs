//! Deterministic visual tests of the wgpu renderer (`cm_view::render`).
//!
//! Every case renders one offscreen frame with fixed inputs and checks it two ways:
//!
//! * **Geometry.** The cursor, selection, gutter and scrollbar rectangles are found
//!   in the pixels and compared with the editor model's coordinates
//!   (`Editor::cursor_coords`, `height_at_line`, `doc_height`, `gutter_width`).
//!   The cursor is the set of pixels that change when `cursor_visible` is toggled;
//!   the selection is the set of pixels that change when the selection collapses.
//! * **Appearance.** The frame is compared with the golden PNG of the reviewed golden
//!   set for the running adapter under [`CHANNEL_TOLERANCE`] and [`MAX_DIFF_FRACTION`].
//!
//! Golden sets. A golden set is `tests/visual/goldens/` or one of its immediate
//! subdirectories that holds `ADAPTER.txt` (the adapter identity: name, backend,
//! device type, vendor, device, driver, driver info, as [`gpu`] prints them) and
//! `REVIEWED.txt` (who reviewed the images, when, and against what). The running
//! adapter uses the one set whose identity matches all seven fields exactly; two
//! matching sets fail the run. With no matching set (another GPU, another driver
//! version, macOS Metal) the geometry and token checks still run, the pixel
//! comparison is reported as unverified for that adapter, and the case ends with
//! `cm_evidence::golden_unverified`: it fails under `CM_EVIDENCE=strict` and passes
//! under `ci` only when `CM_EVIDENCE_ALLOW` names `golden-unverified`. Frames are
//! never compared with another adapter's goldens.
//!
//! Fixed inputs: the bundled font `tests/visual/fonts/DejaVuSansMono.ttf` (SHA-256
//! checked before any rendering; no system font is loaded), a logical viewport of
//! [`LOGICAL_W`] x [`LOGICAL_H`] at scale 1.0 or 2.0 (physical pixels =
//! logical x scale, font `round(14 x scale)` px, line height `round(font x 1.35)`,
//! as `window.rs` derives them), the CM5 default theme, a focused editor with the
//! cursor in its visible blink phase, fixed text, mode, selection and scroll.
//!
//! Outputs go to `$CM_VISUAL_OUT` (default `<workspace>/target/cm-visual/`): every
//! rendered frame as `<case>.png`, a `<case>.txt` record, `adapter.txt`, and on a
//! golden mismatch `<case>.diff.png`. `CM_VISUAL_BLESS=1` also writes each frame and
//! the adapter identity to `<out>/candidates/` as an unreviewed candidate set; the
//! test never writes `tests/visual/goldens/`. A candidate becomes a golden set only
//! when a reviewer copies it there with a `REVIEWED.txt`. A missing golden in a
//! reviewed set fails.
//!
//! No GPU adapter, or only a software (CPU) adapter: `cm_evidence::no_gpu`.
#![cfg(feature = "gpu")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use cm_core::{Pos, Sticky};
use cm_modes::Spec;
use cm_view::editor::sel_dont_scroll;
use cm_view::render::{FrameState, Gpu, Renderer, Rgba, Theme, render_offscreen, write_png};
use cm_view::{CoordSys, Editor, EditorOptions};
use sha2::{Digest, Sha256};

/// The bundled test font, relative to the crate.
const FONT_FILE: &str = "tests/visual/fonts/DejaVuSansMono.ttf";
/// SHA-256 of [`FONT_FILE`]; the test refuses to render with any other bytes.
const FONT_SHA256: &str = "c805f9436dbc268644c1d9584f01a601a653e028e08fd74b9b949f6cf8304d88";
/// Name-table version string (name ID 5) of [`FONT_FILE`], for the record.
const FONT_VERSION: &str = "Version 2.37";

/// Logical viewport. Physical size is this times the case's scale factor.
const LOGICAL_W: u32 = 480;
const LOGICAL_H: u32 = 200;
/// Logical font size; `window.rs` uses `round(14 * scale_factor)`.
const BASE_FONT_PX: f32 = 14.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Golden comparison: a pixel differs when any RGBA channel differs by more than
/// this. Measured run-to-run variance on the recording adapter is 0: every frame
/// was bit-identical over 7 processes (direct and under `xvfb-run -a`) and on a
/// second render in the same process (`RENDERER.md`). Glyphs are rasterized on
/// the CPU (swash); the GPU only blends and sRGB-encodes, so 1 allows one 8-bit
/// quantization step of a different conforming sRGB encode, the smallest non-zero
/// threshold. It is not needed on the recording adapter.
const CHANNEL_TOLERANCE: u8 = 1;
/// Golden comparison: the case fails when more than this fraction of pixels
/// differ. Measured variance is 0 differing pixels, so this is 0: one pixel
/// beyond [`CHANNEL_TOLERANCE`] fails the case.
const MAX_DIFF_FRACTION: f64 = 0.0;
/// Geometry: allowed distance in pixels between a measured edge and the edge the
/// model predicts. Quads are drawn without MSAA, so pixel `i` is covered when
/// `left <= i + 0.5 < right` (the D3D/Vulkan fill rule); the prediction applies the
/// same rule. The only ambiguity is an edge falling exactly on a pixel center,
/// where the f32 round trip through NDC (`x / w * 2 - 1`) can land either side:
/// one pixel. The measured deltas are recorded in each case's `.txt`.
const GEOMETRY_TOLERANCE_PX: i64 = 1;

// -------------------------------------------------------------------------------
// Fixed inputs

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// The bundled font, after its checksum matched. Panics on a mismatch.
fn font_bytes() -> &'static [u8] {
    static FONT: OnceLock<Vec<u8>> = OnceLock::new();
    FONT.get_or_init(|| {
        let path = crate_dir().join(FONT_FILE);
        let data =
            std::fs::read(&path).unwrap_or_else(|e| panic!("bundled font {}: {e}", path.display()));
        let sum = sha256_hex(&data);
        assert_eq!(
            sum,
            FONT_SHA256,
            "bundled font {} has SHA-256 {sum}, expected {FONT_SHA256}: the test font changed",
            path.display()
        );
        data
    })
}

fn out_dir() -> PathBuf {
    let dir = std::env::var_os("CM_VISUAL_OUT")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| cm_evidence::workspace_root().join("target/cm-visual"));
    // Frames are written here on every run; they must never land on the goldens.
    // Checked before creating the directory, and again with symlinks resolved.
    let inside = |d: &Path, g: &Path| {
        assert!(
            !d.starts_with(g),
            "CM_VISUAL_OUT {} is inside the golden sets {}",
            d.display(),
            g.display()
        )
    };
    inside(
        &std::path::absolute(&dir).unwrap(),
        &std::path::absolute(golden_dir()).unwrap(),
    );
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    inside(
        &dir.canonicalize().unwrap(),
        &golden_dir().canonicalize().unwrap(),
    );
    dir
}

fn golden_dir() -> PathBuf {
    crate_dir().join("tests/visual/goldens")
}

/// Where `CM_VISUAL_BLESS=1` writes unreviewed candidate goldens.
fn candidate_dir() -> PathBuf {
    out_dir().join("candidates")
}

/// The identity lines of an `ADAPTER.txt` (or of [`gpu`]'s description).
fn identity(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// The reviewed golden set whose `ADAPTER.txt` matches `desc` exactly, if any. A set
/// without `REVIEWED.txt` is not a golden set. More than one match is an error.
fn golden_set(desc: &str) -> Result<Option<PathBuf>, String> {
    golden_set_in(&golden_dir(), desc)
}

/// [`golden_set`] under `root`.
fn golden_set_in(root: &Path, desc: &str) -> Result<Option<PathBuf>, String> {
    let root = root.to_path_buf();
    let mut dirs = vec![root.clone()];
    if let Ok(rd) = std::fs::read_dir(&root) {
        let mut sub: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_dir())
            .collect();
        sub.sort();
        dirs.extend(sub);
    }
    let want = identity(desc);
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let Ok(adapter) = std::fs::read_to_string(dir.join("ADAPTER.txt")) else {
            continue;
        };
        let reviewed = std::fs::read_to_string(dir.join("REVIEWED.txt")).unwrap_or_default();
        if reviewed.trim().is_empty() {
            continue;
        }
        if identity(&adapter) == want {
            found.push(dir);
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(format!(
            "more than one golden set matches this adapter: {found:?}"
        )),
    }
}

fn blessing() -> bool {
    std::env::var("CM_VISUAL_BLESS").as_deref() == Ok("1")
}

/// The shared device and a description of it, or why there is none.
fn gpu() -> &'static Result<(Gpu, String), String> {
    static GPU: OnceLock<Result<(Gpu, String), String>> = OnceLock::new();
    GPU.get_or_init(|| {
        let gpu = Gpu::headless()?;
        let i = gpu.adapter.get_info();
        let desc = format!(
            "adapter: {}\nbackend: {:?}\ndevice_type: {:?}\nvendor: 0x{:04x}\ndevice: 0x{:04x}\ndriver: {}\ndriver_info: {}\n",
            i.name, i.backend, i.device_type, i.vendor, i.device, i.driver, i.driver_info
        );
        if i.device_type == wgpu::DeviceType::Cpu {
            return Err(format!(
                "only a software adapter is available ({}, {:?}): no GPU",
                i.name, i.backend
            ));
        }
        let mut record = desc.clone();
        let _ = writeln!(record, "format: {FORMAT:?}");
        let _ = writeln!(
            record,
            "font: {FONT_FILE} ({FONT_VERSION}, sha256 {FONT_SHA256})"
        );
        let _ = writeln!(
            record,
            "viewport: {LOGICAL_W}x{LOGICAL_H} logical; tolerance: channel {CHANNEL_TOLERANCE}, fraction {MAX_DIFF_FRACTION}, geometry {GEOMETRY_TOLERANCE_PX}px"
        );
        let set = match golden_set(&desc) {
            Ok(Some(dir)) => format!("golden set: {}", dir.display()),
            Ok(None) => "golden set: none for this adapter (pixel comparison unverified)".into(),
            Err(e) => panic!("{e}"),
        };
        let _ = writeln!(record, "{set}");
        let _ = std::fs::write(out_dir().join("adapter.txt"), &record);
        eprint!("{record}");
        if blessing() {
            std::fs::create_dir_all(candidate_dir()).unwrap();
            std::fs::write(candidate_dir().join("ADAPTER.txt"), &desc).unwrap();
        }
        Ok((gpu, desc))
    })
}

struct Case {
    name: &'static str,
    scale: f32,
    text: String,
    mode: &'static str,
    line_numbers: bool,
    wrap: bool,
    anchor: Pos,
    head: Pos,
    /// Scroll so this line is at the top.
    scroll_line: Option<i64>,
    /// `(line, from, to, style, color)` tokens whose style and drawn color are checked.
    tokens: Vec<(i64, i64, i64, &'static str, u32)>,
}

impl Case {
    fn new(name: &'static str, text: &str, head: Pos) -> Case {
        Case {
            name,
            scale: 1.0,
            text: text.to_string(),
            mode: "text/plain",
            line_numbers: false,
            wrap: false,
            anchor: head,
            head,
            scroll_line: None,
            tokens: Vec::new(),
        }
    }
    fn scale(mut self, s: f32) -> Case {
        self.scale = s;
        self
    }
    fn select(mut self, anchor: Pos) -> Case {
        self.anchor = anchor;
        self
    }
}

const PLAIN: &str = "The quick brown fox jumps over the lazy dog.\n\
0123456789 !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~\n  indented line with trailing text\n";

const JS: &str = "// Fibonacci, iteratively\n\
function fib(n) {\n  let a = 0, b = 1;\n  for (let i = 0; i < n; i++) [a, b] = [b, a + b];\n  \
return \"fib(\" + n + \") = \" + a;\n}\n";

const WRAPPED: &str = "Wrapped: a long line that does not fit in the viewport width, so the \
editor breaks it into several visual rows at word boundaries, and the selection and cursor \
must follow those rows exactly as the measurement model says.\nnext line\n";

fn p(line: i64, ch: i64) -> Pos {
    Pos::new(line, ch)
}

fn js_case(name: &'static str) -> Case {
    Case {
        mode: "javascript",
        tokens: vec![
            (0, 0, 25, "comment", 0xaa5500),
            (1, 0, 8, "keyword", 0x770088),
            (1, 9, 12, "def", 0x0000ff),
            (2, 2, 5, "keyword", 0x770088),
            (2, 10, 11, "number", 0x116644),
            (4, 9, 15, "string", 0xaa1111),
        ],
        ..Case::new(name, JS, p(5, 1))
    }
}

fn scroll_text() -> String {
    (1..=60)
        .map(|i| format!("line {i:02}: scrolled text\n"))
        .collect()
}

fn case(name: &str) -> Case {
    match name {
        "plain_text" => Case::new("plain_text", PLAIN, p(2, 34)),
        "js_highlight" => js_case("js_highlight"),
        "js_highlight_2x" => js_case("js_highlight_2x").scale(2.0),
        "selection_single_line" => {
            Case::new("selection_single_line", PLAIN, p(0, 19)).select(p(0, 4))
        }
        "selection_multiline" => Case::new("selection_multiline", PLAIN, p(2, 8)).select(p(0, 10)),
        "selection_multiline_2x" => Case::new("selection_multiline_2x", PLAIN, p(2, 8))
            .select(p(0, 10))
            .scale(2.0),
        "cursor_position" => Case::new("cursor_position", PLAIN, p(1, 13)),
        "cursor_position_2x" => Case::new("cursor_position_2x", PLAIN, p(1, 13)).scale(2.0),
        "line_numbers" => Case {
            line_numbers: true,
            ..js_case("line_numbers")
        }
        .with_head(p(2, 6)),
        "line_numbers_2x" => Case {
            line_numbers: true,
            ..js_case("line_numbers_2x")
        }
        .with_head(p(2, 6))
        .scale(2.0),
        "scrollbar" => Case {
            scroll_line: Some(20),
            ..Case::new("scrollbar", &scroll_text(), p(25, 4))
        },
        "wrapped_line" => Case {
            wrap: true,
            ..Case::new("wrapped_line", WRAPPED, p(0, 75))
        },
        "wrapped_selection" => Case {
            wrap: true,
            ..Case::new("wrapped_selection", WRAPPED, p(0, 140)).select(p(0, 20))
        },
        "wrapped_selection_2x" => Case {
            wrap: true,
            ..Case::new("wrapped_selection_2x", WRAPPED, p(0, 140))
                .select(p(0, 20))
                .scale(2.0)
        },
        _ => panic!("unknown case {name}"),
    }
}

impl Case {
    fn with_head(mut self, head: Pos) -> Case {
        self.anchor = head;
        self.head = head;
        self
    }
}

// -------------------------------------------------------------------------------
// Images

struct Image {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Image {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }
    fn read(path: &Path) -> Result<Image, String> {
        let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut dec = png::Decoder::new(std::io::BufReader::new(f));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut r = dec.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; r.output_buffer_size().ok_or("png too large")?];
        let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(format!(
                "{}: {:?}/{:?}, expected RGBA8",
                path.display(),
                info.color_type,
                info.bit_depth
            ));
        }
        buf.truncate(info.buffer_size());
        Ok(Image {
            w: info.width,
            h: info.height,
            px: buf,
        })
    }
}

fn lin(v: u8) -> f64 {
    let c = v as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn enc(l: f64) -> u8 {
    let c = if l <= 0.0031308 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}

fn theme_px(c: Rgba) -> [u8; 4] {
    [
        (c[0] * 255.0).round() as u8,
        (c[1] * 255.0).round() as u8,
        (c[2] * 255.0).round() as u8,
        255,
    ]
}

/// Bounding boxes of the pixels where `a` and `b` differ, one per run of rows with the
/// same x-extent: `(x0, x1, y0, y1)`, inclusive.
fn diff_regions(a: &Image, b: &Image) -> Vec<(i64, i64, i64, i64)> {
    let mut out: Vec<(i64, i64, i64, i64)> = Vec::new();
    for y in 0..a.h {
        let xs: Vec<u32> = (0..a.w).filter(|&x| a.at(x, y) != b.at(x, y)).collect();
        let (Some(&x0), Some(&x1)) = (xs.first(), xs.last()) else {
            continue;
        };
        let (x0, x1, y) = (x0 as i64, x1 as i64, y as i64);
        match out.last_mut() {
            Some(r) if r.0 == x0 && r.1 == x1 && r.3 == y - 1 => r.3 = y,
            _ => out.push((x0, x1, y, y)),
        }
    }
    out
}

/// Pixels `[first, last]` a quad edge pair `[a, b)` covers (pixel centers at `i + 0.5`).
fn span(a: f64, b: f64) -> (i64, i64) {
    ((a - 0.5).ceil() as i64, (b - 0.5).ceil() as i64 - 1)
}

/// A rectangle in pixel spans: `(x0, x1, y0, y1)`, inclusive.
fn px_rect(x0: f64, y0: f64, x1: f64, y1: f64, w: u32, h: u32) -> (i64, i64, i64, i64) {
    let (a, b) = span(x0, x1);
    let (c, d) = span(y0, y1);
    (a.max(0), b.min(w as i64 - 1), c.max(0), d.min(h as i64 - 1))
}

// -------------------------------------------------------------------------------
// Rendering

struct Scene {
    renderer: Renderer,
    editor: Editor,
    w: u32,
    h: u32,
}

impl Scene {
    fn new(gpu: &Gpu, c: &Case) -> Scene {
        let font_px = (BASE_FONT_PX * c.scale).round();
        let line_h = (font_px * 1.35).round();
        let mut renderer =
            Renderer::with_font_data(gpu, FORMAT, font_px, line_h, font_bytes().to_vec())
                .expect("renderer with the bundled font");
        renderer.theme = Theme::default();
        let metrics = renderer.metrics();
        cm_core::clock::manual(1e12);
        let opts = EditorOptions {
            line_numbers: c.line_numbers,
            line_wrapping: c.wrap,
            ..Default::default()
        };
        let (w, h) = (
            (LOGICAL_W as f32 * c.scale) as u32,
            (LOGICAL_H as f32 * c.scale) as u32,
        );
        let mut editor = Editor::new(&c.text, Spec::from(c.mode), opts, metrics);
        editor.set_size(w as f64, h as f64);
        editor
            .doc
            .set_selection(c.anchor, Some(c.head), sel_dont_scroll());
        if let Some(line) = c.scroll_line {
            let y = editor.height_at_line(line) + editor.metrics.padding_top;
            editor.scroll_to(None, Some(y));
        }
        Scene {
            renderer,
            editor,
            w,
            h,
        }
    }

    fn frame(&mut self, gpu: &Gpu, cursor_visible: bool) -> Image {
        let px = render_offscreen(
            gpu,
            &mut self.renderer,
            &mut self.editor,
            self.w,
            self.h,
            FrameState {
                focused: true,
                cursor_visible,
            },
        );
        Image {
            w: self.w,
            h: self.h,
            px,
        }
    }

    /// Window x/y of a model "div" coordinate (what `render.rs` draws at).
    fn wx(&self, x: f64) -> f64 {
        x + self.editor.gutter_width - self.editor.display.scroll_left
    }
    fn wy(&self, y: f64) -> f64 {
        y + self.editor.metrics.padding_top - self.editor.display.scroll_top
    }
}

/// Results of one case: lines for the record, and failures.
#[derive(Default)]
struct Report {
    log: String,
    failures: Vec<String>,
    max_geometry_delta: i64,
    /// Why the pixel comparison could not be made, when it could not.
    unverified: Option<String>,
}

impl Report {
    fn note(&mut self, s: impl AsRef<str>) {
        self.log.push_str(s.as_ref());
        self.log.push('\n');
    }
    fn fail(&mut self, s: impl AsRef<str>) {
        self.note(format!("FAIL {}", s.as_ref()));
        self.failures.push(s.as_ref().to_string());
    }
    /// Compare measured and expected pixel rectangles edge by edge.
    fn rect(&mut self, what: &str, got: (i64, i64, i64, i64), want: (i64, i64, i64, i64)) {
        let d = [
            (got.0 - want.0).abs(),
            (got.1 - want.1).abs(),
            (got.2 - want.2).abs(),
            (got.3 - want.3).abs(),
        ];
        let max = *d.iter().max().unwrap();
        self.max_geometry_delta = self.max_geometry_delta.max(max);
        let line = format!(
            "{what}: measured x {}..={} y {}..={}, model x {}..={} y {}..={}, max delta {max}px",
            got.0, got.1, got.2, got.3, want.0, want.1, want.2, want.3
        );
        if max > GEOMETRY_TOLERANCE_PX {
            self.fail(line);
        } else {
            self.note(format!("ok {line}"));
        }
    }
}

fn check_cursor(gpu: &Gpu, s: &mut Scene, shown: &Image, r: &mut Report) {
    let hidden = s.frame(gpu, false);
    let regions = diff_regions(shown, &hidden);
    let head = s.editor.doc.selection().ranges[0].head;
    let c = s.editor.cursor_coords(head, CoordSys::Div);
    // `.CodeMirror-cursor { border-left: 1px solid }`: one CSS pixel at this scale.
    let cw = (s.editor.metrics.char_width / 8.0).max(1.0).round();
    let want = px_rect(
        s.wx(c.left),
        s.wy(c.top),
        s.wx(c.left) + cw,
        s.wy(c.bottom),
        s.w,
        s.h,
    );
    r.note(format!(
        "cursor at {}:{} model div rect left {:.3} top {:.3} bottom {:.3}, width {cw}",
        head.line, head.ch, c.left, c.top, c.bottom
    ));
    // Pixels already black under the cursor (glyph ink) do not change, so the
    // changed pixels can come in pieces; their bounding box is the cursor, and since
    // the cursor is drawn above the text every pixel in it must be the cursor color.
    let Some(bbox) = regions
        .iter()
        .copied()
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)))
    else {
        r.fail("cursor: no pixel changes when the cursor is shown");
        return;
    };
    r.note(format!("cursor changed-pixel pieces: {regions:?}"));
    r.rect("cursor", bbox, want);
    let color = theme_px(s.renderer.theme.cursor);
    let (x0, x1, y0, y1) = bbox;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let px = shown.at(x as u32, y as u32);
            if px != color {
                r.fail(format!(
                    "cursor pixel ({x},{y}) is {}, theme cursor {}",
                    hex(px),
                    hex(color)
                ));
                return;
            }
        }
    }
}

/// The selection rectangles `drawSelectionRange` lays out, from `cursor_coords`.
fn expected_selection(s: &Scene) -> Vec<(i64, i64, i64, i64)> {
    let ed = &s.editor;
    let sel = ed.doc.selection();
    let r = &sel.ranges[0];
    let (a, b) = (r.from(), r.to());
    let right = ed.display.width - ed.gutter_width + ed.display.scroll_left;
    let pad = ed.metrics.padding_h;
    let mut rects: Vec<(f64, f64, f64, f64)> = Vec::new();
    for line in a.line..=b.line {
        let len = ed.line_text(line).len() as i64;
        let s_ch = if line == a.line { a.ch } else { 0 };
        let e_ch = if line == b.line { b.ch } else { len };
        let lc = ed.cursor_coords(
            Pos::with_sticky(line, s_ch, Some(Sticky::After)),
            CoordSys::Div,
        );
        let rc = ed.cursor_coords(
            Pos::with_sticky(line, e_ch, Some(Sticky::Before)),
            CoordSys::Div,
        );
        let end = if line != b.line { right } else { rc.left };
        if lc.top == rc.top {
            rects.push((lc.left, lc.top, end, lc.bottom));
        } else {
            rects.push((lc.left, lc.top, right, lc.bottom));
            if rc.top > lc.bottom {
                rects.push((pad, lc.bottom, right, rc.top));
            }
            rects.push((pad, rc.top, end, rc.bottom));
        }
    }
    // To pixels, merging vertically adjacent rectangles with the same x span (the
    // measurement cannot tell them apart).
    let mut out: Vec<(i64, i64, i64, i64)> = Vec::new();
    for (x0, y0, x1, y1) in rects {
        let pr = px_rect(s.wx(x0), s.wy(y0), s.wx(x1), s.wy(y1), s.w, s.h);
        if pr.0 > pr.1 || pr.2 > pr.3 {
            continue;
        }
        match out.last_mut() {
            Some(q) if q.0 == pr.0 && q.1 == pr.1 && q.3 + 1 == pr.2 => q.3 = pr.3,
            _ => out.push(pr),
        }
    }
    out
}

fn check_selection(gpu: &Gpu, s: &mut Scene, shown: &Image, r: &mut Report) {
    let want = expected_selection(s);
    let head = s.editor.doc.selection().ranges[0].head;
    s.editor
        .doc
        .set_selection(head, Some(head), sel_dont_scroll());
    let base = s.frame(gpu, false);
    let got = diff_regions(shown, &base);
    if got.len() != want.len() {
        r.fail(format!(
            "selection: measured rectangles {got:?}, model {want:?}"
        ));
        return;
    }
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        r.rect(&format!("selection rect {i}"), *g, *w);
    }
    // The fill color: rows inside the rectangle that hold no glyph ink.
    let color = theme_px(s.renderer.theme.selection_focused);
    let (x0, x1, y0, y1) = got[0];
    let n = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .filter(|&(x, y)| shown.at(x as u32, y as u32) == color)
        .count();
    let total = ((x1 - x0 + 1) * (y1 - y0 + 1)) as usize;
    r.note(format!(
        "selection color {}: {n}/{total} pixels of rect 0",
        hex(color)
    ));
    if n * 2 < total {
        r.fail(format!(
            "selection rect 0: only {n}/{total} pixels are the theme selection color {}",
            hex(color)
        ));
    }
}

fn check_gutter(s: &Scene, img: &Image, r: &mut Report) {
    let g = s.editor.gutter_width;
    let theme = &s.renderer.theme;
    // Row 0 is in the top padding: no line-number text there.
    let fill = theme_px(theme.gutter);
    let border = theme_px(theme.gutter_border);
    let first_not = |x0: u32, c: [u8; 4]| (x0..img.w).find(|&x| img.at(x, 0) != c);
    let fill_end = first_not(0, fill).map(|x| x as i64 - 1).unwrap_or(-1);
    let border_end = first_not((fill_end + 1) as u32, border)
        .map(|x| x as i64 - 1)
        .unwrap_or(-1);
    let (f0, f1) = span(0.0, g - 1.0);
    let (b0, b1) = span(g - 1.0, g);
    r.note(format!("gutter_width {g} (model)"));
    let full_h = (0, img.h as i64 - 1);
    let col_fill = (0..img.h).all(|y| img.at(0, y) == fill);
    if !col_fill {
        r.fail("gutter fill does not cover column 0 top to bottom");
    }
    r.rect(
        "gutter fill",
        (0, fill_end, full_h.0, full_h.1),
        (f0, f1, full_h.0, full_h.1),
    );
    r.rect(
        "gutter border",
        (fill_end + 1, border_end, full_h.0, full_h.1),
        (b0, b1, full_h.0, full_h.1),
    );
}

fn check_scrollbar(s: &Scene, img: &Image, r: &mut Report) {
    let ed = &s.editor;
    let (w, h) = (s.w as f64, s.h as f64);
    let doc_h = ed.doc_height() + 2.0 * ed.metrics.padding_top;
    let st = ed.display.scroll_top;
    r.note(format!(
        "scrollbar model: doc height {doc_h}, viewport {h}, scroll_top {st}"
    ));
    if doc_h <= h {
        r.fail("scrollbar case: document fits, no thumb expected");
        return;
    }
    // Proportional thumb: length h*h/docH (min 20), offset scrollTop/(docH-h) of
    // the free track; 6px wide, 2px from the right edge.
    let bar_h = (h * h / doc_h).max(20.0);
    let y = st / (doc_h - h) * (h - bar_h);
    let want = px_rect(w - 8.0, y, w - 2.0, y + bar_h, s.w, s.h);
    let bg = theme_px(s.renderer.theme.background);
    let col = s.w - 5;
    let ys: Vec<i64> = (0..s.h)
        .filter(|&yy| img.at(col, yy) != bg)
        .map(i64::from)
        .collect();
    let mid = ((y + bar_h / 2.0) as u32).min(s.h - 1);
    let xs: Vec<i64> = (s.w - 16..s.w)
        .filter(|&x| img.at(x, mid) != bg)
        .map(i64::from)
        .collect();
    let (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) =
        (ys.first(), ys.last(), xs.first(), xs.last())
    else {
        r.fail("scrollbar thumb not found");
        return;
    };
    r.rect("scrollbar thumb", (x0, x1, y0, y1), want);
    let thumb = img.at(col, mid);
    r.note(format!("scrollbar thumb color {}", hex(thumb)));
}

fn check_tokens(c: &Case, s: &Scene, img: &Image, r: &mut Report) {
    for &(line, from, to, style, color) in &c.tokens {
        let got =
            s.editor
                .get_token_type_at(Pos::with_sticky(line, from + 1, Some(Sticky::Before)));
        if got.as_deref() != Some(style) {
            r.fail(format!(
                "token {line}:{from}..{to}: mode style {got:?}, expected {style:?}"
            ));
            continue;
        }
        let a = s.editor.cursor_coords(Pos::new(line, from), CoordSys::Div);
        let b = s.editor.cursor_coords(Pos::new(line, to), CoordSys::Div);
        let (x0, x1, y0, y1) = px_rect(
            s.wx(a.left),
            s.wy(a.top),
            s.wx(b.left),
            s.wy(a.bottom),
            s.w,
            s.h,
        );
        let want = [(color >> 16) as u8, (color >> 8) as u8, color as u8, 255];
        let bg = theme_px(s.renderer.theme.background);
        // The most-inked pixel of the token. Glyph coverage blends the token color
        // over the background in linear light, so it must equal
        // `encode((1 - a) * bg + a * color)` for one coverage `a` in all channels.
        let ink = (y0..=y1)
            .flat_map(|y| (x0..=x1).map(move |x| img.at(x as u32, y as u32)))
            .min_by_key(|p| p[0] as u32 + p[1] as u32 + p[2] as u32)
            .unwrap_or(bg);
        let (mut num, mut den) = (0.0, 0.0);
        for i in 0..3 {
            let (pb, pc) = (lin(bg[i]), lin(want[i]));
            num += (lin(ink[i]) - pb) * (pc - pb);
            den += (pc - pb) * (pc - pb);
        }
        let a = if den > 0.0 {
            (num / den).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let err = (0..3)
            .map(|i| enc((1.0 - a) * lin(bg[i]) + a * lin(want[i])).abs_diff(ink[i]))
            .max()
            .unwrap();
        let line_s = format!(
            "token {line}:{from}..{to} {style} #{color:06x}: most-inked pixel {} = token color at coverage {a:.3}, within {err}",
            hex(ink)
        );
        if a < 0.5 || err > CHANNEL_TOLERANCE {
            r.fail(line_s);
        } else {
            r.note(format!("ok {line_s}"));
        }
    }
}

/// Compare with the golden of the reviewed set for this adapter; writes a diff image
/// on mismatch. With no reviewed set the comparison is recorded as unverified.
fn check_golden(name: &str, desc: &str, img: &Image, r: &mut Report) {
    if blessing() {
        let cand = candidate_dir().join(format!("{name}.png"));
        write_png(&cand, img.w, img.h, &img.px).expect("write candidate");
        r.note(format!("candidate (unreviewed) {}", cand.display()));
    }
    let set = match golden_set(desc) {
        Ok(Some(dir)) => dir,
        Ok(None) => {
            let why = format!(
                "no reviewed golden set for adapter [{}]; pixel comparison of {name} UNVERIFIED",
                identity(desc).join("; ")
            );
            r.note(&why);
            r.unverified = Some(why);
            return;
        }
        Err(e) => {
            r.fail(e);
            return;
        }
    };
    let golden = set.join(format!("{name}.png"));
    let g = match Image::read(&golden) {
        Ok(g) => g,
        Err(e) => {
            r.fail(format!(
                "golden {e}; write candidates with CM_VISUAL_BLESS=1 and review them into {}",
                set.display()
            ));
            return;
        }
    };
    if (g.w, g.h) != (img.w, img.h) {
        r.fail(format!(
            "golden {} is {}x{}, frame is {}x{}",
            golden.display(),
            g.w,
            g.h,
            img.w,
            img.h
        ));
        return;
    }
    let mut diff = Vec::with_capacity(img.px.len());
    let (mut n, mut max_delta) = (0usize, 0u8);
    for (a, b) in img.px.chunks(4).zip(g.px.chunks(4)) {
        let d = (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap();
        max_delta = max_delta.max(d);
        if d > CHANNEL_TOLERANCE {
            n += 1;
            diff.extend_from_slice(&[255, 0, 0, 255]);
        } else {
            let l = 192 + a[1] / 4;
            diff.extend_from_slice(&[l, l, l, 255]);
        }
    }
    let total = (img.w * img.h) as usize;
    let frac = n as f64 / total as f64;
    let line = format!(
        "golden {name}.png: {n}/{total} pixels beyond channel delta {CHANNEL_TOLERANCE} (fraction {frac:.6}, limit {MAX_DIFF_FRACTION}), max channel delta {max_delta}"
    );
    if frac > MAX_DIFF_FRACTION {
        let dp = out_dir().join(format!("{name}.diff.png"));
        write_png(&dp, img.w, img.h, &diff).expect("write diff");
        r.fail(format!(
            "{line}; actual {}, diff {}, golden adapter: {}",
            out_dir().join(format!("{name}.png")).display(),
            dp.display(),
            std::fs::read_to_string(set.join("ADAPTER.txt"))
                .unwrap_or_default()
                .replace('\n', "; ")
        ));
    } else {
        r.note(format!("ok {line}"));
    }
}

fn run(name: &str) {
    let (gpu, desc) = match gpu() {
        Ok((g, d)) => (g, d),
        Err(e) => return cm_evidence::no_gpu(e),
    };
    let c = case(name);
    let mut r = Report::default();
    let mut s = Scene::new(gpu, &c);
    r.note(format!(
        "case {}: scale {} physical {}x{} font {}px line {}px char_width {:.4} mode {} wrap {} line_numbers {}",
        c.name,
        c.scale,
        s.w,
        s.h,
        s.renderer.font_size,
        s.renderer.line_height,
        s.editor.metrics.char_width,
        s.editor.mode_name(),
        c.wrap,
        c.line_numbers
    ));
    let img = s.frame(gpu, true);
    let actual = out_dir().join(format!("{name}.png"));
    write_png(&actual, img.w, img.h, &img.px).expect("write frame");
    r.note(format!("frame {} sha256 {}", actual.display(), {
        sha256_hex(&std::fs::read(&actual).unwrap())
    }));

    // Determinism within the process: a second frame of the same scene is identical.
    let again = s.frame(gpu, true);
    if again.px != img.px {
        r.fail("a second render of the same scene differs");
    }
    check_golden(name, desc, &img, &mut r);
    check_tokens(&c, &s, &img, &mut r);
    if c.line_numbers {
        check_gutter(&s, &img, &mut r);
    }
    if c.scroll_line.is_some() {
        check_scrollbar(&s, &img, &mut r);
    }
    if c.anchor == c.head {
        check_cursor(gpu, &mut s, &img, &mut r);
    } else {
        check_selection(gpu, &mut s, &img, &mut r);
    }
    let verdict = match (r.failures.is_empty(), &r.unverified) {
        (false, _) => "FAIL",
        (true, Some(_)) => "UNVERIFIED (geometry PASS)",
        (true, None) => "PASS",
    };
    r.note(format!(
        "{verdict} {name} (max geometry delta {}px)",
        r.max_geometry_delta
    ));
    std::fs::write(out_dir().join(format!("{name}.txt")), &r.log).expect("write record");
    println!("{}", r.log);
    assert!(
        r.failures.is_empty(),
        "visual case {name} failed:\n{}",
        r.failures.join("\n")
    );
    if let Some(why) = r.unverified {
        cm_evidence::golden_unverified(why);
    }
}

#[test]
fn font_checksum_matches() {
    let data = font_bytes();
    assert_eq!(sha256_hex(data), FONT_SHA256);
}

#[test]
fn plain_text() {
    run("plain_text");
}
#[test]
fn js_highlight() {
    run("js_highlight");
}
#[test]
fn js_highlight_2x() {
    run("js_highlight_2x");
}
#[test]
fn selection_single_line() {
    run("selection_single_line");
}
#[test]
fn selection_multiline() {
    run("selection_multiline");
}
#[test]
fn selection_multiline_2x() {
    run("selection_multiline_2x");
}
#[test]
fn cursor_position() {
    run("cursor_position");
}
#[test]
fn cursor_position_2x() {
    run("cursor_position_2x");
}
#[test]
fn line_numbers() {
    run("line_numbers");
}
#[test]
fn line_numbers_2x() {
    run("line_numbers_2x");
}
#[test]
fn scrollbar() {
    run("scrollbar");
}
#[test]
fn wrapped_line() {
    run("wrapped_line");
}
#[test]
fn wrapped_selection() {
    run("wrapped_selection");
}
#[test]
fn wrapped_selection_2x() {
    run("wrapped_selection_2x");
}

// -------------------------------------------------------------------------------
// Horizontal scrolling with line numbers (review finding M1, 2026-10-26)
//
// The gutter (`.CodeMirror-gutters`, z-index 3, after the sizer in the DOM) covers
// the cursor layer (z-index 3). A cursor scrolled behind it must leave every gutter
// pixel unchanged, and the part to the right of the gutter edge stays visible and
// above the text. These cases need no golden: the expectations are geometric.

/// Gutter and cursor variants of the horizontal-scroll cases.
#[derive(Clone, Copy, Debug)]
struct HMode {
    line_numbers: bool,
    /// `cm.state.overwrite`: the renderer's wider overwrite cursor (`cw / 4`, at least
    /// 2 px, not rounded).
    overwrite: bool,
}

const NUMBERS: HMode = HMode {
    line_numbers: true,
    overwrite: false,
};

/// 200-column lines with glyph ink everywhere, the cursor at `1:ch`, scrolled
/// horizontally so the cursor's left edge is at window x `gutter + offset`.
fn hscroll_scene(gpu: &Gpu, scale: f32, ch: i64, offset: f64, mode: HMode) -> Scene {
    let text: String = (0..30)
        .map(|i| format!("{i:02} {}\n", "abcdefghij".repeat(20)))
        .collect();
    let c = Case {
        line_numbers: mode.line_numbers,
        ..Case::new("hscroll", &text, p(1, ch)).scale(scale)
    };
    let mut s = Scene::new(gpu, &c);
    s.editor.overwrite = mode.overwrite;
    let left = s.editor.cursor_coords(p(1, ch), CoordSys::Div).left;
    s.editor.scroll_to(Some(left - offset), None);
    assert_eq!(
        s.editor.display.scroll_left,
        left - offset,
        "scroll_left was clamped"
    );
    s
}

/// Renders the scene with and without the cursor and checks the gutter and the
/// visible part of the cursor. Returns the visible cursor columns and the cursor width.
fn check_hscroll(
    gpu: &Gpu,
    scale: f32,
    ch: i64,
    offset: f64,
    mode: HMode,
    r: &mut Report,
) -> (i64, i64) {
    let mut s = hscroll_scene(gpu, scale, ch, offset, mode);
    let shown = s.frame(gpu, true);
    let hidden = s.frame(gpu, false);
    let g = s.editor.gutter_width;
    let c = s.editor.cursor_coords(p(1, ch), CoordSys::Div);
    let cw = if mode.overwrite {
        (s.editor.metrics.char_width as f32 / 4.0).max(2.0) as f64
    } else {
        (s.editor.metrics.char_width / 8.0).max(1.0).round()
    };
    let x = s.wx(c.left);
    r.note(format!(
        "{mode:?} scale {scale} ch {ch} offset {offset}: gutter_width {g}, cursor window x {x:.3}, width {cw}, scroll_left {:.3}",
        s.editor.display.scroll_left
    ));
    if mode.line_numbers {
        check_gutter(&s, &shown, r);
    }
    let in_gutter = (0..s.h)
        .flat_map(|y| (0..g as u32).map(move |x| (x, y)))
        .filter(|&(x, y)| shown.at(x, y) != hidden.at(x, y))
        .count();
    if in_gutter != 0 {
        r.fail(format!(
            "scale {scale} offset {offset}: {in_gutter} gutter pixels change when the cursor is shown"
        ));
    } else {
        r.note("ok 0 gutter pixels change when the cursor is shown");
    }
    // The visible part: the cursor quad clipped at the gutter edge.
    let (x0, x1) = (x.max(g), (x + cw).max(g));
    let want = px_rect(x0, s.wy(c.top), x1, s.wy(c.bottom), s.w, s.h);
    let regions = diff_regions(&shown, &hidden);
    let bbox = regions
        .iter()
        .copied()
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)));
    let cols = (want.1 - want.0 + 1).max(0);
    let out = (cols, cw.round() as i64);
    match bbox {
        None if cols == 0 => r.note("ok cursor fully behind the gutter: no pixel changes"),
        None => r.fail(format!(
            "scale {scale} offset {offset}: cursor expected at {want:?}, nothing drawn"
        )),
        Some(b) if cols == 0 => r.fail(format!(
            "scale {scale} offset {offset}: cursor should be hidden, changed {b:?}"
        )),
        Some(b) => {
            // Exact: no case puts an edge on a pixel center.
            if b != want {
                r.fail(format!(
                    "scale {scale} offset {offset}: cursor measured {b:?}, clipped model {want:?}"
                ));
            } else {
                r.note(format!("ok cursor measured {b:?} = clipped model"));
            }
            // Above the text: every pixel of the visible part is the cursor color.
            let color = theme_px(s.renderer.theme.cursor);
            for y in b.2..=b.3 {
                for xx in b.0..=b.1 {
                    let px = shown.at(xx as u32, y as u32);
                    if px != color {
                        r.fail(format!("cursor pixel ({xx},{y}) is {}", hex(px)));
                        return out;
                    }
                }
            }
        }
    }
    out
}

fn run_hscroll(scale: f32) {
    let gpu = match gpu() {
        Ok((g, _)) => g,
        Err(e) => return cm_evidence::no_gpu(e),
    };
    let mut r = Report::default();
    // Behind the gutter (left edge 12 and 2 logical px inside it).
    let deep = check_hscroll(gpu, scale, 40, -12.0 * scale as f64, NUMBERS, &mut r).0;
    let near = check_hscroll(gpu, scale, 40, -2.0 * scale as f64, NUMBERS, &mut r).0;
    // Straddling the edge: one physical pixel left of it. At scale 1 the 1 px
    // cursor is hidden; at scale 2 the right one of its 2 px stays visible.
    let straddle = check_hscroll(gpu, scale, 40, -1.0, NUMBERS, &mut r).0;
    // Exactly at the edge, and well inside the text area: fully visible.
    let at_edge = check_hscroll(gpu, scale, 40, 0.0, NUMBERS, &mut r).0;
    let (inside, cw) = check_hscroll(gpu, scale, 40, 40.0 * scale as f64, NUMBERS, &mut r);
    r.note(format!(
        "visible columns: behind {deep}/{near}, straddle {straddle}, at edge {at_edge}, inside {inside} (cursor width {cw})"
    ));
    if (deep, near, straddle, at_edge, inside) != (0, 0, cw - 1, cw, cw) {
        r.fail(format!(
            "visible columns {deep}/{near}/{straddle}/{at_edge}/{inside}, expected 0/0/{}/{cw}/{cw}",
            cw - 1
        ));
    }
    let name = format!("hscroll_gutter_{scale}x");
    std::fs::write(out_dir().join(format!("{name}.txt")), &r.log).expect("write record");
    println!("{}", r.log);
    assert!(
        r.failures.is_empty(),
        "{name} failed:\n{}",
        r.failures.join("\n")
    );
}

#[test]
fn hscroll_gutter_clips_cursor() {
    run_hscroll(1.0);
}
#[test]
fn hscroll_gutter_clips_cursor_2x() {
    run_hscroll(2.0);
}

/// The golden-set rule (no adapter is used): a set matches only the exact
/// seven-field identity of its `ADAPTER.txt`, and a set without `REVIEWED.txt` is
/// not a set.
#[test]
fn golden_set_requires_exact_adapter_identity() {
    let recorded = std::fs::read_to_string(golden_dir().join("ADAPTER.txt")).unwrap();
    assert_eq!(identity(&recorded).len(), 7, "ADAPTER.txt has seven fields");
    assert_eq!(golden_set(&recorded), Ok(Some(golden_dir())));
    for (i, line) in identity(&recorded).iter().enumerate() {
        let changed: Vec<String> = identity(&recorded)
            .iter()
            .enumerate()
            .map(|(j, l)| {
                if j == i {
                    format!("{l}-other")
                } else {
                    l.to_string()
                }
            })
            .collect();
        assert_eq!(
            golden_set(&changed.join("\n")),
            Ok(None),
            "a different `{line}` must not match"
        );
    }
    assert!(
        !std::fs::read_to_string(golden_dir().join("REVIEWED.txt"))
            .unwrap()
            .trim()
            .is_empty()
    );
}

/// The vertical scrollbar (`.CodeMirror-vscrollbar`, z-index 6) is above the cursor
/// layer (z-index 3): a cursor under the translucent thumb is seen through it. With
/// a black cursor both stacking orders give black, so the cursor is red here.
#[test]
fn scrollbar_thumb_covers_cursor() {
    let gpu = match gpu() {
        Ok((g, _)) => g,
        Err(e) => return cm_evidence::no_gpu(e),
    };
    let text: String = (0..60)
        .map(|i| format!("{i:02} {}\n", "abcdefghij".repeat(20)))
        .collect();
    let mut s = Scene::new(gpu, &Case::new("thumb", &text, p(1, 80)));
    let red = [1.0, 0.0, 0.0, 1.0];
    s.renderer.theme.cursor = red;
    // Cursor left edge at window x w - 5, inside the thumb's [w - 8, w - 2).
    let left = s.editor.cursor_coords(p(1, 80), CoordSys::Div).left;
    let target = s.w as f64 - 5.0;
    s.editor
        .scroll_to(Some(left + s.editor.gutter_width - target), None);
    let c = s.editor.cursor_coords(p(1, 80), CoordSys::Div);
    let x = s.wx(c.left);
    assert!(
        (x - target).abs() < 1e-6,
        "cursor at window x {x}, wanted {target}"
    );
    let shown = s.frame(gpu, true);
    let hidden = s.frame(gpu, false);
    let (px, py) = (s.w - 5, s.wy(c.top + 2.0) as u32);
    let thumb = s.renderer.theme.scrollbar;
    // The thumb blended over the red cursor, in linear light (sRGB target).
    let want: Vec<u8> = (0..3)
        .map(|i| enc(thumb[3] as f64 * thumb[i] as f64 + (1.0 - thumb[3] as f64) * red[i] as f64))
        .collect();
    let got = shown.at(px, py);
    let behind = hidden.at(px, py);
    println!(
        "cursor x {x:.3}; pixel ({px},{py}) shown {} hidden {} want {:?}",
        hex(got),
        hex(behind),
        want
    );
    assert_ne!(got, behind, "the cursor is not drawn under the thumb");
    for i in 0..3 {
        assert!(
            got[i].abs_diff(want[i]) <= CHANNEL_TOLERANCE,
            "pixel {} is not the thumb over the cursor {want:?}",
            hex(got)
        );
    }
}

/// The other variants: no gutter (the clip edge is the window edge), the overwrite
/// cursor (fractional width), and edges 0.3 px off a pixel boundary on both sides of
/// the clip edge. `check_hscroll` compares each with the clipped model rectangle.
/// Wrapped lines are not a variant: with `lineWrapping` the view never scrolls
/// horizontally, so no cursor reaches the gutter.
fn run_hscroll_variants(scale: f32) {
    let gpu = match gpu() {
        Ok((g, _)) => g,
        Err(e) => return cm_evidence::no_gpu(e),
    };
    let mut r = Report::default();
    let modes = [
        NUMBERS,
        HMode {
            line_numbers: true,
            overwrite: true,
        },
        HMode {
            line_numbers: false,
            overwrite: false,
        },
        HMode {
            line_numbers: false,
            overwrite: true,
        },
    ];
    for mode in modes {
        let behind = check_hscroll(gpu, scale, 40, -12.0 * scale as f64, mode, &mut r).0;
        let mut seen = Vec::new();
        for offset in [-1.0, -0.3, 0.3, 40.0 * scale as f64] {
            seen.push(check_hscroll(gpu, scale, 40, offset, mode, &mut r).0);
        }
        let inside = *seen.last().unwrap();
        r.note(format!(
            "{mode:?}: visible columns behind {behind}, at -1/-0.3/+0.3/inside {seen:?}"
        ));
        if behind != 0 || inside == 0 {
            r.fail(format!(
                "{mode:?}: behind {behind} (want 0), inside {inside} (want > 0)"
            ));
        }
    }
    let name = format!("hscroll_variants_{scale}x");
    std::fs::write(out_dir().join(format!("{name}.txt")), &r.log).expect("write record");
    println!("{}", r.log);
    assert!(
        r.failures.is_empty(),
        "{name} failed:\n{}",
        r.failures.join("\n")
    );
}

#[test]
fn hscroll_variants_clip_cursor() {
    run_hscroll_variants(1.0);
}
#[test]
fn hscroll_variants_clip_cursor_2x() {
    run_hscroll_variants(2.0);
}

/// The golden-set rule on scratch directories: a candidate without `REVIEWED.txt` (or
/// with an empty one) is never used, a reviewed subdirectory set is found, and two
/// reviewed sets for the same adapter are an error.
#[test]
fn golden_set_rule_on_scratch_sets() {
    let root = out_dir().join("golden-set-rule");
    let _ = std::fs::remove_dir_all(&root);
    let desc = "adapter: Test GPU\nbackend: Metal\ndevice_type: IntegratedGpu\nvendor: 0x0\ndevice: 0x0\ndriver: test\ndriver_info: 1\n";
    let set = |name: &str, reviewed: Option<&str>| {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("ADAPTER.txt"), desc).unwrap();
        if let Some(r) = reviewed {
            std::fs::write(d.join("REVIEWED.txt"), r).unwrap();
        }
        d
    };
    std::fs::create_dir_all(&root).unwrap();
    assert_eq!(golden_set_in(&root, desc), Ok(None), "empty root");
    let a = set("metal-a", None);
    assert_eq!(golden_set_in(&root, desc), Ok(None), "no REVIEWED.txt");
    std::fs::write(a.join("REVIEWED.txt"), " \n").unwrap();
    assert_eq!(golden_set_in(&root, desc), Ok(None), "empty REVIEWED.txt");
    std::fs::write(a.join("REVIEWED.txt"), "reviewed by a person").unwrap();
    assert_eq!(golden_set_in(&root, desc), Ok(Some(a.clone())));
    let other = desc.replace("driver_info: 1", "driver_info: 2");
    assert_eq!(golden_set_in(&root, &other), Ok(None), "other driver");
    set("metal-b", Some("reviewed too"));
    assert!(
        golden_set_in(&root, desc).is_err(),
        "two sets must be an error"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// Line classes through `Theme::class_backgrounds`: `bgClass` and `wrapClass` lie under
/// the selection layer, `textClass` (on `pre.CodeMirror-line`, z-index 2) above it
/// (update_line.js:34 and :77; codemirror.css). Lines 1 and 2 are fully selected.
#[test]
fn line_class_backgrounds() {
    let gpu = match gpu() {
        Ok((g, _)) => g,
        Err(e) => return cm_evidence::no_gpu(e),
    };
    let mut s = Scene::new(
        gpu,
        &Case::new("line_classes", "zero\none\ntwo\nthree\nfour", p(3, 0)).select(p(1, 0)),
    );
    let (bg, tx, wr) = (
        [0.0, 0.5, 0.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
    );
    s.renderer.theme.class_backgrounds =
        vec![("bg".into(), bg), ("tx".into(), tx), ("wr".into(), wr)];
    use cm_core::LineWhere;
    for (line, place, cls) in [
        (1, LineWhere::Background, "bg"),
        (2, LineWhere::Text, "tx"),
        (3, LineWhere::Wrap, "wr"),
        (4, LineWhere::Background, "other"),
    ] {
        s.editor.doc.add_line_class(line, place, cls).unwrap();
    }
    let img = s.frame(gpu, false);
    let x = s.w - 40;
    let mid = |s: &Scene, line: i64| {
        s.wy(s.editor.height_at_line(line) + s.editor.line_height(line) / 2.0) as u32
    };
    let theme = s.renderer.theme.clone();
    let want = [
        (0, theme_px(theme.background)),
        (1, theme_px(theme.selection_focused)),
        (2, theme_px(tx)),
        (3, theme_px(wr)),
        (4, theme_px(theme.background)),
    ];
    for (line, c) in want {
        assert_eq!(
            hex(img.at(x, mid(&s, line))),
            hex(c),
            "line {line} at x {x}"
        );
    }
    // The bgClass shows where the selection does not reach: line 1 with nothing
    // selected.
    s.editor.doc.set_selection(p(0, 0), None, sel_dont_scroll());
    let img = s.frame(gpu, false);
    assert_eq!(hex(img.at(x, mid(&s, 1))), hex(theme_px(bg)), "line 1 bg");
    assert_eq!(hex(img.at(x, mid(&s, 2))), hex(theme_px(tx)), "line 2 text");
    // Nothing is drawn over the gutter area (no line numbers here: gutter 0).
    assert_eq!(s.editor.gutter_width, 0.0);
}
