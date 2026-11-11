use std::error::Error;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::archive::Archive;
use crate::config::Config;
use crate::event::UserEvent;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::menu::MenuBar;
use crate::navigation;
use crate::prefetch::Prefetch;
use crate::view;
use crate::viewer::Viewer;

pub fn run(path: Option<PathBuf>, prefetch_depth: usize) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    event_loop.run_app(&mut App::new(path, prefetch_depth, proxy))?;
    Ok(())
}

struct App {
    initial_path: Option<PathBuf>,
    depth: usize,
    proxy: EventLoopProxy<UserEvent>,
    viewer: Option<Viewer>,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    spread: bool,
    scroll: f32,
    last_frame: Vec<u32>,
    config: Config,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    menu: Option<MenuBar>,
    error: Option<String>,
}

impl App {
    fn new(initial_path: Option<PathBuf>, depth: usize, proxy: EventLoopProxy<UserEvent>) -> Self {
        let config = Config::load();
        Self {
            initial_path,
            depth,
            proxy,
            viewer: None,
            window: None,
            surface: None,
            spread: config.spread,
            scroll: 0.0,
            last_frame: Vec::new(),
            config,
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            menu: None,
            error: None,
        }
    }

    fn open(&mut self, path: &Path) {
        match Archive::open(path) {
            Ok(mut archive) => {
                if archive.pages.is_empty() {
                    self.error = Some("no supported images in archive".to_string());
                    self.update_title();
                    return;
                }

                let pages = std::mem::take(&mut archive.pages);
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let proxy = self.proxy.clone();
                let prefetch = Prefetch::start(archive, move || {
                    proxy.send_event(UserEvent::ImageReady).is_ok()
                });
                self.viewer = Some(Viewer::new(name, pages, prefetch, self.depth));
                self.error = None;
                self.scroll = 0.0;
                self.last_frame.clear();
                self.refresh();
            }
            Err(error) => {
                eprintln!("mangania: {error}");
                self.error = Some(error);
                self.update_title();
            }
        }
    }

    fn navigate(&mut self, delta: isize) {
        let Some(viewer) = &self.viewer else {
            return;
        };
        let Some(page) = navigation::step(viewer.current, viewer.pages.len(), self.spread, delta)
        else {
            return;
        };
        self.go_to(page);
    }

