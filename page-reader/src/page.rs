//! The shape of a page as the classifier reads it: headings, links, buttons and text with their positions, and
//! how far down the page you are. The browser extension fills it (see `ext`); `pagekind` reads it.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeKind {
    /// a heading of the page, with its level (1 to 6)
    Heading(u8),
    Link,
    Button,
    Text,
    Item,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub name: String,
    /// screen position of the node (physical px); off-screen nodes may carry the browser's own guess
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    /// a whole paragraph already (from the browser extension), not a run of text to be joined with its neighbours
    pub block: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    /// the address bar's text, as far as it looks like an address
    pub url: Option<String>,
    pub nodes: Vec<Node>,
    /// the visible part of the page on screen: (top, bottom)
    pub view: (f32, f32),
}
