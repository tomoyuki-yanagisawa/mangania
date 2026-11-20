mod draw;
mod input;
mod open;
mod window;

use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

use crate::config::Config;
use crate::debug_log;
use crate::event::UserEvent;
use crate::input::bindings::{KeyBindings, MouseBindings};
use crate::input::click::ClickTracker;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use crate::platform::MenuBar;
use crate::render::viewer::{CacheLimits, Viewer};

use self::window::Window;

pub fn run(path: Option<PathBuf>, prefetch: Option<usize>) -> Result<(), Box<dyn Error>> {
    let event_loop = crate::platform::event_loop_builder().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    event_loop.run_app(&mut App::new(path, prefetch, proxy))?;
    Ok(())
}

struct App {
    initial_path: Option<PathBuf>,
    depth: usize,
    proxy: EventLoopProxy<UserEvent>,
    viewer: Option<Viewer>,
    window: Option<Window>,
    scroll: f32,
    modifiers: ModifiersState,
    clicks: ClickTracker,
    config: Config,
    keys: KeyBindings,
    mouse: MouseBindings,
    status: Option<String>,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    menu: Option<MenuBar>,
    error: Option<String>,
    open_generation: u64,
    viewer_generation: u64,
    saving: bool,
}

impl App {
    fn new(
        initial_path: Option<PathBuf>,
        prefetch: Option<usize>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let config = Config::load();
        if config.debug
            && let Some(path) = crate::config::config_path("debug.log")
        {
            if let Err(error) = debug_log::init(&path) {
                eprintln!("mangania: open debug log {}: {error}", path.display());
            } else {
                debug_log::log(format_args!("debug log: {}", path.display()));
            }
        }
        debug_log::log(format_args!(
            "config: mode={} prefetch={} cache_pages={} cache_mb={} decode_batch_size={} scale_filter={} allow_upscale={} debug={}",
            config.mode.as_str(),
            config.prefetch,
            config.cache_pages,
            config.cache_mb,
            config.decode_batch_size,
            config.scale_filter.as_str(),
            config.allow_upscale,
            config.debug
        ));
        let depth = prefetch.unwrap_or(config.prefetch);
        debug_log::log(format_args!("prefetch depth: {depth}"));
        debug_log::log(format_args!(
            "decode batch size: {}",
            config.effective_decode_batch_size()
        ));
        let cache_limits = CacheLimits {
            entries: config.cache_entries(),
            bytes: config.cache_bytes(),
        };
        debug_log::log(format_args!(
            "memory cache: entries={} bytes={}",
            cache_limits.entries, cache_limits.bytes
        ));
        let keys = KeyBindings::load();
        let mouse = MouseBindings::load();
        Self {
            initial_path,
            depth,
            proxy,
            viewer: None,
            window: None,
            scroll: 0.0,
            modifiers: ModifiersState::empty(),
            clicks: ClickTracker::new(),
            config,
            keys,
            mouse,
            status: None,
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            menu: None,
            error: None,
            open_generation: 0,
            viewer_generation: 0,
            saving: false,
        }
    }

    fn update_title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let title = if let Some(viewer) = &self.viewer {
            let (start, end) = viewer.page_range();
            let pages = if end > start {
                format!("{start}-{end}/{}", viewer.pages.len())
            } else {
                format!("{start}/{}", viewer.pages.len())
            };
            let mut title = format!("MangaNia - {} - {pages}", viewer.name());
            if let Some(message) = viewer.error(viewer.current) {
                title.push_str(" - error: ");
                title.extend(message.chars().take(120));
            } else if viewer.is_loading() {
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
        } else if let Some(status) = &self.status {
            format!("MangaNia - {status}")
        } else {
            "MangaNia - drop a ZIP file".to_string()
        };
        debug_log::log(format_args!("title: {title}"));
        window.winit().set_title(&title);
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn refresh(&mut self) {
        if let Some(viewer) = self.viewer.as_mut() {
            viewer.request_display();
        }
        self.update_title();
        self.request_redraw();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            self.request_redraw();
            return;
        }

        let window = match Window::new(event_loop) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("mangania: {error}");
                event_loop.exit();
                return;
            }
        };

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if self.menu.is_none() {
            let menu = MenuBar::new(
                self.config.mode,
                self.config.scale_filter,
                self.config.allow_upscale,
                self.proxy.clone(),
            );
            menu.attach(window.winit());
            self.menu = Some(menu);
        }

        self.window = Some(window);

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
            UserEvent::Opened { generation, result } => self.finish_open(generation, result),
            UserEvent::Saved { generation, result } => self.finish_save(generation, result),
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
                self.sync_viewport();
                #[cfg(any(target_os = "windows", target_os = "macos"))]
                self.sync_menu();
                self.refresh();
            }
            WindowEvent::DroppedFile(path) => {
                if let Some(window) = &self.window {
                    window.winit().focus_window();
                }
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
