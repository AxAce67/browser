#[path = "../src/css.rs"]
mod css;
#[path = "../src/dom.rs"]
mod dom;
#[path = "../src/html.rs"]
mod html;
#[path = "../src/layout.rs"]
mod layout;
#[path = "../src/renderer.rs"]
mod renderer;
#[path = "../src/source.rs"]
mod source;
#[path = "../src/style.rs"]
mod style;

#[test]
fn renders_fixture_html() {
    let (html_input, path) =
        source::load_html(Some("examples/welcome.html")).expect("fixture should load");
    let document = html::parse(&html_input);
    let stylesheet = style::collect_stylesheets(&document);
    let styled = style::style_tree(&document, &stylesheet);
    let layout = layout::build(&styled, 48);
    let frame = renderer::render(&layout);

    assert!(path.ends_with("examples/welcome.html"));
    assert!(frame.contains("<h1 color=navy weight=bold>"));
    assert!(frame.contains("Toy Browser"));
    assert!(frame.contains("<p bg=beige>"));
    assert!(frame.contains("Next steps: CSS parsing"));
    assert!(!frame.contains("This line should stay hidden."));
}
