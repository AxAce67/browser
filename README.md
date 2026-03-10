# browser

Minimal browser-engine scaffold in Rust.

Current milestone:

- Parse a small subset of HTML into a DOM tree
- Parse a small subset of CSS from `<style>` tags
- Build a simple styled block/text layout
- Build a display list from layout
- Render the result either as terminal text or a native window

Current limitations:

- CSS support is limited to `tag`, `.class`, `#id`
- Supported declarations are `color`, `background-color`, `display`, `font-weight`, and `width`
- GUI rendering is software-only and text uses an 8x8 bitmap font
- GUI scrolling is vertical only

This repository is intentionally starting without external dependencies so the
core parsing and rendering flow stays explicit.

## Layout

- `src/main.rs`: demo entrypoint
- `src/dom.rs`: DOM data model
- `src/html.rs`: tokenizer and parser for a small HTML subset
- `src/css.rs`: CSS parser for simple selectors and declarations
- `src/style.rs`: stylesheet extraction and computed styles
- `src/layout.rs`: block/text layout tree
- `src/paint.rs`: display-list generation
- `src/gui.rs`: native window rendering with `winit` and `pixels`
- `src/renderer.rs`: terminal renderer
- `src/source.rs`: local and remote HTML loader
- `examples/welcome.html`: default fixture

## Run

```bash
cargo run
cargo run -- examples/welcome.html
cargo run -- https://example.com
cargo run -- --gui
cargo run -- --gui examples/welcome.html
```

GUI controls:

- Mouse wheel: vertical scroll
- `ArrowUp` / `ArrowDown`: small scroll
- `PageUp` / `PageDown`: page scroll
- `Home` / `End`: jump to top/bottom

## Test

```bash
cargo test
```

Recommended checks:

1. `cargo test`
2. `cargo run`
3. `cargo run -- path/to/page.html`
4. `cargo run -- --gui`

Right now the test set covers:

- HTML parser unit tests
- CSS parser unit tests
- Style computation unit tests
- Text wrapping unit tests
- Display-list generation unit tests
- GUI scroll/clipping unit tests
- Renderer unit tests
- Local file loading
- HTTP loading through a tiny in-test server
- End-to-end fixture rendering from a local HTML file

## Next steps

1. Add scrolling and viewport clipping in the GUI path.
2. Expand CSS support beyond simple selectors and declarations.
3. Add a real browser chrome layer with URL input and navigation controls.
