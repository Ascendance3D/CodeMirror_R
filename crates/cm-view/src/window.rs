//! The winit front end: a window, a wgpu surface, and winit events mapped to
//! [`crate::app::Input`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

use crate::app::{App, AppOptions, Clipboard, Input, Step};
use crate::keymap::KeyEvent;
use crate::native_clipboard::SystemClipboard;
use crate::render::{FrameState, Gpu, Renderer};

/// `keyNames` for a winit key (the key without modifiers, as CM5 uses `keyCode`).
pub fn cm_key_name(key: &Key) -> Option<String> {
    let s = match key {
        Key::Named(n) => match n {
            NamedKey::ArrowLeft => "Left",
            NamedKey::ArrowRight => "Right",
            NamedKey::ArrowUp => "Up",
            NamedKey::ArrowDown => "Down",
            NamedKey::Home => "Home",
            NamedKey::End => "End",
            NamedKey::PageUp => "PageUp",
            NamedKey::PageDown => "PageDown",
            NamedKey::Delete => "Delete",
            NamedKey::Backspace => "Backspace",
            NamedKey::Tab => "Tab",
            NamedKey::Enter => "Enter",
            NamedKey::Escape => "Esc",
            NamedKey::Insert => "Insert",
            NamedKey::Space => "Space",
            NamedKey::Shift => "Shift",
            NamedKey::Control => "Ctrl",
            NamedKey::Alt => "Alt",
            NamedKey::Super | NamedKey::Meta => "Mod",
            NamedKey::Pause => "Pause",
            NamedKey::CapsLock => "CapsLock",
            NamedKey::ScrollLock => "ScrollLock",
            NamedKey::PrintScreen => "PrintScrn",
            NamedKey::F1 => "F1",
            NamedKey::F2 => "F2",
            NamedKey::F3 => "F3",
            NamedKey::F4 => "F4",
            NamedKey::F5 => "F5",
            NamedKey::F6 => "F6",
            NamedKey::F7 => "F7",
            NamedKey::F8 => "F8",
            NamedKey::F9 => "F9",
            NamedKey::F10 => "F10",
            NamedKey::F11 => "F11",
            NamedKey::F12 => "F12",
            _ => return None,
        }
        .to_string(),
        Key::Character(c) => {
            let mut it = c.chars();
            let ch = it.next()?;
            if it.next().is_some() {
                return None;
            }
            match ch {
                'a'..='z' => ch.to_ascii_uppercase().to_string(),
                'A'..='Z' | '0'..='9' => ch.to_string(),
                ';' | '=' | ',' | '-' | '.' | '/' | '`' | '[' | '\\' | ']' | '\'' | '*' => {
                    ch.to_string()
                }
                ' ' => "Space".into(),
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(s)
}

/// A CM5 key event from a winit key and the modifier state.
pub fn key_event(key_without_mods: &Key, mods: ModifiersState) -> Option<KeyEvent> {
    let name = cm_key_name(key_without_mods)?;
    Some(KeyEvent {
        key: name,
        shift: mods.shift_key(),
        ctrl: mods.control_key(),
        alt: mods.alt_key(),
        meta: mods.super_key(),
        alt_graph: false,
    })
}

struct WinState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: Renderer,
}

pub struct EditorWindow {
    path: PathBuf,
    opts: AppOptions,
    app: Option<App>,
    state: Option<WinState>,
    mods: ModifiersState,
    cursor: (f64, f64),
    mouse_down: bool,
    blink_start: Instant,
    focused: bool,
    script: Option<std::collections::VecDeque<Step>>,
    /// The manual clock of a scripted run (`pause` advances it).
    script_clock: Option<std::rc::Rc<std::cell::Cell<f64>>>,
    pub exit_error: Option<String>,
    start: Instant,
}

impl EditorWindow {
    pub fn new(path: PathBuf, opts: AppOptions, script: Option<Vec<Step>>) -> EditorWindow {
        // The event loop runs on this thread, so the thread-local clock applies.
        let script_clock = script.as_ref().map(|_| cm_core::clock::manual(1_000_000.0));
        EditorWindow {
            script_clock,
            path,
            opts,
            app: None,
            state: None,
            mods: ModifiersState::empty(),
            cursor: (0.0, 0.0),
            mouse_down: false,
            blink_start: Instant::now(),
            focused: true,
            script: script.map(Into::into),
            exit_error: None,
            start: Instant::now(),
        }
    }

    fn redraw(&mut self) {
        let (Some(st), Some(app)) = (&mut self.state, &mut self.app) else {
            return;
        };
        let frame = match st.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                st.surface.configure(&st.gpu.device, &st.config);
                st.window.request_redraw();
                return;
            }
            _ => {
                st.window.request_redraw();
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let elapsed = self.blink_start.elapsed().as_millis();
        let cursor_visible = !self.focused || (elapsed / 530) % 2 == 0;
        st.renderer.render(
            &st.gpu,
            &mut app.editor,
            &view,
            st.config.width,
            st.config.height,
            FrameState {
                focused: self.focused,
                cursor_visible: cursor_visible && self.focused,
            },
        );
        st.window.pre_present_notify();
        st.gpu.queue.present(frame);
        st.window.set_title(&app.title());
    }

    fn input(&mut self, input: Input) {
        if let Some(app) = &mut self.app {
            if app.handle(input) {
                self.blink_start = Instant::now();
                if let Some(st) = &self.state {
                    st.window.request_redraw();
                }
            }
        }
    }

    fn run_script_step(&mut self, event_loop: &ActiveEventLoop) {
        let Some(script) = &mut self.script else {
            return;
        };
        let Some(step) = script.pop_front() else {
            event_loop.exit();
            return;
        };
        match step {
            Step::Input(evs) => {
                for e in evs {
                    self.input(e);
                }
            }
            Step::Pause(ms) => match &self.script_clock {
                Some(c) => c.set(c.get() + ms as f64),
                None => std::thread::sleep(Duration::from_millis(ms)),
            },
            Step::Screenshot(path) => {
                if let (Some(st), Some(app)) = (&mut self.state, &mut self.app) {
                    let (w, h) = (st.config.width, st.config.height);
                    let mut r = Renderer::new(
                        &st.gpu,
                        wgpu::TextureFormat::Rgba8UnormSrgb,
                        st.renderer.font_size,
                        st.renderer.line_height,
                    );
                    let px = crate::render::render_offscreen(
                        &st.gpu,
                        &mut r,
                        &mut app.editor,
                        w,
                        h,
                        FrameState::default(),
                    );
                    if let Err(e) = crate::render::write_png(&path, w, h, &px) {
                        self.exit_error = Some(format!("screenshot: {e}"));
                    }
                }
            }
            Step::Quit => {
                if let Some(app) = &self.app {
                    if !app.status.is_empty() {
                        eprintln!("{}", app.status);
                    }
                }
                event_loop.exit();
            }
        }
    }
}

impl ApplicationHandler for EditorWindow {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_inner_size(LogicalSize::new(1000.0, 720.0))
            .with_title("cm-edit");
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.exit_error = Some(format!("create_window: {e}"));
                event_loop.exit();
                return;
            }
        };
        window.set_ime_allowed(true);
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
                Box::new(event_loop.owned_display_handle()),
            ));
        let surface = match instance.create_surface(window.clone()) {
            Ok(s) => s,
            Err(e) => {
                self.exit_error = Some(format!("create_surface: {e}"));
                event_loop.exit();
                return;
            }
        };
        let gpu = match Gpu::with_instance(instance, Some(&surface)) {
            Ok(g) => g,
            Err(e) => {
                self.exit_error = Some(e);
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&gpu.device, &config);
        let sf = window.scale_factor() as f32;
        let font_size = (14.0 * sf).round();
        let line_height = (font_size * 1.35).round();
        let mut renderer = Renderer::new(&gpu, format, font_size, line_height);
        let metrics = renderer.metrics();
        let clipboard: Box<dyn Clipboard> = Box::new(if self.script.is_some() {
            SystemClipboard::in_memory()
        } else {
            SystemClipboard::new()
        });
        match App::open(&self.path, metrics, &self.opts, clipboard) {
            Ok(mut app) => {
                app.editor
                    .set_size(config.width as f64, config.height as f64);
                window.set_title(&app.title());
                self.app = Some(app);
            }
            Err(e) => {
                self.exit_error = Some(format!("open {}: {e}", self.path.display()));
                event_loop.exit();
                return;
            }
        }
        self.state = Some(WinState {
            window: window.clone(),
            surface,
            config,
            gpu,
            renderer,
        });
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(st) = &mut self.state {
                    st.config.width = size.width.max(1);
                    st.config.height = size.height.max(1);
                    st.surface.configure(&st.gpu.device, &st.config);
                }
                self.input(Input::Resize {
                    width: size.width.max(1) as f64,
                    height: size.height.max(1) as f64,
                });
            }
            WindowEvent::Focused(f) => {
                self.focused = f;
                if let Some(app) = &mut self.app {
                    app.set_focused(f);
                }
                if let Some(st) = &self.state {
                    st.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                let base = event.key_without_modifiers();
                let key = key_event(&base, self.mods).unwrap_or_default();
                let text = event.text.as_ref().map(|t| t.to_string());
                self.input(Input::Key { key, text });
            }
            WindowEvent::Ime(Ime::Commit(text)) => self.input(Input::Text(text)),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if self.mouse_down {
                    let mods = KeyEvent {
                        shift: self.mods.shift_key(),
                        ctrl: self.mods.control_key(),
                        alt: self.mods.alt_key(),
                        meta: self.mods.super_key(),
                        ..Default::default()
                    };
                    self.input(Input::MouseMove {
                        x: position.x,
                        y: position.y,
                        mods,
                    });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Right => 3,
                    _ => return,
                };
                let (x, y) = self.cursor;
                if state == ElementState::Pressed {
                    let mods = KeyEvent {
                        shift: self.mods.shift_key(),
                        ctrl: self.mods.control_key(),
                        alt: self.mods.alt_key(),
                        meta: self.mods.super_key(),
                        ..Default::default()
                    };
                    self.mouse_down = b == 1;
                    let time = self.start.elapsed().as_secs_f64() * 1000.0;
                    self.input(Input::MouseDown {
                        x,
                        y,
                        button: b,
                        mods,
                        time,
                    });
                } else if b == 1 {
                    self.mouse_down = false;
                    self.input(Input::MouseUp { x, y });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lh = self
                    .app
                    .as_ref()
                    .map(|a| a.editor.metrics.text_height)
                    .unwrap_or(20.0);
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (-x as f64 * lh * 3.0, -y as f64 * lh * 3.0)
                    }
                    MouseScrollDelta::PixelDelta(p) => (-p.x, -p.y),
                };
                self.input(Input::Wheel { dx, dy });
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.script.is_some() {
            self.run_script_step(event_loop);
            if let Some(st) = &self.state {
                st.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        let more = self.app.as_mut().is_some_and(|a| a.idle());
        if let Some(st) = &self.state {
            if more {
                st.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::Poll);
            } else {
                // Cursor blink (`cursorBlinkRate` 530ms).
                let next = Instant::now()
                    + Duration::from_millis(
                        530 - (self.blink_start.elapsed().as_millis() % 530) as u64,
                    );
                st.window.request_redraw();
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            }
        }
    }
}

/// Run the editor window until it closes.
pub fn run(path: PathBuf, opts: AppOptions, script: Option<Vec<Step>>) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("event loop: {e}"))?;
    let mut w = EditorWindow::new(path, opts, script);
    event_loop
        .run_app(&mut w)
        .map_err(|e| format!("run: {e}"))?;
    match w.exit_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
