use crate::html;
use crate::layout;
use crate::layout::TextMeasurer;
use crate::paint::{Color, DisplayCommand, DisplayList, CHAR_WIDTH};
use crate::source;
use crate::style;
use font8x8::UnicodeFonts;
use fontdb::{Database, Family, Query, Style, Weight};
use fontdue::{Font, FontSettings};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use pixels::{Pixels, SurfaceTexture};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

const CHROME_HEIGHT: u32 = 64;
const TOP_MARGIN: u32 = 20;
const SIDE_MARGIN: u32 = 32;
const SCROLLBAR_WIDTH: u32 = 10;
const ADDRESS_BAR_HEIGHT: u32 = 32;
const ADDRESS_BAR_PADDING: u32 = 12;
const DEFAULT_VIEWPORT_WIDTH: u32 = 960;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 720;
const MAX_CONTENT_WIDTH_PX: u32 = 1080;

pub fn run(initial_source: &str) -> Result<(), String> {
    let text_rasterizer = TextRasterizer::load();
    let initial_page = load_page(
        initial_source,
        DEFAULT_VIEWPORT_WIDTH,
        DEFAULT_VIEWPORT_HEIGHT,
        1.0,
        &text_rasterizer,
    )?;
    let event_loop =
        EventLoop::new().map_err(|err| format!("failed to create event loop: {err}"))?;
    let mut app = GuiApp::new(initial_page, initial_source, text_rasterizer);
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
    content_scale: u32,
    window_scale_factor: f64,
    scroll_y: u32,
    modifiers: ModifiersState,
    cursor_position: Option<PhysicalPosition<f64>>,
    text_rasterizer: TextRasterizer,
}

