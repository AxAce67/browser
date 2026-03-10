use crate::html;
use crate::layout;
use crate::paint::{Color, DisplayCommand, DisplayList, CHAR_WIDTH};
use crate::source;
use crate::style;
use font8x8::UnicodeFonts;
use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

const CHROME_HEIGHT: u32 = 52;
const ADDRESS_BAR_HEIGHT: u32 = 28;
const ADDRESS_BAR_PADDING: u32 = 12;

pub fn run(initial_source: &str) -> Result<(), String> {
    let initial_page = load_page(initial_source)?;
    let event_loop =
        EventLoop::new().map_err(|err| format!("failed to create event loop: {err}"))?;
    let mut app = GuiApp::new(initial_page, initial_source);
    event_loop
        .run_app(&mut app)
        .map_err(|err| format!("failed to run GUI app: {err}"))
}

struct GuiApp {
    page: PageData,
    current_source: String,
    address_input: String,
    address_focus: bool,
    preedit_text: String,
    status_message: Option<String>,
    title: String,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    viewport_width: u32,
    viewport_height: u32,
    scroll_y: u32,
    modifiers: ModifiersState,
}

impl GuiApp {
    fn new(page: PageData, initial_source: &str) -> Self {
        let viewport_width = page.display_list.width.clamp(520, 1200);
        let viewport_height = (page.display_list.height + CHROME_HEIGHT).clamp(360, 800);
        Self {
            title: initial_source.to_string(),
            page,
            current_source: initial_source.to_string(),
            address_input: initial_source.to_string(),
            address_focus: false,
            preedit_text: String::new(),
            status_message: Some(
                "Cmd/Ctrl+L to edit address, Enter to load, wheel to scroll".to_string(),
            ),
            window: None,
            pixels: None,
            viewport_width,
            viewport_height,
            scroll_y: 0,
            modifiers: ModifiersState::empty(),
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let Some(pixels) = self.pixels.as_mut() else {
            return Ok(());
        };

        let frame = pixels.frame_mut();
        clear_frame(frame, Color::rgb(240, 236, 228));
        draw_chrome(
            frame,
            self.viewport_width,
            self.viewport_height,
            &self.address_input,
            &self.preedit_text,
            self.address_focus,
            self.status_message.as_deref(),
        );
        rasterize(
            &self.page.display_list,
            frame,
            self.viewport_width,
            self.viewport_height,
            self.scroll_y,
        );
        pixels
            .render()
            .map_err(|err| format!("failed to render frame: {err}"))
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn navigate(&mut self, source_input: String) {
        match load_page(&source_input) {
            Ok(page) => {
                self.page = page;
                self.current_source = source_input.clone();
                self.address_input = source_input.clone();
                self.title = source_input;
                self.scroll_y = 0;
                self.status_message = Some("Page loaded".to_string());
                self.preedit_text.clear();
                self.address_focus = false;
                if let Some(window) = self.window.as_ref() {
                    window.set_title(&format!("Toy Browser - {}", self.title));
                }
                if let Some(pixels) = self.pixels.as_mut() {
                    let _ = pixels.resize_buffer(self.viewport_width, self.viewport_height);
                }
            }
            Err(err) => {
                self.status_message = Some(err);
            }
        }
        self.request_redraw();
    }

    fn content_viewport_height(&self) -> u32 {
        self.viewport_height.saturating_sub(CHROME_HEIGHT).max(1)
    }
}

impl ApplicationHandler for GuiApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attributes = Window::default_attributes()
            .with_title(format!("Toy Browser - {}", self.title))
            .with_inner_size(LogicalSize::new(
                self.viewport_width as f64,
                self.viewport_height as f64,
            ))
            .with_min_inner_size(LogicalSize::new(420.0, 320.0));

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        let window_size = window.inner_size();
        self.viewport_width = window_size.width.max(1);
        self.viewport_height = window_size.height.max(1);
        self.scroll_y = clamp_scroll(
            self.scroll_y,
            self.page.display_list.height,
            self.content_viewport_height(),
        );

        let surface_texture =
            SurfaceTexture::new(window_size.width, window_size.height, window.clone());
        let pixels = match Pixels::new(self.viewport_width, self.viewport_height, surface_texture) {
            Ok(pixels) => pixels,
            Err(err) => {
                eprintln!("failed to create pixel buffer: {err}");
                event_loop.exit();
                return;
            }
        };

        window.request_redraw();
        self.pixels = Some(pixels);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Ime(event) => match event {
                Ime::Commit(text) if self.address_focus => {
                    self.address_input.push_str(&text);
                    self.preedit_text.clear();
                    self.request_redraw();
                }
                Ime::Preedit(text, _) if self.address_focus => {
                    self.preedit_text = text;
                    self.request_redraw();
                }
                Ime::Disabled | Ime::Enabled | Ime::Commit(_) | Ime::Preedit(_, _) => {}
            },
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.draw() {
                    eprintln!("{err}");
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                self.viewport_width = size.width.max(1);
                self.viewport_height = size.height.max(1);
                self.scroll_y = clamp_scroll(
                    self.scroll_y,
                    self.page.display_list.height,
                    self.content_viewport_height(),
                );
                if let Some(pixels) = self.pixels.as_mut() {
                    if let Err(err) = pixels.resize_surface(size.width, size.height) {
                        eprintln!("failed to resize surface: {err}");
                        event_loop.exit();
                        return;
                    }
                    if let Err(err) = pixels.resize_buffer(self.viewport_width, self.viewport_height)
                    {
                        eprintln!("failed to resize buffer: {err}");
                        event_loop.exit();
                        return;
                    }
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } if !self.address_focus => {
                let scroll_delta = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => (-(lines * 54.0)).round() as i32,
                    MouseScrollDelta::PixelDelta(position) => -position.y.round() as i32,
                };
                self.scroll_y = apply_scroll_delta(
                    self.scroll_y,
                    scroll_delta,
                    self.page.display_list.height,
                    self.content_viewport_height(),
                );
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if matches!(event.logical_key.as_ref(), Key::Character(ch) if ch.eq_ignore_ascii_case("l"))
                    && (self.modifiers.control_key() || self.modifiers.super_key())
                {
                    self.address_focus = true;
                    self.address_input = self.current_source.clone();
                    self.preedit_text.clear();
                    self.status_message = Some("Editing address".to_string());
                    self.request_redraw();
                    return;
                }

                if self.address_focus {
                    match event.logical_key.as_ref() {
                        Key::Named(NamedKey::Enter) => {
                            let requested = self.address_input.trim().to_string();
                            if !requested.is_empty() {
                                self.navigate(requested);
                            }
                        }
                        Key::Named(NamedKey::Escape) => {
                            self.address_focus = false;
                            self.address_input = self.current_source.clone();
                            self.preedit_text.clear();
                            self.status_message = Some("Address edit cancelled".to_string());
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::Backspace) => {
                            self.address_input.pop();
                            self.request_redraw();
                        }
                        _ => {}
                    }
                    return;
                }

                let next = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::ArrowDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        48,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::ArrowUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -48,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::PageDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        self.content_viewport_height() as i32 - 48,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::PageUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -(self.content_viewport_height() as i32 - 48),
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::Home) => Some(0),
                    Key::Named(NamedKey::End) => Some(clamp_scroll(
                        u32::MAX,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Character(ch) if ch.eq_ignore_ascii_case("r") => {
                        self.navigate(self.current_source.clone());
                        None
                    }
                    _ => None,
                };

                if let Some(next_scroll) = next {
                    self.scroll_y = next_scroll;
                    self.request_redraw();
                }
            }
            _ => {}
        }
    }
}

