//! "What is on the page": the summary of the page and the buttons of its own that are safe to press from the
//! island. The page comes from the browser extension (see `ext`), which presses a button by its text.

use serde::Serialize;

#[derive(Serialize, Clone, Default)]
pub struct PagePreview {
    /// the opening of the page's main text
    pub lead: Option<String>,
    /// the page's buttons that are safe to press from the island
    pub buttons: Vec<PageButton>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PageButton {
    pub label: String,
}

/// Buttons that are safe to press on someone's behalf. Anything that spends
/// money, sends, posts, deletes or signs in is deliberately not on the list.
pub const SAFE_BUTTONS: &[&str] = &[
    "share", "reply", "join", "follow", "subscribe", "save", "like", "upvote", "play", "pause", "next", "previous",
    "download", "show more", "load more", "read more", "see more", "view more", "comments", "copy link",
    "bookmark", "expand", "collapse", "skip", "watch later", "open in app",
];

fn button_key(text: &str) -> String {
    text.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// The summary and the safe buttons of a page the browser extension described. The ones on screen come first.
pub fn preview_from_ext(page: &crate::page::Page) -> Option<PagePreview> {
    use crate::page::NodeKind;
    let lead = crate::kind::lead(&page.nodes);
    let mut found: Vec<PageButton> = Vec::new();
    let mut buttons: Vec<&crate::page::Node> = page.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Button)).collect();
    buttons.sort_by_key(|n| !(n.top >= 0.0 && n.bottom <= page.view.1));
    for n in buttons {
        let key = button_key(&n.name);
        if !SAFE_BUTTONS.contains(&key.as_str()) || found.iter().any(|b| button_key(&b.label) == key) {
            continue;
        }
        let mut label = key.clone();
        if let Some(c) = label.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        found.push(PageButton { label });
    }
    found.truncate(5);
    (lead.is_some() || !found.is_empty()).then_some(PagePreview { lead, buttons: found })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{Node, NodeKind, Page};

    fn node(kind: NodeKind, name: &str, top: f32) -> Node {
        Node { kind, name: name.to_string(), left: 0.0, top, right: 100.0, bottom: top + 20.0, block: true }
    }

    #[test]
    fn only_safe_buttons_are_offered_on_screen_first() {
        let page = Page {
            nodes: vec![
                node(NodeKind::Button, "Buy now", 10.0),
                node(NodeKind::Button, "Share", 5000.0),
                node(NodeKind::Button, "Subscribe", 100.0),
                node(NodeKind::Button, "subscribe", 200.0),
            ],
            view: (0.0, 900.0),
            ..Default::default()
        };
        let p = preview_from_ext(&page).unwrap();
        let labels: Vec<_> = p.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["Subscribe", "Share"]);
    }

    #[test]
    fn nothing_to_say_gives_nothing() {
        assert!(preview_from_ext(&Page::default()).is_none());
    }
}
