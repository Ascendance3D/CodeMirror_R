//! The wgpu renderer: `src/display/update_display.js`, `update_line.js`,
//! `selection.js` and `line_numbers.js` turned into quads and glyphon text.
//!
//! Only the lines in `[viewFrom, viewTo)` that intersect the visible area are drawn
//! (CM5's viewport). Text is positioned by the editor's box model
//! ([`crate::measure`]), one glyphon buffer per run of same-styled cells, so what is
//! drawn and what `coordsChar`/`cursorCoords` compute always agree. Colors follow
//! the CM5 default theme (`lib/codemirror.css`, `.cm-s-default`).
//!
//! Not drawn: mode line classes (`Styles::classes`), marked-text styling
//! (`className`, `css`, `startStyle`/`endStyle`, `title`), `addLineClass` and gutter
//! markers (no API; the only gutter is line numbers), line/inline widgets and
//! `addWidget` (no API). Collapsed ranges are drawn as ordinary text. Text,
//! selections and cursors are laid out left to right (no bidi reordering).

use std::collections::HashMap;
use std::rc::Rc;

use glyphon::cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics as GMetrics, Shaping, SwashCache, Weight,
};
use glyphon::{Cache, Resolution, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport};
use wgpu::util::DeviceExt;

use crate::editor::{CoordSys, Editor};
use crate::highlight::Styles;
use crate::measure::{Glyph, LineLayout, Metrics, Rect};

/// The font the editor draws with.
pub const MONO_FAMILY: &str = "DejaVu Sans Mono";

/// An RGBA color in sRGB space, 0..1.
pub type Rgba = [f32; 4];

pub fn rgb(hex: u32) -> Rgba {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// Colors of the CM5 default theme.
#[derive(Clone, Debug)]
pub struct Theme {
    pub background: Rgba,
    pub text: Rgba,
    pub gutter: Rgba,
    pub gutter_border: Rgba,
    pub line_number: Rgba,
    pub selection: Rgba,
    pub selection_focused: Rgba,
    pub cursor: Rgba,
    pub scrollbar: Rgba,
    /// `background` of a host CSS class (the page's style sheet), for line
    /// `bgClass`/`gutterClass` properties: `(class, color)`, later entries win.
    pub class_backgrounds: Vec<(String, Rgba)>,
}

impl Theme {
    /// The background a class list gets from [`Theme::class_backgrounds`].
    pub fn class_background(&self, classes: &str) -> Option<Rgba> {
        self.class_backgrounds
            .iter()
            .rev()
            .find(|(k, _)| classes.split_whitespace().any(|c| c == k))
            .map(|(_, c)| *c)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            background: rgb(0xffffff),
            text: rgb(0x000000),
            gutter: rgb(0xf7f7f7),
            gutter_border: rgb(0xdddddd),
            line_number: rgb(0x999999),
            selection: rgb(0xd9d9d9),
            selection_focused: rgb(0xd7d4f0),
            cursor: rgb(0x000000),
            scrollbar: [0.0, 0.0, 0.0, 0.25],
            class_backgrounds: Vec::new(),
        }
    }
}

/// Token style → (color, bold), `.cm-s-default .cm-*` rules.
pub fn style_color(style: Option<&str>) -> (Option<u32>, bool) {
    let Some(style) = style else {
        return (None, false);
    };
    let mut color = None;
    let mut bold = false;
    for cls in style.split_whitespace() {
        let c = match cls {
            "header" => Some(0x0000ff),
            "quote" => Some(0x009900),
            "negative" => Some(0xdd4444),
            "positive" => Some(0x229922),
            "keyword" => Some(0x770088),
            "atom" => Some(0x221199),
            "number" => Some(0x116644),
            "def" => Some(0x0000ff),
            "variable-2" => Some(0x0055aa),
            "variable-3" | "type" => Some(0x008855),
            "comment" => Some(0xaa5500),
            "string" => Some(0xaa1111),
            "string-2" => Some(0xff5500),
            "meta" | "qualifier" => Some(0x555555),
            "builtin" => Some(0x3300aa),
            "bracket" => Some(0x999977),
            "tag" => Some(0x117700),
            "attribute" => Some(0x0000cc),
            "hr" => Some(0x999999),
            "link" => Some(0x0000cc),
            "error" | "invalidchar" => Some(0xff0000),
            _ => None,
        };
        if cls == "header" || cls == "strong" {
            bold = true;
        }
        if color.is_none() {
            color = c;
        }
    }
    (color, bold)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    color: [f32; 4],
}