impl GuiApp {
    fn new(page: PageData, initial_source: &str, text_rasterizer: TextRasterizer) -> Self {
        let viewport_width = DEFAULT_VIEWPORT_WIDTH;
        let viewport_height = DEFAULT_VIEWPORT_HEIGHT;
        Self {
            title: initial_source.to_string(),
            page,
            current_source: initial_source.to_string(),
            address_input: initial_source.to_string(),
            address_focus: false,
            preedit_text: String::new(),
            status_message: Some(
                "Click or Cmd/Ctrl+L to edit address, Enter to load".to_string(),
            ),
            window: None,
            pixels: None,
            viewport_width,
            viewport_height,
            content_scale: content_scale_for_viewport(viewport_width, viewport_height, 1.0),
            window_scale_factor: 1.0,
            scroll_y: 0,
            modifiers: ModifiersState::empty(),
            cursor_position: None,
            text_rasterizer,
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let Some(pixels) = self.pixels.as_mut() else {
            return Ok(());
        };

        let frame = pixels.frame_mut();
        clear_frame(frame, Color::rgb(240, 236, 228));
        draw_chrome(
            &self.text_rasterizer,
            frame,
            self.viewport_width,
            self.viewport_height,
            self.content_scale,
            &self.address_input,
            &self.preedit_text,
            self.address_focus,
            self.status_message.as_deref(),
        );
        rasterize(
            &self.text_rasterizer,
            &self.page.display_list,
            frame,
            self.viewport_width,
            self.viewport_height,
            self.scroll_y,
            self.content_scale,
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

    fn set_address_focus(&mut self, focused: bool) {
        self.address_focus = focused;
        self.preedit_text.clear();
        if let Some(window) = self.window.as_ref() {
            window.set_ime_allowed(focused);
        }
    }

    fn relayout_current_page(&mut self) {
        self.content_scale = content_scale_for_viewport(
            self.viewport_width,
            self.viewport_height,
            self.window_scale_factor,
        );
        self.page.display_list = build_page_display_list(
            &self.page.html_input,
            self.viewport_width,
            self.viewport_height,
            self.window_scale_factor,
            &self.text_rasterizer,
        );
        self.scroll_y = clamp_scroll(
            self.scroll_y,
            self.page.display_list.height,
            self.content_viewport_height(),
        );
    }

    fn navigate(&mut self, source_input: String) {
        match load_page(
            &source_input,
            self.viewport_width,
            self.viewport_height,
            self.window_scale_factor,
            &self.text_rasterizer,
        ) {
            Ok(page) => {
                self.page = page;
                self.current_source = source_input.clone();
                self.address_input = source_input.clone();
                self.title = source_input;
                self.scroll_y = 0;
                self.status_message = Some("Page loaded".to_string());
                self.set_address_focus(false);
                if let Some(window) = self.window.as_ref() {
                    window.set_title(&format!("Toy Browser - {}", self.title));
                }
            }
            Err(err) => {
                self.status_message = Some(err);
            }
        }
        self.request_redraw();
    }

    fn content_viewport_height(&self) -> u32 {
        let available_pixels = self
            .viewport_height
            .saturating_sub(chrome_height(self.content_scale) + top_margin(self.content_scale) * 2)
            .max(self.content_scale);
        (available_pixels / self.content_scale).max(1)
    }

    fn handle_address_bar_click(&mut self, position: PhysicalPosition<f64>) {
        let focused = address_bar_hit_test(
            position.x,
            position.y,
            self.viewport_width,
            self.content_scale,
        );
        if focused {
            self.set_address_focus(true);
            self.status_message = Some("Editing address".to_string());
        } else if self.address_focus {
            self.set_address_focus(false);
            self.status_message = Some("Address bar unfocused".to_string());
        }
        self.request_redraw();
    }
}

struct TextRasterizer {
    regular: Option<Font>,
    bold: Option<Font>,
}

impl TextRasterizer {
    fn load() -> Self {
        let mut database = Database::new();
        database.load_system_fonts();

        Self {
            regular: load_font(&database, Weight::NORMAL),
            bold: load_font(&database, Weight::BOLD)
                .or_else(|| load_font(&database, Weight::NORMAL)),
        }
    }

    fn draw_text(
        &self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        text: &str,
        color: Color,
        font_weight: crate::style::FontWeight,
        font_size: f32,
    ) {
        let font = match font_weight {
            crate::style::FontWeight::Bold => self.bold.as_ref().or(self.regular.as_ref()),
            crate::style::FontWeight::Normal => self.regular.as_ref().or(self.bold.as_ref()),
        };

        if let Some(font) = font {
            draw_text_fontdue(frame, width, height, x, y, text, color, font, font_size);
        } else {
            let bitmap_scale = if font_size >= 24.0 { 2 } else { 1 };
            draw_text_bitmap(
                frame,
                width,
                height,
                x,
                y,
                text,
                color,
                font_weight,
                bitmap_scale,
            );
        }
    }

    fn font_for_weight(&self, font_weight: crate::style::FontWeight) -> Option<&Font> {
        match font_weight {
            crate::style::FontWeight::Bold => self.bold.as_ref().or(self.regular.as_ref()),
            crate::style::FontWeight::Normal => self.regular.as_ref().or(self.bold.as_ref()),
        }
    }

    fn measure_text_width(
        &self,
        text: &str,
        font_weight: crate::style::FontWeight,
        font_size: usize,
    ) -> usize {
        let font_size = font_size.max(1) as f32;
        if let Some(font) = self.font_for_weight(font_weight) {
            text.chars()
                .map(|ch| font.metrics(ch, font_size).advance_width.max(0.0))
                .sum::<f32>()
                .ceil() as usize
        } else {
            text.chars().count() * CHAR_WIDTH as usize
        }
    }
}

impl TextMeasurer for TextRasterizer {
    fn measure_text(&self, text: &str, font_size: usize, font_weight: crate::style::FontWeight) -> usize {
        self.measure_text_width(text, font_weight, font_size)
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
            .with_min_inner_size(LogicalSize::new(480.0, 360.0));

        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        let window_size = window.inner_size();
        self.window_scale_factor = window.scale_factor();
        self.viewport_width = window_size.width.max(1);
        self.viewport_height = window_size.height.max(1);
        self.relayout_current_page();

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

        window.set_ime_allowed(false);
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
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some(position);
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => {
                if let Some(position) = self.cursor_position {
                    self.handle_address_bar_click(position);
                }
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
                self.relayout_current_page();

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
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.window_scale_factor = scale_factor;
                self.relayout_current_page();
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } if !self.address_focus => {
                let scroll_delta = match delta {
                    MouseScrollDelta::LineDelta(_, lines) => (-(lines * 18.0)).round() as i32,
                    MouseScrollDelta::PixelDelta(position) => {
                        -(position.y.round() as i32) / self.content_scale.max(1) as i32
                    }
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
                    self.set_address_focus(true);
                    self.address_input = self.current_source.clone();
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
                            self.set_address_focus(false);
                            self.address_input = self.current_source.clone();
                            self.status_message = Some("Address edit cancelled".to_string());
                            self.request_redraw();
                        }
                        Key::Named(NamedKey::Backspace) => {
                            self.address_input.pop();
                            self.request_redraw();
                        }
                        Key::Character(text)
                            if !(self.modifiers.control_key() || self.modifiers.super_key()) =>
                        {
                            self.address_input.push_str(text);
                            self.request_redraw();
                        }
                        _ => {}
                    }
                    return;
                }

                let next = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::ArrowDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        24,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::ArrowUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -24,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::PageDown) => Some(apply_scroll_delta(
                        self.scroll_y,
                        self.content_viewport_height() as i32 - 8,
                        self.page.display_list.height,
                        self.content_viewport_height(),
                    )),
                    Key::Named(NamedKey::PageUp) => Some(apply_scroll_delta(
                        self.scroll_y,
                        -(self.content_viewport_height() as i32 - 8),
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
    html_input: String,
    display_list: DisplayList,
}

fn load_page(
    source_input: &str,
    viewport_width: u32,
    viewport_height: u32,
    scale_factor: f64,
    text_measurer: &dyn TextMeasurer,
) -> Result<PageData, String> {
    let (html_input, _) = source::load_html(Some(source_input))?;
    let display_list = build_page_display_list(
        &html_input,
        viewport_width,
        viewport_height,
        scale_factor,
        text_measurer,
    );
    Ok(PageData {
        html_input,
        display_list,
    })
}

fn build_page_display_list(
    html_input: &str,
    viewport_width: u32,
    viewport_height: u32,
    scale_factor: f64,
    text_measurer: &dyn TextMeasurer,
) -> DisplayList {
    let content_scale = content_scale_for_viewport(viewport_width, viewport_height, scale_factor);
    let available_width = viewport_width
        .saturating_sub(SIDE_MARGIN * 2 + SCROLLBAR_WIDTH + 18)
        .min(MAX_CONTENT_WIDTH_PX)
        .max(420);
    let layout_width = (available_width / content_scale.max(1)) as usize;

    let document = html::parse(html_input);
    let stylesheet = style::collect_stylesheets(&document);
    let styled = style::style_tree(&document, &stylesheet);
    let layout = layout::build_with_measurer(&styled, layout_width.max(180), text_measurer);
    crate::paint::build_display_list(&layout)
}

fn content_scale_for_viewport(viewport_width: u32, viewport_height: u32, scale_factor: f64) -> u32 {
    let device_scale = scale_factor.round().clamp(1.0, 4.0) as u32;
    let logical_width = (viewport_width as f64 / scale_factor.max(1.0)).round() as u32;
    let logical_height = (viewport_height as f64 / scale_factor.max(1.0)).round() as u32;
    let large_viewport_scale = if logical_width >= 2200 || logical_height >= 1500 {
        2
    } else {
        1
    };
    device_scale.saturating_mul(large_viewport_scale)
}

fn load_font(database: &Database, weight: Weight) -> Option<Font> {
    let query = Query {
        families: &[Family::SansSerif],
        weight,
        style: Style::Normal,
        ..Query::default()
    };
    let id = database.query(&query)?;
    let font = database.with_face_data(id, |data, face_index| {
        Font::from_bytes(
            data.to_vec(),
            FontSettings {
                collection_index: face_index,
                ..FontSettings::default()
            },
        )
    })?;
    font.ok()
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
    text_rasterizer: &TextRasterizer,
    frame: &mut [u8],
    width: u32,
    height: u32,
    scale: u32,
    address_input: &str,
    preedit_text: &str,
    address_focus: bool,
    status_message: Option<&str>,
) {
    draw_rect(
        frame,
        width,
        height,
        0,
        0,
        width as i32,
        chrome_height(scale) as i32,
        Color::rgb(232, 228, 220),
    );
    draw_rect(
        frame,
        width,
        height,
        address_bar_padding(scale) as i32,
        (12 * scale) as i32,
        width.saturating_sub(address_bar_padding(scale) * 2) as i32,
        address_bar_height(scale) as i32,
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
    text_rasterizer.draw_text(
        frame,
        width,
        height,
        (address_bar_padding(scale) + 8 * scale) as i32,
        (19 * scale) as i32,
        &display_text,
        Color::rgb(32, 35, 40),
        crate::style::FontWeight::Normal,
        17.0 * scale as f32,
    );

    if let Some(message) = status_message {
        text_rasterizer.draw_text(
            frame,
            width,
            height,
            (address_bar_padding(scale) + 8 * scale) as i32,
            (47 * scale) as i32,
            message,
            Color::rgb(96, 100, 110),
            crate::style::FontWeight::Normal,
            13.0 * scale as f32,
        );
    }
}

fn rasterize(
    text_rasterizer: &TextRasterizer,
    display_list: &DisplayList,
    frame: &mut [u8],
    width: u32,
    height: u32,
    scroll_y: u32,
    scale: u32,
) {
    let content_pixel_width = display_list.width.saturating_mul(scale);
    let origin_x = compute_content_origin_x(width, content_pixel_width) as i32;
    let origin_y = (chrome_height(scale) + top_margin(scale)) as i32;

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
                origin_x + (*x as i32 * scale as i32),
                origin_y + ((*y as i32 - scroll_y as i32) * scale as i32),
                *rect_width as i32 * scale as i32,
                *rect_height as i32 * scale as i32,
                *color,
            ),
            DisplayCommand::DrawText {
                x,
                y,
                text,
                color,
                font_weight,
                font_size,
            } => text_rasterizer.draw_text(
                frame,
                width,
                height,
                origin_x + (*x as i32 * scale as i32),
                origin_y + ((*y as i32 - scroll_y as i32) * scale as i32),
                text,
                *color,
                *font_weight,
                *font_size as f32 * scale as f32,
            ),
        }
    }

    draw_scrollbar(
        frame,
        width,
        height,
        display_list.height,
        scroll_y,
        content_viewport_height_for(height, scale),
        scale,
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

fn draw_text_bitmap(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    font_weight: crate::style::FontWeight,
    scale: u32,
) {
    let mut cursor_x = x;
    for ch in text.chars() {
        if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if (bits >> col) & 1 == 1 {
                        let px = cursor_x + col * scale as i32;
                        let py = y + row as i32 * scale as i32;
                        draw_rect(
                            frame,
                            width,
                            height,
                            px,
                            py,
                            scale as i32,
                            scale as i32,
                            color,
                        );
                        if font_weight == crate::style::FontWeight::Bold {
                            draw_rect(
                                frame,
                                width,
                                height,
                                px + scale as i32,
                                py,
                                scale as i32,
                                scale as i32,
                                color,
                            );
                        }
                    }
                }
            }
        }
        cursor_x += (CHAR_WIDTH * scale) as i32;
    }
}

fn draw_text_fontdue(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    text: &str,
    color: Color,
    font: &Font,
    font_size: f32,
) {
    let fonts = [font];
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    let line_height = if font_size >= 28.0 {
        1.1
    } else {
        1.35
    };
    layout.reset(&LayoutSettings {
        x: x as f32,
        y: y as f32,
        line_height,
        ..LayoutSettings::default()
    });
    layout.append(&fonts, &TextStyle::new(text, font_size, 0));

    for glyph in layout.glyphs() {
        let (metrics, bitmap) = font.rasterize_config(glyph.key);
        draw_glyph_bitmap(
            frame,
            width,
            height,
            glyph.x.round() as i32,
            glyph.y.round() as i32,
            metrics.width,
            metrics.height,
            &bitmap,
            color,
        );
    }
}

fn draw_glyph_bitmap(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    glyph_width: usize,
    glyph_height: usize,
    bitmap: &[u8],
    color: Color,
) {
    for row in 0..glyph_height {
        for col in 0..glyph_width {
            let coverage = bitmap[row * glyph_width + col];
            if coverage == 0 {
                continue;
            }

            blend_pixel(
                frame,
                width,
                height,
                x + col as i32,
                y + row as i32,
                color,
                coverage,
            );
        }
    }
}

fn draw_scrollbar(
    frame: &mut [u8],
    width: u32,
    height: u32,
    content_height: u32,
    scroll_y: u32,
    viewport_height: u32,
    scale: u32,
) {
    if content_height <= viewport_height || width < 12 || viewport_height == 0 {
        return;
    }

    let track_x = width.saturating_sub(scaled(14, scale));
    let track_y = chrome_height(scale) + top_margin(scale);
    let track_height = height.saturating_sub(track_y + top_margin(scale));

    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        track_y as i32,
        scaled(SCROLLBAR_WIDTH, scale.min(2)) as i32,
        track_height as i32,
        Color::rgb(228, 224, 214),
    );

