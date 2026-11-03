use crate::chroma::ChromaHandle;
use crate::embedding::EmbeddingEngine;
use crate::error::AppError;
use crate::project::hex_prefix;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thirtyfour::WebDriver;

// Walks the interactive elements design.md 8.1 lists (a, button, input,
// select, textarea, [role], [onclick], [tabindex]) and, for each, computes a
// CSS path and XPath by hand-walking parentElement — the only place DOM
// structure is cheaply available; resolving one element at a time through
// WebDriver round-trips would be far slower for a page with many elements.
pub const INDEX_SCRIPT: &str = include_str!("index_page.js");

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Bbox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct RawIndexedElement {
    tag: String,
    #[serde(default)]
    input_type: Option<String>,
    #[serde(default)]
    editable: bool,
    #[serde(default)]
    role: Option<String>,
    text: String,
    aria_label: Option<String>,
    placeholder: Option<String>,
    name_or_id: Option<String>,
    css: String,
    xpath: String,
    bbox: Bbox,
    visible: bool,
}

pub fn normalize_url(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    url[..end].to_string()
}

pub fn element_id(normalized_url: &str, css_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(normalized_url.as_bytes());
    hasher.update(css_path.as_bytes());
    let digest = hasher.finalize();
    hex_prefix(&digest, 16)
}

fn compose_document(el: &RawIndexedElement, normalized_url: &str) -> String {
    let mut doc = el.tag.clone();
    if !el.text.is_empty() {
        doc.push_str(&format!(" \"{}\"", el.text));
    }
    let descriptor = el
        .aria_label
        .as_ref()
        .filter(|s| !s.is_empty())
        .or_else(|| el.placeholder.as_ref().filter(|s| !s.is_empty()))
        .or_else(|| el.name_or_id.as_ref().filter(|s| !s.is_empty()));
    if let Some(d) = descriptor {
        doc.push_str(&format!(" — {d}"));
    }
    doc.push_str(&format!(" on {normalized_url}"));
    doc
}

fn sanitize_element(mut el: RawIndexedElement) -> Option<RawIndexedElement> {
    if !el.visible || el.input_type.as_deref() == Some("hidden") {
        return None;
    }
    let role = el.role.as_deref().unwrap_or_default().to_ascii_lowercase();
    if el.editable
        || matches!(el.tag.as_str(), "input" | "textarea" | "select" | "option")
        || matches!(
            role.as_str(),
            "textbox" | "searchbox" | "combobox" | "spinbutton"
        )
    {
        el.text.clear();
    }
    Some(el)
}

pub fn elements_collection_name(project_id: &str) -> String {
    format!("sel_{project_id}_elements")
}

pub fn runs_collection_name(project_id: &str) -> String {
    format!("sel_{project_id}_runs")
}

/// Runs after `navigate` (best-effort — callers should log and continue on
/// error, never fail the navigation itself). Returns the number of elements
/// indexed.
pub async fn index_page(
    driver: &WebDriver,
    chroma: &ChromaHandle,
    embedder: &EmbeddingEngine,
    project_id: &str,
    now_epoch: u64,
) -> Result<usize, AppError> {
    if crate::chroma::memory_disabled() {
        return Ok(0);
    }
    let url = driver.current_url().await.map_err(AppError::from)?;
    let normalized = normalize_url(url.as_str());

    let ret = driver
        .execute(INDEX_SCRIPT, Vec::new())
        .await
        .map_err(AppError::from)?;
    let elements: Vec<RawIndexedElement> = ret
        .convert()
        .map_err(|e| AppError::InvalidTarget(format!("failed to parse indexed elements: {e}")))?;

    // Defense in depth: a changed or compromised DOM script cannot persist
    // control values merely by returning them in the text field.
    let elements: Vec<_> = elements.into_iter().filter_map(sanitize_element).collect();

    if elements.is_empty() {
        return Ok(0);
    }

    let mut ids = Vec::with_capacity(elements.len());
    let mut documents = Vec::with_capacity(elements.len());
    let mut metadatas = Vec::with_capacity(elements.len());

    for el in &elements {
        ids.push(element_id(&normalized, &el.css));
        documents.push(compose_document(el, &normalized));
        // Chroma metadata values must be scalars (string/int/float/bool) —
        // a nested bbox object is rejected with a 422 ("did not match any
        // variant of untagged enum UpdateMetadataValue") — so the bbox is
        // flattened into four scalar fields instead of one nested object.
        metadatas.push(serde_json::json!({
            "url": normalized,
            "css": el.css,
            "xpath": el.xpath,
            "tag": el.tag,
            "text": el.text,
            "bbox_x": el.bbox.x,
            "bbox_y": el.bbox.y,
            "bbox_w": el.bbox.w,
            "bbox_h": el.bbox.h,
            "last_seen": now_epoch,
        }));
    }

    let embeddings = embedder.embed_many(&documents).await?;

    let client = chroma.ensure_client().await?;
    let collection_id = client
        .get_or_create_collection(&elements_collection_name(project_id), "cosine")
        .await?;
    client
        .upsert(&collection_id, &ids, &embeddings, &documents, &metadatas)
        .await?;

    Ok(elements.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_clears_form_and_editable_text_before_documents() {
        for (tag, editable) in [("input", false), ("textarea", false), ("span", true)] {
            let record: RawIndexedElement = serde_json::from_value(serde_json::json!({
                "tag":tag,"text":"synthetic-sensitive-input","editable":editable,
                "css":"#field","xpath":"//field","bbox":{"x":0,"y":0,"w":10,"h":10},"visible":true
            }))
            .unwrap();
            let sanitized = sanitize_element(record).unwrap();
            assert!(sanitized.text.is_empty());
            assert!(!compose_document(&sanitized, "https://example.com")
                .contains("synthetic-sensitive-input"));
        }
    }

    #[test]
    fn normalize_url_strips_query_and_fragment() {
        assert_eq!(
            normalize_url("https://example.com/login?redirect=/x#section"),
            "https://example.com/login"
        );
        assert_eq!(
            normalize_url("https://example.com/login"),
            "https://example.com/login"
        );
    }

    #[test]
    fn element_id_is_stable_and_sixteen_hex_chars() {
        let a = element_id("https://example.com/login", "#username");
        let b = element_id("https://example.com/login", "#username");
        let c = element_id("https://example.com/login", "#password");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn compose_document_prefers_aria_label_then_placeholder_then_name() {
        let base = RawIndexedElement {
            tag: "input".to_string(),
            input_type: None,
            editable: false,
            role: None,
            text: String::new(),
            aria_label: None,
            placeholder: None,
            name_or_id: Some("username".to_string()),
            css: "#username".to_string(),
            xpath: "//*[@id=\"username\"]".to_string(),
            bbox: Bbox {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            visible: true,
        };
        assert_eq!(
            compose_document(&base, "https://example.com/login"),
            "input — username on https://example.com/login"
        );

        let with_placeholder = RawIndexedElement {
            placeholder: Some("Email address".to_string()),
            ..base.clone()
        };
        assert_eq!(
            compose_document(&with_placeholder, "https://example.com/login"),
            "input — Email address on https://example.com/login"
        );

        let with_aria = RawIndexedElement {
            aria_label: Some("Username field".to_string()),
            ..with_placeholder
        };
        assert_eq!(
            compose_document(&with_aria, "https://example.com/login"),
            "input — Username field on https://example.com/login"
        );
    }
}
