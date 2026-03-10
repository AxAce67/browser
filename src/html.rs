use crate::dom::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    OpenTag {
        name: String,
        attributes: Vec<(String, String)>,
    },
    CloseTag(String),
    Text(String),
}

pub fn parse(input: &str) -> Node {
    let tokens = tokenize(input);
    Parser::new(tokens).parse_document()
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    let mut text_buffer = String::new();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            if !text_buffer.trim().is_empty() {
                tokens.push(Token::Text(normalize_whitespace(&text_buffer)));
            }
            text_buffer.clear();

            let mut tag = String::new();
            for next in chars.by_ref() {
                if next == '>' {
                    break;
                }
                tag.push(next);
            }

            let tag = tag.trim();
            if tag.is_empty() || tag.starts_with('!') {
                continue;
            }

            if let Some(stripped) = tag.strip_prefix('/') {
                tokens.push(Token::CloseTag(tag_name(stripped)));
            } else {
                let self_closing = tag.ends_with('/');
                let open_tag = parse_open_tag(tag.trim_end_matches('/'));
                let name = match &open_tag {
                    Token::OpenTag { name, .. } => name.clone(),
                    _ => String::new(),
                };
                if !name.is_empty() {
                    tokens.push(open_tag);
                    if self_closing || is_void_tag(&name) {
                        tokens.push(Token::CloseTag(name));
                    }
                }
            }
        } else {
            text_buffer.push(ch);
        }
    }

    if !text_buffer.trim().is_empty() {
        tokens.push(Token::Text(normalize_whitespace(&text_buffer)));
    }

    tokens
}

fn tag_name(raw: &str) -> String {
    raw.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn parse_open_tag(raw: &str) -> Token {
    let mut chars = raw.chars().peekable();
    let mut name = String::new();

    while let Some(ch) = chars.peek() {
        if ch.is_whitespace() {
            break;
        }
        name.push(*ch);
        chars.next();
    }

    let mut attributes = Vec::new();

    loop {
        skip_whitespace(&mut chars);
        if chars.peek().is_none() {
            break;
        }

        let mut key = String::new();
        while let Some(ch) = chars.peek() {
            if ch.is_whitespace() || *ch == '=' {
                break;
            }
            key.push(*ch);
            chars.next();
        }

        skip_whitespace(&mut chars);
        let mut value = String::new();

        if chars.peek() == Some(&'=') {
            chars.next();
            skip_whitespace(&mut chars);
            value = parse_attribute_value(&mut chars);
        }

        if !key.is_empty() {
            attributes.push((key.to_ascii_lowercase(), value));
        }
    }

    Token::OpenTag {
        name: name.to_ascii_lowercase(),
        attributes,
    }
}

fn parse_attribute_value<I>(chars: &mut std::iter::Peekable<I>) -> String
where
    I: Iterator<Item = char>,
{
    match chars.peek() {
        Some('"') | Some('\'') => {
            let quote = chars.next().unwrap_or('"');
            let mut value = String::new();
            for ch in chars.by_ref() {
                if ch == quote {
                    break;
                }
                value.push(ch);
            }
            value
        }
        Some(_) => {
            let mut value = String::new();
            while let Some(ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                value.push(*ch);
                chars.next();
            }
            value
        }
        None => String::new(),
    }
}

fn skip_whitespace<I>(chars: &mut std::iter::Peekable<I>)
where
    I: Iterator<Item = char>,
{
    while matches!(chars.peek(), Some(ch) if ch.is_whitespace()) {
        chars.next();
    }
}

fn normalize_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_void_tag(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img" | "meta" | "link" | "input")
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn parse_document(&mut self) -> Node {
        let children = self.parse_nodes(None);
        Node::document(children)
    }

    fn parse_nodes(&mut self, until: Option<&str>) -> Vec<Node> {
        let mut nodes = Vec::new();

        while let Some(token) = self.peek() {
            match token {
                Token::CloseTag(name) => {
                    if until == Some(name.as_str()) {
                        self.pos += 1;
                    }
                    break;
                }
                Token::OpenTag { name, attributes } => {
                    let name = name.clone();
                    let attributes = attributes.clone();
                    self.pos += 1;
                    let children = self.parse_nodes(Some(name.as_str()));
                    nodes.push(Node::element_with_attrs(name, attributes, children));
                }
                Token::Text(text) => {
                    let text = text.clone();
                    self.pos += 1;
                    nodes.push(Node::text(text));
                }
            }
        }

        nodes
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::dom::NodeType;

    #[test]
    fn parses_nested_elements() {
        let document = parse("<html><body><h1>Hello</h1><p>world</p></body></html>");
        assert_eq!(document.children.len(), 1);
        assert!(matches!(
            &document.children[0].node_type,
            NodeType::Element(data) if data.tag_name == "html"
        ));
        assert!(matches!(
            &document.children[0].children[0].node_type,
            NodeType::Element(data) if data.tag_name == "body"
        ));

        let h1 = &document.children[0].children[0].children[0];
        assert!(matches!(
            &h1.node_type,
            NodeType::Element(data) if data.tag_name == "h1"
        ));
        assert!(matches!(h1.children[0].node_type, NodeType::Text(_)));
    }

    #[test]
    fn parses_attributes() {
        let document = parse(r#"<div id="main" class="hero card" hidden>hello</div>"#);
        let NodeType::Element(data) = &document.children[0].node_type else {
            panic!("expected element");
        };
        assert_eq!(data.tag_name, "div");
        assert_eq!(
            data.attributes,
            vec![
                ("id".to_string(), "main".to_string()),
                ("class".to_string(), "hero card".to_string()),
                ("hidden".to_string(), String::new()),
            ]
        );
    }
}