struct PageData {
    display_list: DisplayList,
}

fn load_page(source_input: &str) -> Result<PageData, String> {
    let (html_input, _) = source::load_html(Some(source_input))?;
    let document = html::parse(&html_input);
    let stylesheet = style::collect_stylesheets(&document);
    let styled = style::style_tree(&document, &stylesheet);
    let layout = layout::build(&styled, 48);
    let display_list = crate::paint::build_display_list(&layout);
    Ok(PageData { display_list })
}

fn clear_frame(frame: &mut [u8], color: Color) {
    for pixel in frame.chunks_exact_mut(4) {
        pixel[0] = color.r;
        pixel[1] = color.g;
        pixel[2] = color.b;
        pixel[3] = color.a;
    }
}

fn draw_chrome(
    frame: &mut [u8],
    width: u32,
    height: u32,
    address_input: &str,
    preedit_text: &str,
    address_focus: bool,
    status_message: Option<&str>,
) {
    draw_rect(frame, width, height, 0, 0, width as i32, CHROME_HEIGHT as i32, Color::rgb(232, 228, 220));
    draw_rect(
        frame,
        width,
        height,
        ADDRESS_BAR_PADDING as i32,
        12,
        width.saturating_sub(ADDRESS_BAR_PADDING * 2) as i32,
        ADDRESS_BAR_HEIGHT as i32,
        if address_focus {
            Color::rgb(255, 255, 255)
        } else {
            Color::rgb(246, 243, 236)
        },
    );

    let display_text = format!(
        "{}{}{}",
        if address_focus { "> " } else { "" },
        address_input,
        preedit_text
    );
    draw_text(
        frame,
        width,
        height,
        (ADDRESS_BAR_PADDING + 8) as i32,
        20,
        &display_text,
        Color::rgb(32, 35, 40),
        crate::style::FontWeight::Normal,
    );

    if let Some(message) = status_message {
        draw_text(
            frame,
            width,
            height,
            (ADDRESS_BAR_PADDING + 8) as i32,
            40,
            message,
            Color::rgb(96, 100, 110),
            crate::style::FontWeight::Normal,
        );
    }
}

