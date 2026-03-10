#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Document,
    Element(ElementData),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementData {
    pub tag_name: String,
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub node_type: NodeType,
    pub children: Vec<Node>,
}

impl Node {
    pub fn document(children: Vec<Node>) -> Self {
        Self {
            node_type: NodeType::Document,
            children,
        }
    }

    #[allow(dead_code)]
    pub fn element(name: impl Into<String>, children: Vec<Node>) -> Self {
        Self::element_with_attrs(name, Vec::new(), children)
    }

    pub fn element_with_attrs(
        name: impl Into<String>,
        attributes: Vec<(String, String)>,
        children: Vec<Node>,
    ) -> Self {
        Self {
            node_type: NodeType::Element(ElementData {
                tag_name: name.into(),
                attributes,
            }),
            children,
        }
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self {
            node_type: NodeType::Text(value.into()),
            children: Vec::new(),
        }
    }
}
