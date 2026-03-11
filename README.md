# browser

Minimal browser-engine scaffold in Rust.

Current milestone:

- Parse a small subset of HTML into a DOM tree
- Parse a small subset of CSS from `<style>` tags
- Build a simple styled block/text layout
- Build a display list from layout
- Render the result as terminal text, a toy native window, or a WebView shell

Current limitations:

- CSS support is limited to `tag`, `.class`, `#id`
- Supported declarations are `color`, `background-color`, `display`, `font-weight`, `text-decoration`, `font-size`, `line-height`, `width`, `margin`, `padding`, and their side-specific variants
- The toy GUI is software-only and text is rasterized from system fonts
- The WebView mode borrows the page engine from the OS WebView and keeps the browser chrome in Rust
- Only the toy GUI uses the custom renderer; modern sites should be opened in `--webview`

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
- `src/webview.rs`: browser shell with a child `wry` WebView
- `src/renderer.rs`: terminal renderer
- `src/source.rs`: local and remote HTML loader
- `examples/welcome.html`: default fixture

## Run

```bash
cargo run
cargo run -- examples/welcome.html
cargo run -- https://example.com
cargo run -- --gui
cargo run -- --toy examples/welcome.html
cargo run -- --webview
cargo run -- --webview https://example.com
cargo run -- --webview motherfuckingwebsite.com
cargo run -- --gui examples/welcome.html
```

Toy GUI controls:

- Click address bar: focus it
- Drag in address bar: select text
- Click link text: navigate to its `href`
- Mouse wheel: vertical scroll
- `ArrowUp` / `ArrowDown`: small scroll
- `PageUp` / `PageDown`: page scroll
- `Home` / `End`: jump to top/bottom
- `Cmd+L` / `Ctrl+L`: focus address bar
- `Cmd+A` / `Ctrl+A`: select whole address
- `Cmd+C` / `Ctrl+C`: copy address
- `Cmd+X` / `Ctrl+X`: cut address
- `Cmd+V` / `Ctrl+V`: paste into address bar
- `ArrowLeft` / `ArrowRight`: move address caret
- `Shift+ArrowLeft` / `Shift+ArrowRight`: expand address selection
- `Delete` / `Backspace`: delete in address bar
- `Home` / `End` while editing: move caret to start/end
- `Enter`: load typed path or URL
- `Esc`: cancel address editing
- `R`: reload current page

WebView controls:

- Click address bar: focus it
- Drag in address bar: select text
- `Cmd+L` / `Ctrl+L`: focus address bar
- `Cmd+A` / `Ctrl+A`: select whole address
- `Cmd+C` / `Ctrl+C`: copy address
- `Cmd+X` / `Ctrl+X`: cut address
- `Cmd+V` / `Ctrl+V`: paste into address bar
- `ArrowLeft` / `ArrowRight`: move address caret
- `Shift+ArrowLeft` / `Shift+ArrowRight`: expand address selection
- `Delete` / `Backspace`: delete in address bar
- `Home` / `End` while editing: move caret to start/end
- `Enter`: load typed path or URL
- `Esc`: cancel address editing
- `Cmd+R` / `Ctrl+R`: reload current page
- `Alt+Left`: go back
- `Alt+Right`: go forward

## Test

```bash
cargo test
```

Recommended checks:

1. `cargo test`
2. `cargo run`
3. `cargo run -- path/to/page.html`
4. `cargo run -- --gui`
5. `cargo run -- --webview https://example.com`

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

1. Add basic navigation history (`Back` / `Forward`).
2. Add browser chrome buttons for WebView mode.
3. Expand toy-renderer CSS support beyond simple selectors and declarations.
