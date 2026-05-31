use quick_xml::{Reader, events::Event};

use crate::storage::StorageError;

#[derive(Debug, Clone)]
pub(super) struct XmlNode {
    pub(super) name: String,
    pub(super) text: String,
    pub(super) line: u32,
    children: Vec<XmlNode>,
}

impl XmlNode {
    pub(super) fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|child| child.name == name)
    }

    pub(super) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XmlNode> {
        self.children.iter().filter(move |child| child.name == name)
    }

    pub(super) fn children(&self) -> &[XmlNode] {
        &self.children
    }
}

pub(super) fn parse_xml_document(content: &str) -> Result<Option<XmlNode>, StorageError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    let mut stack = Vec::<XmlNode>::new();
    let mut root = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let line = line_for_event_end(content, reader.buffer_position() as usize);
                stack.push(XmlNode {
                    name: String::from_utf8_lossy(event.name().as_ref()).into_owned(),
                    text: String::new(),
                    line,
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(event)) => {
                let line = line_for_event_end(content, reader.buffer_position() as usize);
                let node = XmlNode {
                    name: String::from_utf8_lossy(event.name().as_ref()).into_owned(),
                    text: String::new(),
                    line,
                    children: Vec::new(),
                };
                push_xml_node(&mut stack, &mut root, node);
            }
            Ok(Event::Text(event)) => {
                if let Some(node) = stack.last_mut() {
                    let text = event
                        .decode()
                        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
                    node.text.push_str(text.as_ref());
                }
            }
            Ok(Event::CData(event)) => {
                if let Some(node) = stack.last_mut() {
                    let text = event
                        .decode()
                        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
                    node.text.push_str(text.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                if let Some(node) = stack.pop() {
                    push_xml_node(&mut stack, &mut root, node);
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(StorageError::InvalidInput(error.to_string())),
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(StorageError::InvalidInput(
            "malformed XML document ended before closing all elements".to_owned(),
        ));
    }

    Ok(root)
}

fn push_xml_node(stack: &mut [XmlNode], root: &mut Option<XmlNode>, node: XmlNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        *root = Some(node);
    }
}

fn line_for_event_end(content: &str, end: usize) -> u32 {
    let bounded_end = end.min(content.len());
    let event_start = content.as_bytes()[..bounded_end]
        .iter()
        .rposition(|byte| *byte == b'<')
        .unwrap_or(bounded_end);
    content.as_bytes()[..event_start]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        .saturating_add(1) as u32
}
