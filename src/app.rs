use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::archive::Archive;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::menu::MenuBar;
use crate::prefetch::{Prefetch, UserEvent};
use crate::view;

const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;

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
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    menu: Option<MenuBar>,
    error: Option<String>,
}

struct Viewer {
    name: String,
    pages: Vec<usize>,
    current: usize,
    prefetch: Prefetch,
    cache: HashMap<usize, Arc<RgbaImage>>,
    order: VecDeque<usize>,
    used: usize,
    errors: HashMap<usize, String>,
    depth: usize,
}

impl App {
    fn new(initial_path: Option<PathBuf>, depth: usize, proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            initial_path,
            depth,
            proxy,
            viewer: None,
            window: None,
            surface: None,
            spread: false,
            scroll: 0.0,
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
                self.viewer = Some(Viewer {
                    name,
                    pages,
                    current: 0,
                    prefetch: Prefetch::start(archive, self.proxy.clone()),
                    cache: HashMap::new(),
                    order: VecDeque::new(),
                    used: 0,
                    errors: HashMap::new(),
                    depth: self.depth,
                });
                self.error = None;
                self.scroll = 0.0;
                self.refresh();
            }
            Err(error) => {
                self.error = Some(error);
                self.update_title();
            }
        }
    }

    fn navigate(&mut self, delta: isize) {
        let step = if self.spread { 2 } else { 1 };
        let Some(viewer) = &self.viewer else {
            return;
        };
        let target = if delta > 0 {
            viewer.current + step
        } else {
            viewer.current.saturating_sub(step)
        };
        self.go_to(target);
    }

    fn go_to(&mut self, page: usize) {
        let spread = self.spread;
        self.scroll = 0.0;
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };
        let page = page.min(viewer.pages.len().saturating_sub(1));
        let page = if spread { page - page % 2 } else { page };
        if viewer.current == page {
            return;
        }

        viewer.current = page;
        viewer.prefetch.clear_pending();
        self.error = None;
        self.refresh();
    }

    fn set_spread(&mut self, spread: bool) {
        self.scroll = 0.0;
        self.spread = spread;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(menu) = &self.menu {
            menu.single.set_checked(!spread);
            menu.spread.set_checked(spread);
        }

        if let Some(viewer) = self.viewer.as_mut() {
            if spread {
                viewer.current -= viewer.current % 2;
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
            let mut title = format!("MangaNia - {} - {pages}", viewer.name);
            if let Some(message) = viewer.errors.get(&viewer.current) {
                title.push_str(" - error: ");
                title.extend(message.chars().take(120));
            } else if !viewer.cache.contains_key(&viewer.current) {
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

        let (current, next) = match self.viewer.as_ref() {
            Some(viewer) => (
                viewer.cache.get(&viewer.current).cloned(),
                if self.spread {
                    viewer.cache.get(&(viewer.current + 1)).cloned()
                } else {
                    None
                },
            ),
            None => (None, None),
        };

        match current {
            Some(current) if self.spread => view::render_spread(
                &mut buffer,
                size.width as usize,
                size.height as usize,
                next.as_deref(),
                Some(current.as_ref()),
            ),
            Some(current) => view::render(
                &mut buffer,
                size.width as usize,
                size.height as usize,
                current.as_ref(),
            ),
            None => view::blank(&mut buffer),
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

impl Viewer {
    fn request_current(&mut self, spread: bool) {
        let current = self.current;
        self.request(current, true);
        if spread && current + 1 < self.pages.len() {
            self.request(current + 1, false);
        }
    }

    fn prefetch_around(&mut self) {
        let current = self.current;
        for offset in 1..=self.depth {
            let Some(page) = current.checked_add(offset) else {
                break;
            };
            if page >= self.pages.len() {
                break;
            }
            self.request(page, false);
        }
    }

    fn request(&mut self, page: usize, urgent: bool) {
        if page >= self.pages.len()
            || self.cache.contains_key(&page)
            || self.errors.contains_key(&page)
        {
            return;
        }
        let entry = self.pages[page];
        self.prefetch.request(page, entry, urgent);
    }

    fn insert(&mut self, page: usize, image: Arc<RgbaImage>) {
        let bytes = image.as_raw().len();
        if bytes > MAX_CACHE_BYTES && page != self.current {
            return;
        }
        if self.cache.insert(page, image).is_none() {
            self.order.push_back(page);
            self.used += bytes;
        }

        let limit = self.depth + 2;
        while (self.order.len() > limit || self.used > MAX_CACHE_BYTES) && self.cache.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if oldest == self.current {
                self.order.push_back(oldest);
                continue;
            }
            if let Some(image) = self.cache.remove(&oldest) {
                self.used -= image.as_raw().len();
            }
        }
    }
}