fn rasterize(
    display_list: &DisplayList,
    frame: &mut [u8],
    width: u32,
    height: u32,
    scroll_y: u32,
) {
    for command in &display_list.commands {
        match command {
            DisplayCommand::FillRect {
                x,
                y,
                width: rect_width,
                height: rect_height,
                color,
            } => draw_rect(
                frame,
                width,
                height,
                *x as i32,
                *y as i32 - scroll_y as i32 + CHROME_HEIGHT as i32,
                *rect_width as i32,
                *rect_height as i32,
                *color,
            ),
            DisplayCommand::DrawText {
                x,
                y,
                text,
                color,
                font_weight,
            } => draw_text(
                frame,
                width,
                height,
                *x as i32,
                *y as i32 - scroll_y as i32 + CHROME_HEIGHT as i32,
                text,
                *color,
                *font_weight,
            ),
        }
    }

    draw_scrollbar(
        frame,
        width,
        height,
        display_list.height,
        scroll_y,
        CHROME_HEIGHT,
    );
}

fn draw_rect(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    rect_width: i32,
    rect_height: i32,
    color: Color,
) {
    let start_x = x.max(0) as u32;
    let start_y = y.max(0) as u32;
    let max_x = (x + rect_width).max(0) as u32;
    let max_y = (y + rect_height).max(0) as u32;
    let max_x = max_x.min(width);
    let max_y = max_y.min(height);

    for py in start_y..max_y {
        for px in start_x..max_x {
            set_pixel(frame, width, height, px as i32, py as i32, color);
        }
    }
}

fn draw_text(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    font_weight: crate::style::FontWeight,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if (bits >> col) & 1 == 1 {
                        let px = cursor_x + col;
                        let py = y + row as i32;
                        set_pixel(frame, width, height, px, py, color);
                        if font_weight == crate::style::FontWeight::Bold {
                            set_pixel(frame, width, height, px + 1, py, color);
                        }
                    }
                }
            }
        }
        cursor_x += CHAR_WIDTH as i32;
    }
}

