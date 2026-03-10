mod css;
mod dom;
mod gui;
mod html;
mod layout;
mod paint;
mod renderer;
mod source;
mod style;

fn main() {
    let config = match parse_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(1);
        }
    };

    if config.gui {
        let requested_source = config
            .source
            .unwrap_or_else(|| source::DEFAULT_SOURCE.to_string());
        if let Err(message) = gui::run(&requested_source) {
            eprintln!("{message}");
            std::process::exit(1);
        }
        return;
    }

    let (html_input, source_path) = match source::load_html(config.source.as_deref()) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let document = html::parse(&html_input);
    let stylesheet = style::collect_stylesheets(&document);
    let styled = style::style_tree(&document, &stylesheet);
    let layout = layout::build_with_measurer(&styled, 48 * 8, &layout::MonospaceTextMeasurer);

    let _display_list = paint::build_display_list(&layout);
    let frame = renderer::render(&layout);
    println!("Source: {}", source_path.display());
    println!("{frame}");
}

struct RunConfig {
    gui: bool,
    source: Option<String>,
}

fn parse_args() -> Result<RunConfig, String> {
    let mut gui = false;
    let mut source = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--gui" => gui = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            _ => {
                if source.replace(arg).is_some() {
                    return Err("expected at most one HTML source".to_string());
                }
            }
        }
    }

    Ok(RunConfig { gui, source })
}

fn print_usage() {
    eprintln!("Usage: cargo run -- [--gui] [source]");
}
