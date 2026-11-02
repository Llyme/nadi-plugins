//! What the browser extension said about a page besides its nodes, as the classifier reads it (the island passes it on as it came).

use serde::Deserialize;

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct ExtMeta {
    pub lang: Option<String>,
    pub site: Option<String>,
    #[serde(rename = "ogType")]
    pub og_type: Option<String>,
    pub author: Option<String>,
    pub published: Option<String>,
    pub modified: Option<String>,
    pub section: Option<String>,
    pub generator: Option<String>,
    pub description: Option<String>,
    pub price: Option<String>,
    pub currency: Option<String>,
    pub canonical: Option<String>,
    /// (label, value) rows of a wiki infobox
    pub facts: Vec<(String, String)>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Rating {
    pub value: Option<f64>,
    pub count: Option<u64>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Offer {
    pub price: Option<String>,
    pub currency: Option<String>,
    pub availability: Option<String>,
}

/// one JSON-LD item of the page, reduced to the fields that matter
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct LdItem {
    #[serde(rename = "type")]
    pub types: Vec<String>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub published: Option<String>,
    pub modified: Option<String>,
    pub section: Option<String>,
    pub words: Option<u32>,
    pub publisher: Option<String>,
    /// minutes
    pub total: Option<u32>,
    pub prep: Option<u32>,
    pub cook: Option<u32>,
    #[serde(rename = "yield")]
    pub yields: Option<String>,
    pub ingredients: Option<u32>,
    pub steps: Vec<String>,
    pub rating: Option<Rating>,
    pub offer: Option<Offer>,
    pub brand: Option<String>,
    /// minutes
    pub length: Option<u32>,
    pub uploaded: Option<String>,
    pub answers: Option<u32>,
    pub votes: Option<i64>,
    pub accepted: Option<String>,
    pub questions: Option<u32>,
    pub crumbs: Vec<String>,
}

impl LdItem {
    pub fn is(&self, t: &str) -> bool {
        self.types.iter().any(|x| x == t)
    }
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct ExtVideo {
    pub cur: f32,
    pub dur: f32,
    pub paused: bool,
    pub ended: bool,
    pub rate: f32,
}

/// What the island tells of a guide: its key (the island fetches the words by it) and the headings that say where you are.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct GuideLite {
    pub key: String,
    pub text: bool,
    pub heads: Vec<String>,
}

/// everything the extension said about a page besides the nodes
#[derive(Clone, Default, Debug)]
pub struct ExtData {
    pub guide: Option<GuideLite>,
    pub meta: ExtMeta,
    pub ld: Vec<LdItem>,
    pub video: Option<ExtVideo>,
    pub words: u32,
}
