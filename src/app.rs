use std::error::Error;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Fullscreen, Window, WindowId};

use crate::archive::Archive;
use crate::bindings::{Action, KeyBindings, MouseBindings, MouseInput, WheelDirection};
use crate::click::ClickTracker;
use crate::config::Config;
use crate::event::UserEvent;
use crate::export;
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
    modifiers: ModifiersState,
    clicks: ClickTracker,
    config: Config,
    keys: KeyBindings,
    mouse: MouseBindings,
    status: Option<String>,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    menu: Option<MenuBar>,
    error: Option<String>,
}

impl App {
    fn new(initial_path: Option<PathBuf>, depth: usize, proxy: EventLoopProxy<UserEvent>) -> Self {
        let config = Config::load();
        let keys = KeyBindings::load();
        let mouse = MouseBindings::load();
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
            modifiers: ModifiersState::empty(),
            clicks: ClickTracker::new(),
            config,
            keys,
            mouse,
            status: None,
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
                self.viewer = Some(Viewer::new(
                    name,
                    path.to_path_buf(),
                    pages,
                    prefetch,
                    self.depth,
                ));
                self.error = None;
                self.status = None;
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
        self.status = None;
        self.refresh();
    }

    fn set_spread(&mut self, spread: bool) {
        self.scroll = 0.0;
        self.spread = spread;
        self.config.spread = spread;
        self.config.save();
        self.status = None;
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

    fn handle_click(&mut self, button: MouseButton, event_loop: &ActiveEventLoop) {
        let actions = self.clicks.press(button, &self.mouse, Instant::now());
        for action in actions {
            self.perform(action, event_loop);
        }
        self.update_click_timer(event_loop);
    }

    fn flush_pending_click(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(action) = self.clicks.flush() {
            self.perform(action, event_loop);
        }
    }

    fn update_click_timer(&self, event_loop: &ActiveEventLoop) {
        let control_flow = match self.clicks.deadline() {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        };
        event_loop.set_control_flow(control_flow);
    }

    fn handle_scroll(&mut self, delta: MouseScrollDelta, event_loop: &ActiveEventLoop) {
        self.flush_pending_click(event_loop);
        self.update_click_timer(event_loop);
        if self.viewer.is_none() {
            return;
        }
        self.scroll += match delta {
            MouseScrollDelta::LineDelta(_, y) => y * 50.0,
            MouseScrollDelta::PixelDelta(position) => position.y as f32,
        };
        let threshold = self.mouse.wheel_threshold();
        let direction = if self.scroll <= -threshold {
            Some(WheelDirection::Down)
        } else if self.scroll >= threshold {
            Some(WheelDirection::Up)
        } else {
            None
        };
        let Some(direction) = direction else {
            return;
        };
        self.scroll = 0.0;
        if let Some(action) = self.mouse.action_for(MouseInput::Wheel(direction)) {
            self.perform(action, event_loop);
        }
    }

    fn handle_key(&mut self, key: Key, event_loop: &ActiveEventLoop) {
        if let Some(action) = self.keys.action_for(&key, self.modifiers) {
            self.perform(action, event_loop);
        }
    }

    fn perform(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Next => self.navigate(1),
            Action::Prev => self.navigate(-1),
            Action::First => self.go_to(0),
            Action::Last => self.go_to(usize::MAX),
            Action::ToggleSpread => self.set_spread(!self.spread),
            Action::SinglePage => self.set_spread(false),
            Action::SpreadPage => self.set_spread(true),
            Action::ToggleFullscreen => {
                let fullscreen = self.is_fullscreen();
                self.set_fullscreen(!fullscreen);
            }
            Action::SavePage => self.save_current_page(),
            Action::Quit => {
                if self.is_fullscreen() {
                    self.set_fullscreen(false);
                } else {
                    event_loop.exit();
                }
            }
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .and_then(|window| window.fullscreen())
            .is_some()
    }

    fn save_current_page(&mut self) {
        let Some(viewer) = &self.viewer else {
            return;
        };
        match export::save_current_page(&viewer.path, viewer.current, &self.config) {
            Ok(path) => {
                eprintln!("mangania: saved {}", path.display());
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default();
                self.status = Some(format!("saved {name}"));
                self.error = None;
            }
            Err(error) => {
                eprintln!("mangania: save: {error}");
                self.status = Some(format!("save error: {error}"));
            }
        }
        self.update_title();
        self.request_redraw();
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
        self.update_fullscreen_menu(fullscreen);
    }

    fn update_fullscreen_menu(&self, _fullscreen: bool) {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(menu) = &self.menu {
            menu.fullscreen.set_checked(_fullscreen);
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
            if let Some(status) = &self.status {
                title.push_str(" - ");
                title.push_str(status);
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
            let pages = if self.spread {
                [next.as_deref(), Some(current.as_ref())]
            } else {
                [None, Some(current.as_ref())]
            };
            view::render_pages(
                &mut buffer,
                size.width as usize,
                size.height as usize,
                &pages,
            );

            self.last_frame.resize(buffer.len(), 0);
            self.last_frame.copy_from_slice(&buffer);
        }
        let _ = buffer.present();
    }

    fn handle_images_ready(&mut self) {
        let Some(viewer) = self.viewer.as_mut() else {
            return;
        };

        let results = viewer.prefetch.poll();
        if results.is_empty() {
            return;
        }

        for (page, result) in results {
            match result {
                Ok(image) => viewer.insert(page, image),
                Err(message) => {
                    eprintln!("mangania: page {}: {message}", page + 1);
                    viewer.errors.insert(page, message);
                }
            }
        }

        self.refresh();
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn handle_menu(&mut self, event: muda::MenuEvent, event_loop: &ActiveEventLoop) {
        let action = self
            .menu
            .as_ref()
            .and_then(|menu| menu.action_for(&event.id));
        if let Some(action) = action {
            self.perform(action, event_loop);
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
            UserEvent::Menu(event) => self.handle_menu(event, _event_loop),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .clicks
            .deadline()
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.flush_pending_click(event_loop);
        }
        self.update_click_timer(event_loop);
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
                self.update_fullscreen_menu(self.is_fullscreen());
                self.request_redraw();
            }
            WindowEvent::DroppedFile(path) => {
                self.open(&path);
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.handle_key(event.logical_key, event_loop);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.clicks.cursor_moved(position);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => self.handle_click(button, event_loop),
            WindowEvent::MouseWheel { delta, .. } => self.handle_scroll(delta, event_loop),
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }
}
