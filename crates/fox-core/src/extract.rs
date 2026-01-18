//! Readability-style content extraction

use crate::{markdown, ExtractedContent, FoxError, Link, Result};
use scraper::{ElementRef, Html, Selector};
use std::collections::HashMap;
use tracing::debug;
use url::Url;

/// Extract the page title from HTML
pub fn extract_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("title").ok()?;

    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
}

/// Extract main content from HTML using readability-style algorithm
pub fn extract_content(html: &str, base_url: &Url) -> Result<ExtractedContent> {
    let document = Html::parse_document(html);

    // Try to find the main content container
    let content_html = find_main_content(&document)?;

    // Convert to markdown
    let (text, links) = html_to_markdown_with_links(&content_html, base_url);

    // Extract title from content or page
    let title = extract_content_title(&document);

    Ok(ExtractedContent { text, title, links })
}

/// Find the main content container using readability-style scoring
fn find_main_content(document: &Html) -> Result<String> {
    // Priority order for content selection:
    // 1. <article> element
    // 2. <main> element
    // 3. Element with role="main"
    // 4. Common content class names
    // 5. Highest-scoring element by text density

    let selectors_priority = [
        "article",
        "main",
        "[role='main']",
        ".post-content",
        ".article-content",
        ".entry-content",
        ".content",
        "#content",
        ".post",
        ".article",
    ];

    for selector_str in &selectors_priority {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let html = element.html();
                if has_meaningful_content(&html) {
                    debug!("Found content using selector: {}", selector_str);
                    return Ok(html);
                }
            }
        }
    }

    // Fall back to scoring-based approach
    score_and_extract(document)
}

/// Check if HTML has meaningful content
fn has_meaningful_content(html: &str) -> bool {
    let text_len = Html::parse_fragment(html)
        .root_element()
        .text()
        .collect::<String>()
        .trim()
        .len();
    text_len > 100
}

/// Score elements and extract the best candidate
fn score_and_extract(document: &Html) -> Result<String> {
    let body_selector = Selector::parse("body").unwrap();
    let body = document
        .select(&body_selector)
        .next()
        .ok_or_else(|| FoxError::ExtractionError("No body element found".to_string()))?;

    let mut scores: HashMap<String, (i32, String)> = HashMap::new();
    score_element(body, &mut scores, 0);

    // Find the highest-scoring element
    let best = scores
        .into_iter()
        .max_by_key(|(_, (score, _))| *score)
        .map(|(_, (_, html))| html)
        .unwrap_or_else(|| body.html());

    Ok(best)
}

/// Score an element and its children
fn score_element(element: ElementRef, scores: &mut HashMap<String, (i32, String)>, depth: usize) {
    let tag_name = element.value().name();

    // Skip non-content elements
    if matches!(
        tag_name,
        "script" | "style" | "nav" | "header" | "footer" | "aside" | "noscript" | "iframe"
    ) {
        return;
    }

    // Check for negative class/id patterns
    if let Some(class) = element.value().attr("class") {
        let class_lower = class.to_lowercase();
        if class_lower.contains("nav")
            || class_lower.contains("menu")
            || class_lower.contains("sidebar")
            || class_lower.contains("footer")
            || class_lower.contains("header")
            || class_lower.contains("ad")
            || class_lower.contains("comment")
        {
            return;
        }
    }

    // Calculate score
    let mut score = 0i32;

    // Positive signals
    if matches!(tag_name, "article" | "main" | "section") {
        score += 50;
    }
    if matches!(tag_name, "div" | "td") {
        score += 5;
    }
    if matches!(tag_name, "p") {
        score += 10;
    }

    // Count paragraphs and text length
    let p_selector = Selector::parse("p").unwrap();
    let p_count = element.select(&p_selector).count();
    score += (p_count * 5) as i32;

    // Text density
    let text: String = element.text().collect();
    let text_len = text.trim().len();
    score += (text_len / 100) as i32;

    // Penalize for being too deep
    score -= (depth * 2) as i32;

    // Store score if meaningful
    if text_len > 50 {
        let key = format!("{}_{}", tag_name, depth);
        scores.insert(key, (score, element.html()));
    }

    // Score children
    for child in element.children() {
        if let Some(child_elem) = ElementRef::wrap(child) {
            score_element(child_elem, scores, depth + 1);
        }
    }
}

/// Extract title from content area
fn extract_content_title(document: &Html) -> Option<String> {
    // Try h1 first
    let h1_selector = Selector::parse("h1").ok()?;
    if let Some(h1) = document.select(&h1_selector).next() {
        let text = h1.text().collect::<String>().trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }

    // Fall back to title tag
    extract_title(&document.html())
}

/// Convert HTML to markdown and extract links
fn html_to_markdown_with_links(html: &str, base_url: &Url) -> (String, Vec<Link>) {
    let mut links = Vec::new();
    let md = markdown::html_to_markdown_with_base(html, base_url, &mut links);
    (md, links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        let html = "<html><head><title>Test Page</title></head><body></body></html>";
        assert_eq!(extract_title(html), Some("Test Page".to_string()));
    }

    #[test]
    fn test_extract_title_missing() {
        let html = "<html><head></head><body></body></html>";
        assert_eq!(extract_title(html), None);
    }

    #[test]
    fn test_meaningful_content() {
        let short = "<p>Hi</p>";
        let long = "<p>".to_string() + &"a".repeat(200) + "</p>";
        assert!(!has_meaningful_content(short));
        assert!(has_meaningful_content(&long));
    }
}