    fn go_to(&mut self, page: usize) {
        let spread = self.spread;
        self.scroll = 0.0;
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };
        if !viewer.go_to(page, spread) {
            return;
        }
        self.error = None;
        self.refresh();
    }

    fn set_spread(&mut self, spread: bool) {
        self.scroll = 0.0;
        self.spread = spread;
        self.config.spread = spread;
        self.config.save();
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(menu) = &self.menu {
            menu.single.set_checked(!spread);
            menu.spread.set_checked(spread);
        }

        if let Some(viewer) = self.viewer.as_mut() {
            if spread {
                viewer.align_spread();
            }
            viewer.prefetch.clear_pending();
        }
        self.refresh();
    }

    fn handle_scroll(&mut self, delta: MouseScrollDelta) {
        if self.viewer.is_none() {
            return;
        }
        self.scroll += match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 50.0,
            MouseScrollDelta::PixelDelta(position) => position.y as f32,
        };
        if self.scroll <= -50.0 {
            self.navigate(1);
        } else if self.scroll >= 50.0 {
            self.navigate(-1);
        }
    }

    fn handle_key(&mut self, key: Key, event_loop: &ActiveEventLoop) {
        match key {
            Key::Named(NamedKey::ArrowRight) => self.navigate(1),
            Key::Named(NamedKey::ArrowLeft) => self.navigate(-1),
            Key::Named(NamedKey::Home) => self.go_to(0),
            Key::Named(NamedKey::End) => self.go_to(usize::MAX),
            Key::Named(NamedKey::F11) => {
                let fullscreen = self.is_fullscreen();
                self.set_fullscreen(!fullscreen);
            }
            Key::Character(key) if key.as_str().eq_ignore_ascii_case("v") => {
                self.set_spread(!self.spread);
            }
            Key::Named(NamedKey::Escape) => {
                if self.is_fullscreen() {
                    self.set_fullscreen(false);
                } else {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .and_then(|window| window.fullscreen())
            .is_some()
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        if let Some(window) = &self.window {
            window.set_fullscreen(if fullscreen {
                Some(Fullscreen::Borderless(None))
            } else {
                None
            });
            window.request_redraw();
        }
    }

    fn update_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let title = if let Some(viewer) = &self.viewer {
            let start = viewer.current + 1;
            let end = (viewer.current + 2).min(viewer.pages.len());
            let pages = if self.spread && end > start {
                format!("{start}-{end}/{}", viewer.pages.len())
            } else {
                format!("{start}/{}", viewer.pages.len())
            };
            let mut title = format!("MangaNia - {} - {pages}", viewer.file_name);
            if let Some(message) = viewer.errors.get(&viewer.current) {
                title.push_str(" - error: ");
                title.extend(message.chars().take(120));
            } else if !viewer.cache.contains(viewer.current) {
                title.push_str(" - loading...");
            }
            if let Some(error) = &self.error {
                title.push_str(" - open error: ");
                title.extend(error.chars().take(120));
            }
            title
        } else if let Some(error) = &self.error {
            format!("MangaNia - error: {error}")
        } else {
            "MangaNia - drop a ZIP file".to_string()
        };
        window.set_title(&title);
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn refresh(&mut self) {
        if let Some(viewer) = self.viewer.as_mut() {
            viewer.request_current(self.spread);
            viewer.prefetch_around();
        }
        self.update_title();
        self.request_redraw();
    }

    fn draw(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if surface.resize(width, height).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };

        let (current, next, waiting_for_next) = match self.viewer.as_ref() {
            Some(viewer) => {
                let next = if self.spread {
                    viewer.cache.get(viewer.current + 1)
                } else {
                    None
                };
                let waiting = self.spread
                    && viewer.current + 1 < viewer.pages.len()
                    && next.is_none()
                    && !viewer.errors.contains_key(&(viewer.current + 1));
                (viewer.cache.get(viewer.current), next, waiting)
            }
            None => (None, None, false),
        };

        if current.is_none() || waiting_for_next {
            if self.last_frame.len() == buffer.len() {
                buffer.copy_from_slice(&self.last_frame);
            } else {
                view::blank(&mut buffer);
            }
        } else if let Some(current) = current {
            if self.spread {
                view::render_spread(
                    &mut buffer,
                    size.width as usize,
                    size.height as usize,
                    next.as_deref(),
                    Some(current.as_ref()),
                );
            } else {
                view::render(
                    &mut buffer,
                    size.width as usize,
                    size.height as usize,
                    current.as_ref(),
                );
            }

            self.last_frame.resize(buffer.len(), 0);
            self.last_frame.copy_from_slice(&buffer);
        }
        let _ = buffer.present();
    }

    fn handle_images_ready(&mut self) {
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };

        let mut changed = false;
        for (page, result) in viewer.prefetch.poll() {
            match result {
                Ok(image) => {
                    viewer.insert(page, image);
                    changed = true;
                }
                Err(message) => {
                    eprintln!("mangania: page {}: {message}", page + 1);
                    viewer.errors.insert(page, message);
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }

        self.refresh();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn handle_menu(&mut self, event: muda::MenuEvent) {
        let Some(menu) = &self.menu else {
            return;
        };
        let next = event.id == menu.next.id();
        let prev = event.id == menu.prev.id();
        let first = event.id == menu.first.id();
        let last = event.id == menu.last.id();
        let single = event.id == menu.single.id();
        let spread = event.id == menu.spread.id();
        let fullscreen = event.id == menu.fullscreen.id();

        if next {
            self.navigate(1);
        } else if prev {
            self.navigate(-1);
        } else if first {
            self.go_to(0);
        } else if last {
            self.go_to(usize::MAX);
        } else if single {
            self.set_spread(false);
        } else if spread {
            self.set_spread(true);
        } else if fullscreen {
            let next = !self.is_fullscreen();
            self.set_fullscreen(next);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            self.request_redraw();
            return;
        }

        let setup = || -> Result<_, String> {
            let window = event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("MangaNia")
                        .with_inner_size(LogicalSize::new(1200.0, 900.0)),
                )
                .map_err(|error| format!("create window: {error}"))?;
            let window = Arc::new(window);
            let context = softbuffer::Context::new(window.clone())
                .map_err(|error| format!("create display context: {error}"))?;
            let surface = softbuffer::Surface::new(&context, window.clone())
                .map_err(|error| format!("create surface: {error}"))?;
            Ok((window, surface))
        };
        let (window, surface) = match setup() {
            Ok(setup) => setup,
            Err(error) => {
                eprintln!("mangania: {error}");
                event_loop.exit();
                return;
            }
        };

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if self.menu.is_none() {
            let menu = MenuBar::new(self.spread, self.proxy.clone());
            menu.attach(&window);
            self.menu = Some(menu);
        }

        self.window = Some(window);
        self.surface = Some(surface);

        if let Some(path) = self.initial_path.take() {
            self.open(&path);
        } else {
            self.update_title();
        }
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::ImageReady => self.handle_images_ready(),
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            UserEvent::Menu(event) => self.handle_menu(event),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) => {
                self.open(&path);
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.handle_key(event.logical_key, event_loop);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => match button {
                MouseButton::Left => self.navigate(1),
                MouseButton::Right => self.navigate(-1),
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => self.handle_scroll(delta),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}
