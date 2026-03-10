use crate::paint::{Color, DisplayCommand, DisplayList, CHAR_WIDTH};
use font8x8::UnicodeFonts;
use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

pub fn run(display_list: DisplayList, title: &str) -> Result<(), String> {
    let event_loop =
        EventLoop::new().map_err(|err| format!("failed to create event loop: {err}"))?;
    let mut app = GuiApp::new(display_list, title);
    event_loop
        .run_app(&mut app)
        .map_err(|err| format!("failed to run GUI app: {err}"))
}

struct GuiApp {
    display_list: DisplayList,
    title: String,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    viewport_width: u32,
    viewport_height: u32,
    scroll_y: u32,
}

impl GuiApp {
    fn new(display_list: DisplayList, title: &str) -> Self {
        let viewport_width = display_list.width.clamp(480, 1024);
        let viewport_height = display_list.height.clamp(320, 720);
        Self {
            display_list,
            title: title.to_string(),
            window: None,
            pixels: None,
            viewport_width,
            viewport_height,
            scroll_y: 0,
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let Some(pixels) = self.pixels.as_mut() else {
            return Ok(());
        };

        let frame = pixels.frame_mut();
        clear_frame(frame, self.display_list.background);
        rasterize(
            &self.display_list,
            frame,
            self.viewport_width,
            self.viewport_height,
            self.scroll_y,
        );
        pixels
            .render()
            .map_err(|err| format!("failed to render frame: {err}"))
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
            .with_min_inner_size(LogicalSize::new(320.0, 240.0));

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
            self.display_list.height,
            self.viewport_height,
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
                    self.display_list.height,
                    self.viewport_height,
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
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_delta = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => (-(lines * 54.0)).round() as i32,
                    MouseScrollDelta::PixelDelta(position) => -position.y.round() as i32,
                };
                self.scroll_y = apply_scroll_delta(
                    self.scroll_y,
                    scroll_delta,
                    self.display_list.height,
                    self.viewport_height,
                );
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let next = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::ArrowDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        48,
                        self.display_list.height,
                        self.viewport_height,
                    )),
                    Key::Named(NamedKey::ArrowUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -48,
                        self.display_list.height,
                        self.viewport_height,
                    )),
                    Key::Named(NamedKey::PageDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        self.viewport_height as i32 - 48,
                        self.display_list.height,
                        self.viewport_height,
                    )),
                    Key::Named(NamedKey::PageUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -(self.viewport_height as i32 - 48),
                        self.display_list.height,
                        self.viewport_height,
                    )),
                    Key::Named(NamedKey::Home) => Some(0),
                    Key::Named(NamedKey::End) => Some(clamp_scroll(
                        u32::MAX,
                        self.display_list.height,
                        self.viewport_height,
                    )),
                    _ => None,
                };

                if let Some(next_scroll) = next {
                    self.scroll_y = next_scroll;
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }
}

fn clear_frame(frame: &mut [u8], color: Color) {
    for pixel in frame.chunks_exact_mut(4) {
        pixel[0] = color.r;
        pixel[1] = color.g;
        pixel[2] = color.b;
        pixel[3] = color.a;
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
                *y as i32 - scroll_y as i32,
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
                *y as i32 - scroll_y as i32,
                text,
                *color,
                *font_weight,
            ),
        }
    }

    draw_scrollbar(frame, width, height, display_list.height, scroll_y);
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

fn draw_scrollbar(frame: &mut [u8], width: u32, height: u32, content_height: u32, scroll_y: u32) {
    if content_height <= height || width < 12 {
        return;
    }

    let track_x = width.saturating_sub(10);
    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        0,
        6,
        height as i32,
        Color::rgb(228, 224, 214),
    );

    let thumb_height = ((height as f32 / content_height as f32) * height as f32)
        .round()
        .max(24.0) as u32;
    let travel = height.saturating_sub(thumb_height).max(1);
    let max_scroll = content_height.saturating_sub(height).max(1);
    let thumb_y = ((scroll_y as f32 / max_scroll as f32) * travel as f32).round() as u32;

    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        thumb_y as i32,
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
    use super::{apply_scroll_delta, clamp_scroll, rasterize};
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

        let mut frame = vec![255_u8; (120 * 40 * 4) as usize];
        rasterize(&display_list, &mut frame, 120, 40, 35);

        let pixel_index = ((5 * 120 + 12) * 4) as usize;
        assert_eq!(frame[pixel_index], 255);
        assert_eq!(frame[pixel_index + 1], 0);
        assert_eq!(frame[pixel_index + 2], 0);
    }
}