const RECT_SHADER: &str = r#"
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VOut {
    var o: VOut; o.pos = vec4<f32>(pos, 0.0, 1.0); o.color = color; return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return i.color; }
"#;

/// A GPU device without a window (`request_adapter` with no surface).
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Headless device. Falls back to a software adapter when no GPU is found.
    pub fn headless() -> Result<Gpu, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        Gpu::with_instance(instance, None)
    }

    pub fn with_instance(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Gpu, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: true,
                compatible_surface: surface,
                apply_limit_buckets: false,
            }))
        })
        .map_err(|e| format!("no wgpu adapter: {e}"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| format!("request_device: {e}"))?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }
}

struct Seg {
    buffer: Buffer,
    x: f32,
    top: f32,
}

struct LineEntry {
    text: Vec<u16>,
    styles: Rc<Styles>,
    wrap: Option<f64>,
    segs: Vec<Seg>,
}

/// Draw state the app passes per frame.
#[derive(Clone, Copy, Debug)]
pub struct FrameState {
    pub focused: bool,
    pub cursor_visible: bool,
}

impl Default for FrameState {
    fn default() -> Self {
        FrameState {
            focused: true,
            cursor_visible: true,
        }
    }
}

/// The renderer: a rect pipeline and a glyphon text renderer for one target format.
pub struct Renderer {
    pub format: wgpu::TextureFormat,
    rect_pipeline: wgpu::RenderPipeline,
    pub font_system: FontSystem,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text: TextRenderer,
    pub font_size: f32,
    pub line_height: f32,
    lines: HashMap<usize, LineEntry>,
    numbers: HashMap<i64, Buffer>,
    pub theme: Theme,
}

impl Renderer {
    /// `font_size` and `line_height` in physical pixels. Fonts come from the system
    /// font database, plus the system DejaVu Sans Mono file when it exists.
    pub fn new(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        font_size: f32,
        line_height: f32,
    ) -> Renderer {
        let mut font_system = FontSystem::new();
        let mono = std::path::Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");
        if mono.exists() {
            let _ = font_system.db_mut().load_font_file(mono);
        }
        Renderer::with_font_system(gpu, format, font_size, line_height, font_system)
    }