fn draw_scrollbar(
    frame: &mut [u8],
    width: u32,
    height: u32,
    content_height: u32,
    scroll_y: u32,
    top_offset: u32,
) {
    let viewport_height = height.saturating_sub(top_offset);
    if content_height <= viewport_height || width < 12 || viewport_height == 0 {
        return;
    }

    let track_x = width.saturating_sub(10);
    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        top_offset as i32,
        6,
        viewport_height as i32,
        Color::rgb(228, 224, 214),
    );

    let thumb_height = ((viewport_height as f32 / content_height as f32) * viewport_height as f32)
        .round()
        .max(24.0) as u32;
    let travel = viewport_height.saturating_sub(thumb_height).max(1);
    let max_scroll = content_height.saturating_sub(viewport_height).max(1);
    let thumb_y = ((scroll_y as f32 / max_scroll as f32) * travel as f32).round() as u32;

    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        (top_offset + thumb_y) as i32,
        6,
        thumb_height as i32,
        Color::rgb(122, 128, 138),
    );
}

fn set_pixel(frame: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: Color) {
    if x < 0 || y < 0 {
        return;
    }

    let x = x as u32;
    let y = y as u32;
    if x >= width || y >= height {
        return;
    }

    let index = ((y * width + x) * 4) as usize;
    if let Some(pixel) = frame.get_mut(index..index + 4) {
        pixel[0] = color.r;
        pixel[1] = color.g;
        pixel[2] = color.b;
        pixel[3] = color.a;
    }
}

fn clamp_scroll(scroll_y: u32, content_height: u32, viewport_height: u32) -> u32 {
    scroll_y.min(content_height.saturating_sub(viewport_height))
}

fn apply_scroll_delta(
    current_scroll: u32,
    delta: i32,
    content_height: u32,
    viewport_height: u32,
) -> u32 {
    let next = if delta.is_negative() {
        current_scroll.saturating_sub(delta.unsigned_abs())
    } else {
        current_scroll.saturating_add(delta as u32)
    };
    clamp_scroll(next, content_height, viewport_height)
}

#[cfg(test)]
mod tests {
    use super::{apply_scroll_delta, clamp_scroll, load_page, rasterize, CHROME_HEIGHT};
    use crate::paint::{Color, DisplayCommand, DisplayList};

    #[test]
    fn clamps_scroll_to_content_height() {
        assert_eq!(clamp_scroll(10, 200, 300), 0);
        assert_eq!(clamp_scroll(500, 800, 300), 500);
        assert_eq!(clamp_scroll(900, 800, 300), 500);
    }

    #[test]
    fn applies_scroll_delta_with_bounds() {
        assert_eq!(apply_scroll_delta(0, 40, 1000, 200), 40);
        assert_eq!(apply_scroll_delta(10, -40, 1000, 200), 0);
        assert_eq!(apply_scroll_delta(900, 100, 1000, 200), 800);
    }

    #[test]
    fn rasterizes_with_scroll_offset() {
        let display_list = DisplayList {
            width: 120,
            height: 300,
            commands: vec![DisplayCommand::FillRect {
                x: 10,
                y: 40,
                width: 20,
                height: 10,
                color: Color::rgb(255, 0, 0),
            }],
            background: Color::rgb(255, 255, 255),
        };

        let mut frame = vec![255_u8; (120 * 90 * 4) as usize];
        rasterize(&display_list, &mut frame, 120, 90, 35);

        let pixel_index = (((CHROME_HEIGHT + 5) * 120 + 12) * 4) as usize;
        assert_eq!(frame[pixel_index], 255);
        assert_eq!(frame[pixel_index + 1], 0);
        assert_eq!(frame[pixel_index + 2], 0);
    }

    #[test]
    fn loads_page_fixture() {
        let page = load_page("examples/welcome.html").expect("fixture should load");
        assert!(!page.display_list.commands.is_empty());
    }
}