    let thumb_height = ((viewport_height as f32 / content_height as f32) * track_height as f32)
        .round()
        .max(28.0) as u32;
    let travel = track_height.saturating_sub(thumb_height).max(1);
    let max_scroll = content_height.saturating_sub(viewport_height).max(1);
    let thumb_y = ((scroll_y as f32 / max_scroll as f32) * travel as f32).round() as u32;

    draw_rect(
        frame,
        width,
        height,
        track_x as i32,
        (track_y + thumb_y) as i32,
        scaled(SCROLLBAR_WIDTH, scale.min(2)) as i32,
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

fn blend_pixel(
    frame: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    color: Color,
    coverage: u8,
) {
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
        let alpha = coverage as f32 / 255.0;
        pixel[0] = ((1.0 - alpha) * pixel[0] as f32 + alpha * color.r as f32).round() as u8;
        pixel[1] = ((1.0 - alpha) * pixel[1] as f32 + alpha * color.g as f32).round() as u8;
        pixel[2] = ((1.0 - alpha) * pixel[2] as f32 + alpha * color.b as f32).round() as u8;
        pixel[3] = 0xff;
    }
}

fn compute_content_origin_x(viewport_width: u32, content_pixel_width: u32) -> u32 {
    if viewport_width <= content_pixel_width + SIDE_MARGIN * 2 {
        SIDE_MARGIN
    } else {
        (viewport_width - content_pixel_width) / 2
    }
}

fn content_viewport_height_for(viewport_height: u32, scale: u32) -> u32 {
    let available_pixels = viewport_height
        .saturating_sub(chrome_height(scale) + top_margin(scale) * 2)
        .max(scale);
    (available_pixels / scale).max(1)
}

fn address_bar_hit_test(x: f64, y: f64, viewport_width: u32, scale: u32) -> bool {
    let min_x = address_bar_padding(scale) as f64;
    let max_x = viewport_width.saturating_sub(address_bar_padding(scale)) as f64;
    let min_y = (12 * scale) as f64;
    let max_y = (12 * scale + address_bar_height(scale)) as f64;
    x >= min_x && x <= max_x && y >= min_y && y <= max_y
}

fn chrome_height(scale: u32) -> u32 {
    scaled(CHROME_HEIGHT, scale)
}

fn top_margin(scale: u32) -> u32 {
    scaled(TOP_MARGIN, scale)
}

fn address_bar_height(scale: u32) -> u32 {
    scaled(ADDRESS_BAR_HEIGHT, scale)
}

fn address_bar_padding(scale: u32) -> u32 {
    scaled(ADDRESS_BAR_PADDING, scale)
}

fn scaled(value: u32, scale: u32) -> u32 {
    value.saturating_mul(scale.max(1))
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
    use super::{
        address_bar_hit_test, apply_scroll_delta, chrome_height, clamp_scroll,
        content_scale_for_viewport, load_page, rasterize, top_margin, TextRasterizer,
    };
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
    fn scales_up_for_large_viewports() {
        assert_eq!(content_scale_for_viewport(800, 600, 1.0), 1);
        assert_eq!(content_scale_for_viewport(1400, 900, 1.0), 1);
        assert_eq!(content_scale_for_viewport(2400, 1600, 1.0), 2);
        assert_eq!(content_scale_for_viewport(1920, 1440, 2.0), 2);
    }

    #[test]
    fn detects_address_bar_hits() {
        assert!(address_bar_hit_test(30.0, 20.0, 900, 1));
        assert!(!address_bar_hit_test(30.0, 90.0, 900, 1));
        assert!(address_bar_hit_test(60.0, 40.0, 1800, 2));
    }

    #[test]
    fn rasterizes_with_scroll_offset() {
        let text_rasterizer = TextRasterizer::load();
        let scale = 2;
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

        let mut frame = vec![255_u8; (240 * 280 * 4) as usize];
        rasterize(&text_rasterizer, &display_list, &mut frame, 240, 280, 35, scale);

        let pixel_y = chrome_height(scale) + top_margin(scale) + ((40 - 35) as u32 * scale) + 4;
        let pixel_index = ((pixel_y * 240 + 72) * 4) as usize;
        assert_eq!(frame[pixel_index], 255);
        assert_eq!(frame[pixel_index + 1], 0);
        assert_eq!(frame[pixel_index + 2], 0);
    }

    #[test]
    fn loads_page_fixture() {
        let text_rasterizer = TextRasterizer::load();
        let page = load_page("examples/welcome.html", 960, 720, 1.0, &text_rasterizer)
            .expect("fixture should load");
        assert!(!page.display_list.commands.is_empty());
    }
}
