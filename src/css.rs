#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    pub tag_name: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub value: String,
}

pub fn parse(input: &str) -> Stylesheet {
    let mut rules = Vec::new();

    for block in input.split('}') {
        let Some((selectors, declarations)) = block.split_once('{') else {
            continue;
        };

        let selectors = selectors
            .split(',')
            .map(parse_selector)
            .filter(|selector| {
                selector.tag_name.is_some() || selector.id.is_some() || !selector.classes.is_empty()
            })
            .collect::<Vec<_>>();

        let declarations = declarations
            .split(';')
            .filter_map(|entry| {
                let (name, value) = entry.split_once(':')?;
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name.is_empty() || value.is_empty() {
                    None
                } else {
                    Some(Declaration { name, value })
                }
            })
            .collect::<Vec<_>>();

        if !selectors.is_empty() && !declarations.is_empty() {
            rules.push(Rule {
                selectors,
                declarations,
            });
        }
    }

    Stylesheet { rules }
}

fn parse_selector(input: &str) -> Selector {
    let mut tag_name = None;
    let mut id = None;
    let mut classes = Vec::new();
    let mut buffer = String::new();
    let mut mode = SelectorMode::Tag;

    for ch in input.trim().chars().chain(std::iter::once(' ')) {
        match ch {
            '#' => {
                flush_selector_part(&mut mode, &mut buffer, &mut tag_name, &mut id, &mut classes);
                mode = SelectorMode::Id;
            }
            '.' => {
                flush_selector_part(&mut mode, &mut buffer, &mut tag_name, &mut id, &mut classes);
                mode = SelectorMode::Class;
            }
            ch if ch.is_whitespace() => {
                flush_selector_part(&mut mode, &mut buffer, &mut tag_name, &mut id, &mut classes);
                mode = SelectorMode::Done;
            }
            _ => {
                if !matches!(mode, SelectorMode::Done) {
                    buffer.push(ch);
                }
            }
        }
    }

    Selector {
        tag_name,
        id,
        classes,
    }
}

fn flush_selector_part(
    mode: &mut SelectorMode,
    buffer: &mut String,
    tag_name: &mut Option<String>,
    id: &mut Option<String>,
    classes: &mut Vec<String>,
) {
    if buffer.is_empty() {
        return;
    }

    let value = std::mem::take(buffer).to_ascii_lowercase();
    match mode {
        SelectorMode::Tag => *tag_name = Some(value),
        SelectorMode::Id => *id = Some(value),
        SelectorMode::Class => classes.push(value),
        SelectorMode::Done => {}
    }
}

enum SelectorMode {
    Tag,
    Id,
    Class,
    Done,
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parses_simple_rules() {
        let stylesheet = parse(
            "
            h1, .hero { color: blue; font-weight: bold; }
            #main { display: none; }
            ",
        );

        assert_eq!(stylesheet.rules.len(), 2);
        assert_eq!(stylesheet.rules[0].selectors.len(), 2);
        assert_eq!(stylesheet.rules[0].declarations[0].name, "color");
        assert_eq!(stylesheet.rules[1].selectors[0].id.as_deref(), Some("main"));
    }
}
