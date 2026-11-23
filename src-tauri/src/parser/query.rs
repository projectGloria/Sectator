//! Power-user query DSL: the structured form of what the user typed.

use serde::{Deserialize, Serialize};

use crate::models::{Category, SourceType};

/// Comparison used by `seeds:` and `size:` operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Cmp {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NumFilter {
    pub cmp: Cmp,
    pub value: u64,
}

/// Parsed search: free terms plus every recognised operator.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    /// Plain terms (lowercased), matched as tokens.
    pub terms: Vec<String>,
    /// `"exact phrase"` (lowercased), matched contiguously.
    pub phrases: Vec<String>,
    /// `-term`
    pub excluded: Vec<String>,
    /// `file:<pattern>`
    pub file_patterns: Vec<String>,
    /// `cat:`
    pub category: Option<Category>,
    /// `res:`
    pub resolutions: Vec<String>,
    /// `codec:`
    pub codecs: Vec<String>,
    /// `src:` (release source such as remux / web-dl)
    pub release_sources: Vec<String>,
    /// `seeds:`
    pub seeds: Option<NumFilter>,
    /// `size:` (bytes); may hold a lower and an upper bound.
    pub sizes: Vec<NumFilter>,
    /// `site:`
    pub sites: Vec<String>,
    /// `source:`
    pub source_types: Vec<SourceType>,
}