    /// A renderer whose only font is `data` (one TrueType/OpenType file), for
    /// reproducible output: no system fonts are loaded and the locale is fixed to
    /// `en-US`. The file's family must be [`MONO_FAMILY`].
    pub fn with_font_data(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        font_size: f32,
        line_height: f32,
        data: Vec<u8>,
    ) -> Result<Renderer, String> {
        let mut db = glyphon::cosmic_text::fontdb::Database::new();
        db.load_font_data(data);
        let families: Vec<String> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(name, _)| name.clone()))
            .collect();
        if !families.iter().any(|f| f == MONO_FAMILY) {
            return Err(format!(
                "font data has families {families:?}, expected {MONO_FAMILY:?}"
            ));
        }
        let font_system = FontSystem::new_with_locale_and_db("en-US".into(), db);
        Ok(Renderer::with_font_system(
            gpu,
            format,
            font_size,
            line_height,
            font_system,
        ))
    }

    fn with_font_system(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        font_size: f32,
        line_height: f32,
        font_system: FontSystem,
    ) -> Renderer {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cm rects"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor::default());
        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cm rects"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });
        let swash = SwashCache::new();
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, &gpu.queue, &cache, format);
        let text = TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Renderer {
            format,
            rect_pipeline,
            font_system,
            swash,
            viewport,
            atlas,
            text,
            font_size,
            line_height,
            lines: HashMap::new(),
            numbers: HashMap::new(),
            theme: Theme::default(),
        }
    }

    /// Editor metrics for this font: the monospace advance measured by shaping.
    pub fn metrics(&mut self) -> Metrics {
        let mut b = Buffer::new(
            &mut self.font_system,
            GMetrics::new(self.font_size, self.line_height),
        );
        b.set_text(
            "xxxxxxxxxx",
            &Attrs::new().family(Family::Name(MONO_FAMILY)),
            Shaping::Advanced,
            None,
        );
        b.shape_until_scroll(&mut self.font_system, false);
        let w = b
            .layout_runs()
            .next()
            .map(|r| r.line_w)
            .unwrap_or(self.font_size * 6.0);
        let cw = (w / 10.0) as f64;
        let mut m = Metrics::monospace(cw, self.line_height as f64);
        m.padding_h = (4.0 * self.font_size as f64 / 14.0).round();
        m.padding_top = m.padding_h;
        m
    }

    fn make_buffer(&mut self, text: &str, color: Option<u32>, bold: bool, default: Rgba) -> Buffer {
        let mut b = Buffer::new(
            &mut self.font_system,
            GMetrics::new(self.font_size, self.line_height),
        );
        let c = match color {
            Some(h) => Color::rgb((h >> 16) as u8, (h >> 8) as u8, h as u8),
            None => Color::rgba(
                (default[0] * 255.0) as u8,
                (default[1] * 255.0) as u8,
                (default[2] * 255.0) as u8,
                255,
            ),
        };
        let mut attrs = Attrs::new().family(Family::Name(MONO_FAMILY)).color(c);
        if bold {
            attrs = attrs.weight(Weight::BOLD);
        }
        b.set_text(text, &attrs, Shaping::Advanced, None);
        b.shape_until_scroll(&mut self.font_system, false);
        b
    }

    /// Build the text segments of one line (`buildLineContent`): runs of same-style
    /// text cells on one visual row; tabs are skipped, special characters become a
    /// red bullet, wide and astral characters are placed one by one.
    fn build_line(&mut self, text: &[u16], layout: &LineLayout, styles: &Styles) -> Vec<Seg> {
        let mut segs = Vec::new();
        let n = text.len();
        let mut style_at = Vec::with_capacity(n);
        {
            let mut si = 0;
            for i in 0..n {
                while si < styles.spans.len() && styles.spans[si].0 <= i {
                    si += 1;
                }
                style_at.push(styles.spans.get(si).and_then(|s| s.1.clone()));
            }
        }
        let default = self.theme.text;
        let mut i = 0;
        while i < n {
            let g = layout.glyphs[i];
            match g {
                Glyph::Tab(_) => {
                    i += 1;
                    continue;
                }
                Glyph::Special => {
                    let b = self.make_buffer("\u{2022}", Some(0xff0000), false, default);
                    segs.push(Seg {
                        buffer: b,
                        x: layout.boxes[i].left as f32,
                        top: layout.boxes[i].top as f32,
                    });
                    i += 1;
                    continue;
                }
                _ => {}
            }
            let start = i;
            let single = matches!(g, Glyph::Astral | Glyph::Wide | Glyph::Extend);
            i += 1;
            // Following extending units belong to this cluster.
            while i < n && layout.glyphs[i] == Glyph::Extend {
                i += 1;
            }
            if !single {
                while i < n
                    && layout.glyphs[i] == Glyph::Text
                    && style_at[i] == style_at[start]
                    && layout.rows[i] == layout.rows[start]
                {
                    i += 1;
                    while i < n && layout.glyphs[i] == Glyph::Extend {
                        i += 1;
                    }
                }
            }
            let s = String::from_utf16_lossy(&text[start..i]);
            let (color, bold) = style_color(style_at[start].as_deref());
            let b = self.make_buffer(&s, color, bold, default);
            segs.push(Seg {
                buffer: b,
                x: layout.boxes[start].left as f32,
                top: layout.boxes[start].top as f32,
            });
        }
        segs
    }

    /// Render the editor into `view` (`width` x `height` physical pixels).
    pub fn render(
        &mut self,
        gpu: &Gpu,
        editor: &mut Editor,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        frame: FrameState,
    ) {
        let device = &gpu.device;
        let queue = &gpu.queue;
        let w = width as f32;
        let h = height as f32;
        let theme = self.theme.clone();
        let m = editor.metrics.clone();
        let gutter = editor.gutter_width as f32;
        let st = editor.display.scroll_top as f32;
        let sl = editor.display.scroll_left as f32;
        let pad_top = m.padding_top as f32;
        // Window y of a "div" y.
        let wy = |y: f64| y as f32 + pad_top - st;
        let wx = |x: f64| x as f32 + gutter - sl;

        let mut quads: Vec<(Rect, Rgba)> = Vec::new();
        let full = |x0: f32, y0: f32, x1: f32, y1: f32| Rect {
            left: x0 as f64,
            top: y0 as f64,
            right: x1 as f64,
            bottom: y1 as f64,
        };

        // Visible lines (the drawn part of the viewport).
        let (vis_from, vis_to) = editor.visible_lines();
        let first = editor.first_line();
        let end = first + editor.doc.line_count() as i64;
        let (from, to) = (vis_from.max(first), (vis_to + 1).min(end));

        // Line classes through the theme's class backgrounds. The wrapper (`wrapClass`)
        // and `div.CodeMirror-linebackground` (mode `line-background-*` + `bgClass`,
        // update_line.js:34) cover the whole line view, widgets included, below the
        // selection layer; `pre.CodeMirror-line` (mode `line-*` + `textClass`,
        // update_line.js:77) covers the text rows above it (z-index 2).
        let line_left = gutter - sl;
        let line_right = (editor.display.width as f32).max(gutter);
        let mut text_class_quads: Vec<(Rect, Rgba)> = Vec::new();
        for line in from..to {
            let Some(handle) = editor.doc.get_line_handle(line) else {
                continue;
            };
            if editor.doc.line_is_hidden(line) {
                continue;
            }
            let styles = editor.line_styles(line);
            let (mode_bg, mode_text) = styles.classes.clone().unwrap_or_default();
            let join = |a: Option<String>, b: Option<String>| match (a, b) {
                (Some(a), Some(b)) => Some(format!("{a} {b}")),
                (a, b) => a.or(b),
            };
            let top = wy(editor.height_at_line(line));
            let bottom = top + editor.line_height(line) as f32;
            for cls in [handle.wrap_class(), join(mode_bg, handle.bg_class())]
                .into_iter()
                .flatten()
            {
                if let Some(c) = theme.class_background(&cls) {
                    quads.push((full(line_left, top, line_right, bottom), c));
                }
            }
            if let Some(c) =
                join(mode_text, handle.text_class()).and_then(|cls| theme.class_background(&cls))
            {
                let t = wy(editor.text_top(line));
                let b = t + editor.text_rows_height(line) as f32;
                text_class_quads.push((full(line_left, t, line_right, b), c));
            }
        }

        // Selections (`drawSelectionRange`) and cursors (`drawSelectionCursor`).
        let sel = editor.doc.selection();
        let sel_color = if frame.focused {
            theme.selection_focused
        } else {
            theme.selection
        };
        let text_right = (editor.display.width as f32).max(gutter);
        for r in &sel.ranges {
            if r.empty() {
                continue;
            }
            let (a, b) = (r.from(), r.to());
            for line in a.line.max(from)..=b.line.min(to - 1) {
                let (t, l) = editor.layout(line);
                let top = editor.text_top(line);
                let s_ch = if line == a.line { a.ch } else { 0 };
                let e_ch = if line == b.line { b.ch } else { t.len() as i64 };
                let left_c = l.cursor_coords(&t, s_ch, Some(cm_core::Sticky::After));
                let right_c = l.cursor_coords(&t, e_ch, Some(cm_core::Sticky::Before));
                let extend_right = line != b.line;
                if left_c.top == right_c.top {
                    let x1 = if extend_right {
                        text_right
                    } else {
                        wx(right_c.left)
                    };
                    quads.push((
                        full(
                            wx(left_c.left),
                            wy(top + left_c.top),
                            x1,
                            wy(top + left_c.bottom),
                        ),
                        sel_color,
                    ));
                } else {
                    // Wrapped: first row to the right edge, middle rows full, last row to `to`.
                    quads.push((
                        full(
                            wx(left_c.left),
                            wy(top + left_c.top),
                            text_right,
                            wy(top + left_c.bottom),
                        ),
                        sel_color,
                    ));
                    if right_c.top > left_c.bottom {
                        quads.push((
                            full(
                                wx(m.padding_h),
                                wy(top + left_c.bottom),
                                text_right,
                                wy(top + right_c.top),
                            ),
                            sel_color,
                        ));
                    }
                    let x1 = if extend_right {
                        text_right
                    } else {
                        wx(right_c.left)
                    };
                    quads.push((
                        full(
                            wx(m.padding_h),
                            wy(top + right_c.top),
                            x1,
                            wy(top + right_c.bottom),
                        ),
                        sel_color,
                    ));
                }
            }
        }
        quads.extend(text_class_quads);
        // Cursors are drawn above the text (`div.CodeMirror-cursors { z-index: 3 }`),
        // but below the gutters, which also have z-index 3 and come later in the DOM
        // (`d.scroller` = [sizer, heightForcer, gutters]). The gutter is filled before
        // the text, so a cursor scrolled behind it is clipped to the text area here.
        let mut cursors: Vec<(Rect, Rgba)> = Vec::new();
        if frame.cursor_visible {
            for r in &sel.ranges {
                // `showCursorWhenSelecting` is false by default: cursors for empty ranges.
                if !r.empty() {
                    continue;
                }
                if r.head.line < from || r.head.line >= to {
                    continue;
                }
                let c = editor.cursor_coords(r.head, CoordSys::Div);
                let cw = if editor.overwrite {
                    (m.char_width as f32 / 4.0).max(2.0)
                } else {
                    (m.char_width as f32 / 8.0).max(1.0).round()
                };
                let (x0, x1) = (wx(c.left).max(gutter), (wx(c.left) + cw).max(gutter));
                if x1 > x0 {
                    cursors.push((full(x0, wy(c.top), x1, wy(c.bottom)), theme.cursor));
                }
            }
        }

        // Text.
        let mut keep: HashMap<usize, LineEntry> = HashMap::new();
        let wrap = if editor.options.line_wrapping {
            Some(editor.wrap_width())
        } else {
            None
        };
        let mut placed: Vec<(usize, f32)> = Vec::new();
        for line in from..to {
            let handle = editor.doc.get_line_handle(line).unwrap();
            let id = handle.id();
            let t = handle.text_utf16();
            let styles = editor.line_styles(line);
            let reuse = match self.lines.remove(&id) {
                Some(e) if e.text == t && Rc::ptr_eq(&e.styles, &styles) && e.wrap == wrap => {
                    Some(e)
                }
                _ => None,
            };
            let entry = match reuse {
                Some(e) => e,
                None => {
                    let l = editor.layout_for(line, &t);
                    let segs = self.build_line(&t, &l, &styles);
                    LineEntry {
                        text: t,
                        styles,
                        wrap,
                        segs,
                    }
                }
            };
            placed.push((id, wy(editor.text_top(line))));
            keep.insert(id, entry);
        }
        self.lines = keep;

        // Line numbers (`lineNumberFor`).
        let mut numbers_keep: HashMap<i64, Buffer> = HashMap::new();
        let mut number_pos: Vec<(i64, f32, f32)> = Vec::new();
        let mut marker_pos: Vec<(String, f32, f32)> = Vec::new();
        if !editor.gutter_columns.is_empty() {
            quads.push((full(0.0, 0.0, gutter - 1.0, h), theme.gutter));
            quads.push((full(gutter - 1.0, 0.0, gutter, h), theme.gutter_border));
            // updateLineGutter (update_line.js:87): a `gutterClass` line property
            // fills the whole gutter behind the line (CodeMirror-gutter-background).
            for line in from..to {
                // The gutter background spans the whole line wrapper, widgets
                // included; markers sit beside the text.
                let wrap_top = wy(editor.height_at_line(line));
                let y = wy(editor.text_top(line));
                let lh = editor.line_height(line) as f32;
                let handle = editor.doc.get_line_handle(line);
                if let Some(cls) = handle.as_ref().and_then(|l| l.gutter_class()) {
                    if let Some(c) = theme.class_background(&cls) {
                        quads.push((full(0.0, wrap_top, gutter - 1.0, wrap_top + lh), c));
                    }
                }
                // A marker drawn as text (`String`/`&str`) in its column
                // (`div.CodeMirror-gutter-elt`, left = gutterLeft[id]).
                let markers = handle.and_then(|l| l.gutter_markers());
                for (id, mk) in markers.iter().flatten() {
                    let Some(mk) = mk else { continue };
                    let Some(col) = editor.gutter_columns.iter().find(|c| &c.class_name == id)
                    else {
                        continue;
                    };
                    let text = mk
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| mk.downcast_ref::<&'static str>().map(|s| s.to_string()));
                    if let Some(t) = text {
                        marker_pos.push((t, col.left as f32, y));
                    }
                }
            }
        }
        let lnum_right = editor
            .gutter_columns
            .iter()
            .find(|c| c.class_name == "CodeMirror-linenumbers")
            .map(|c| (c.left + c.width) as f32);
        if let (true, Some(lnum_right)) = (editor.options.line_numbers, lnum_right) {
            for line in from..to {
                let n = line + editor.options.first_line_number - editor.first_line();
                let b = match self.numbers.remove(&n) {
                    Some(b) => b,
                    None => {
                        let lc = theme.line_number;
                        self.make_buffer(&n.to_string(), None, false, lc)
                    }
                };
                let digits = n.to_string().len() as f32;
                let x = lnum_right - 3.0 - digits * m.char_width as f32;
                number_pos.push((n, x, wy(editor.text_top(line))));
                numbers_keep.insert(n, b);
            }
        }
        self.numbers = numbers_keep;
        let marker_buffers: Vec<Buffer> = marker_pos
            .iter()
            .map(|(t, _, _)| self.make_buffer(t, None, false, theme.line_number))
            .collect();

        // Scrollbar thumb, above the text and the cursors
        // (`.CodeMirror-vscrollbar { z-index: 6 }`).
        let mut thumb: Option<(Rect, Rgba)> = None;
        let doc_h = editor.doc_height() as f32 + 2.0 * pad_top;
        if doc_h > h {
            let bar_h = (h * h / doc_h).max(20.0);
            let y = (st / (doc_h - h)) * (h - bar_h);
            thumb = Some((full(w - 8.0, y, w - 2.0, y + bar_h), theme.scrollbar));
        }

        // Upload quads (NDC, linear color); the first `under` are drawn below the text.
        let under = (quads.len() * 6) as u32;
        quads.extend(cursors);
        quads.extend(thumb);
        let mut verts: Vec<Vertex> = Vec::with_capacity(quads.len() * 6);
        for (r, c) in &quads {
            let (x0, y0, x1, y1) = (r.left as f32, r.top as f32, r.right as f32, r.bottom as f32);
            let nx = |x: f32| x / w * 2.0 - 1.0;
            let ny = |y: f32| 1.0 - y / h * 2.0;
            let col = [
                srgb_to_linear(c[0]),
                srgb_to_linear(c[1]),
                srgb_to_linear(c[2]),
                c[3],
            ];
            for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y0), (x1, y1), (x0, y1)] {
                verts.push(Vertex {
                    pos: [nx(x), ny(y)],
                    color: col,
                });
            }
        }
        let vbuf = if verts.is_empty() {
            None
        } else {
            Some(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cm quads"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            )
        };

        // Text areas.
        self.viewport.update(queue, Resolution { width, height });
        let text_bounds = TextBounds {
            left: gutter as i32,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        let gutter_bounds = TextBounds {
            left: 0,
            top: 0,
            right: gutter as i32,
            bottom: height as i32,
        };
        let mut areas: Vec<TextArea> = Vec::new();
        let text_color = Color::rgb(0, 0, 0);
        for (id, line_top) in &placed {
            let e = &self.lines[id];
            for s in &e.segs {
                areas.push(TextArea {
                    buffer: &s.buffer,
                    left: wx(s.x as f64),
                    top: line_top + s.top,
                    scale: 1.0,
                    bounds: text_bounds,
                    default_color: text_color,
                    custom_glyphs: &[],
                });
            }
        }
        for (n, x, y) in &number_pos {
            areas.push(TextArea {
                buffer: &self.numbers[n],
                left: *x,
                top: *y,
                scale: 1.0,
                bounds: gutter_bounds,
                default_color: text_color,
                custom_glyphs: &[],
            });
        }
        for (b, (_, x, y)) in marker_buffers.iter().zip(&marker_pos) {
            areas.push(TextArea {
                buffer: b,
                left: *x,
                top: *y,
                scale: 1.0,
                bounds: gutter_bounds,
                default_color: text_color,
                custom_glyphs: &[],
            });
        }
        self.text
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash,
            )
            .expect("glyphon prepare");

        let bg = theme.background;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("cm frame"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cm pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: srgb_to_linear(bg[0]) as f64,
                            g: srgb_to_linear(bg[1]) as f64,
                            b: srgb_to_linear(bg[2]) as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(vb) = &vbuf {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..under, 0..1);
            }
            self.text
                .render(&self.atlas, &self.viewport, &mut pass)
                .expect("glyphon render");
            if let Some(vb) = vbuf.as_ref().filter(|_| verts.len() as u32 > under) {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(under..verts.len() as u32, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
        self.atlas.trim();
    }
}

/// Render one frame offscreen and return RGBA8 pixels (sRGB).
pub fn render_offscreen(
    gpu: &Gpu,
    renderer: &mut Renderer,
    editor: &mut Editor,
    width: u32,
    height: u32,
    frame: FrameState,
) -> Vec<u8> {
    let device = &gpu.device;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cm offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.render(gpu, editor, &view, width, height, frame);
    let bpr = (width * 4).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cm readback"),
        size: (bpr * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("cm copy"),
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(enc.finish()));
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().expect("map callback").expect("map ok");
    let data = slice.get_mapped_range().expect("mapped range");
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    let bgra = matches!(
        renderer.format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    for y in 0..height {
        let row = &data[(y * bpr) as usize..(y * bpr + width * 4) as usize];
        if bgra {
            for px in row.chunks(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        } else {
            out.extend_from_slice(row);
        }
    }
    drop(data);
    buf.unmap();
    out
}

/// Write RGBA8 pixels as a PNG.
pub fn write_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    w.write_image_data(rgba).map_err(std::io::Error::other)?;
    Ok(())
}
