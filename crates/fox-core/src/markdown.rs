//! HTML to Markdown conversion

use crate::Link;
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};
use url::Url;

/// Convert HTML to Markdown
pub fn html_to_markdown(html: &str) -> String {
    let base_url = Url::parse("about:blank").unwrap();
    let mut links = Vec::new();
    html_to_markdown_with_base(html, &base_url, &mut links)
}

/// Convert HTML to Markdown with a base URL for resolving links
pub fn html_to_markdown_with_base(html: &str, base_url: &Url, links: &mut Vec<Link>) -> String {
    let document = Html::parse_fragment(html);
    let mut output = String::new();
    let root = document.root_element();

    convert_element(root, base_url, &mut output, links, &mut Context::default());

    // Clean up the output
    clean_markdown(&output)
}

/// Convert Markdown to plain text
pub fn markdown_to_plain(markdown: &str) -> String {
    let mut text = markdown.to_string();

    // Remove headers
    let header_re = Regex::new(r"^#{1,6}\s+").unwrap();
    text = header_re.replace_all(&text, "").to_string();

    // Remove links but keep text
    let link_re = Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap();
    text = link_re.replace_all(&text, "$1").to_string();

    // Remove bold/italic
    let bold_re = Regex::new(r"\*\*([^*]+)\*\*").unwrap();
    text = bold_re.replace_all(&text, "$1").to_string();
    let italic_re = Regex::new(r"\*([^*]+)\*").unwrap();
    text = italic_re.replace_all(&text, "$1").to_string();

    // Remove code markers
    text = text.replace('`', "");

    // Clean up whitespace
    let ws_re = Regex::new(r"\n{3,}").unwrap();
    text = ws_re.replace_all(&text, "\n\n").to_string();

    text.trim().to_string()
}

#[derive(Default)]
struct Context {
    in_pre: bool,
    in_code: bool,
    in_list: bool,
    list_depth: usize,
    list_counters: Vec<usize>,
    current_position: usize,
}

fn convert_element(
    element: ElementRef,
    base_url: &Url,
    output: &mut String,
    links: &mut Vec<Link>,
    ctx: &mut Context,
) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                let content = text.text.as_ref();
                if ctx.in_pre || ctx.in_code {
                    output.push_str(content);
                } else {
                    // Normalize whitespace
                    let normalized = normalize_whitespace(content);
                    if !normalized.is_empty() {
                        output.push_str(&normalized);
                    }
                }
                ctx.current_position = output.len();
            }
            Node::Element(_) => {
                if let Some(elem) = ElementRef::wrap(child) {
                    convert_tag(elem, base_url, output, links, ctx);
                }
            }
            _ => {}
        }
    }
}

fn convert_tag(
    element: ElementRef,
    base_url: &Url,
    output: &mut String,
    links: &mut Vec<Link>,
    ctx: &mut Context,
) {
    let tag = element.value().name();

    match tag {
        // Skip non-content elements
        "script" | "style" | "noscript" | "nav" | "footer" | "header" => return,

        // Headings
        "h1" => {
            ensure_newlines(output, 2);
            output.push_str("# ");
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }
        "h2" => {
            ensure_newlines(output, 2);
            output.push_str("## ");
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }
        "h3" => {
            ensure_newlines(output, 2);
            output.push_str("### ");
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }
        "h4" => {
            ensure_newlines(output, 2);
            output.push_str("#### ");
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }
        "h5" => {
            ensure_newlines(output, 2);
            output.push_str("##### ");
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }
        "h6" => {
            ensure_newlines(output, 2);
            output.push_str("###### ");
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }

        // Paragraphs and blocks
        "p" => {
            ensure_newlines(output, 2);
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }
        "div" | "section" | "article" | "main" => {
            ensure_newlines(output, 1);
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 1);
        }
        "br" => {
            output.push_str("  \n");
        }
        "hr" => {
            ensure_newlines(output, 2);
            output.push_str("---");
            ensure_newlines(output, 2);
        }

        // Inline formatting
        "strong" | "b" => {
            output.push_str("**");
            convert_element(element, base_url, output, links, ctx);
            output.push_str("**");
        }
        "em" | "i" => {
            output.push('*');
            convert_element(element, base_url, output, links, ctx);
            output.push('*');
        }
        "u" => {
            output.push('_');
            convert_element(element, base_url, output, links, ctx);
            output.push('_');
        }
        "s" | "strike" | "del" => {
            output.push_str("~~");
            convert_element(element, base_url, output, links, ctx);
            output.push_str("~~");
        }

        // Code
        "code" => {
            if ctx.in_pre {
                convert_element(element, base_url, output, links, ctx);
            } else {
                output.push('`');
                ctx.in_code = true;
                convert_element(element, base_url, output, links, ctx);
                ctx.in_code = false;
                output.push('`');
            }
        }
        "pre" => {
            ensure_newlines(output, 2);
            output.push_str("```\n");
            ctx.in_pre = true;
            convert_element(element, base_url, output, links, ctx);
            ctx.in_pre = false;
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str("```");
            ensure_newlines(output, 2);
        }

        // Links
        "a" => {
            let text: String = element.text().collect();
            let text = text.trim();
            if let Some(href) = element.value().attr("href") {
                let resolved = resolve_url(href, base_url);
                let position = output.len();

                output.push('[');
                if text.is_empty() {
                    output.push_str(&resolved);
                } else {
                    output.push_str(text);
                }
                output.push_str("](");
                output.push_str(&resolved);
                output.push(')');

                links.push(Link {
                    text: text.to_string(),
                    url: resolved,
                    position,
                });
            } else {
                convert_element(element, base_url, output, links, ctx);
            }
        }

        // Images
        "img" => {
            let alt = element.value().attr("alt").unwrap_or("image");
            if let Some(src) = element.value().attr("src") {
                let resolved = resolve_url(src, base_url);
                output.push_str(&format!("![{}]({})", alt, resolved));
            } else {
                output.push_str(&format!("[IMG: {}]", alt));
            }
        }

        // Lists
        "ul" => {
            ensure_newlines(output, 2);
            ctx.list_depth += 1;
            ctx.in_list = true;
            ctx.list_counters.push(0);
            convert_element(element, base_url, output, links, ctx);
            ctx.list_counters.pop();
            ctx.list_depth -= 1;
            ctx.in_list = ctx.list_depth > 0;
            ensure_newlines(output, 2);
        }
        "ol" => {
            ensure_newlines(output, 2);
            ctx.list_depth += 1;
            ctx.in_list = true;
            ctx.list_counters.push(1);
            convert_element(element, base_url, output, links, ctx);
            ctx.list_counters.pop();
            ctx.list_depth -= 1;
            ctx.in_list = ctx.list_depth > 0;
            ensure_newlines(output, 2);
        }
        "li" => {
            ensure_newlines(output, 1);
            let indent = "  ".repeat(ctx.list_depth.saturating_sub(1));
            output.push_str(&indent);

            if let Some(counter) = ctx.list_counters.last_mut() {
                if *counter > 0 {
                    output.push_str(&format!("{}. ", counter));
                    *counter += 1;
                } else {
                    output.push_str("- ");
                }
            } else {
                output.push_str("- ");
            }
            convert_element(element, base_url, output, links, ctx);
        }

        // Blockquotes
        "blockquote" => {
            ensure_newlines(output, 2);
            let inner = element.text().collect::<String>();
            for line in inner.lines() {
                output.push_str("> ");
                output.push_str(line.trim());
                output.push('\n');
            }
            ensure_newlines(output, 1);
        }

        // Tables
        "table" => {
            ensure_newlines(output, 2);
            convert_table(element, base_url, output, links, ctx);
            ensure_newlines(output, 2);
        }

        // Video/Audio
        "video" | "audio" => {
            let media_type = if tag == "video" { "VIDEO" } else { "AUDIO" };
            if let Some(src) = element.value().attr("src") {
                let resolved = resolve_url(src, base_url);
                output.push_str(&format!("[{}: {}]", media_type, resolved));
            } else {
                output.push_str(&format!("[{}]", media_type));
            }
        }

        // Iframes
        "iframe" => {
            if let Some(src) = element.value().attr("src") {
                let resolved = resolve_url(src, base_url);
                let title = element.value().attr("title").unwrap_or("embedded content");
                output.push_str(&format!("[IFRAME: {} ({})]", title, resolved));
            }
        }

        // Form elements
        "form" => {
            ensure_newlines(output, 2);
            output.push_str("[FORM]");
            ensure_newlines(output, 1);
            convert_element(element, base_url, output, links, ctx);
            ensure_newlines(output, 1);
            output.push_str("[/FORM]");
            ensure_newlines(output, 2);
        }
        "input" => {
            let input_type = element.value().attr("type").unwrap_or("text");
            let name = element.value().attr("name").unwrap_or("");
            let placeholder = element.value().attr("placeholder").unwrap_or("");
            match input_type {
                "hidden" => {}
                "submit" | "button" => {
                    let value = element.value().attr("value").unwrap_or("Submit");
                    output.push_str(&format!("[{}]", value));
                }
                "checkbox" | "radio" => {
                    let checked = element.value().attr("checked").is_some();
                    let marker = if checked { "[x]" } else { "[ ]" };
                    output.push_str(marker);
                }
                _ => {
                    let label = if !placeholder.is_empty() {
                        placeholder
                    } else if !name.is_empty() {
                        name
                    } else {
                        "input"
                    };
                    output.push_str(&format!("[INPUT: {}]", label));
                }
            }
        }
        "textarea" => {
            let name = element.value().attr("name").unwrap_or("text");
            output.push_str(&format!("[TEXTAREA: {}]", name));
        }
        "select" => {
            let name = element.value().attr("name").unwrap_or("select");
            output.push_str(&format!("[SELECT: {}]", name));
        }
        "button" => {
            let text: String = element.text().collect();
            output.push_str(&format!("[{}]", text.trim()));
        }
        "label" => {
            convert_element(element, base_url, output, links, ctx);
        }

        // Default: just process children
        _ => {
            convert_element(element, base_url, output, links, ctx);
        }
    }
}

fn convert_table(
    table: ElementRef,
    base_url: &Url,
    output: &mut String,
    links: &mut Vec<Link>,
    ctx: &mut Context,
) {
    let row_selector = Selector::parse("tr").unwrap();
    let th_selector = Selector::parse("th").unwrap();
    let td_selector = Selector::parse("td").unwrap();

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut has_header = false;

    for row in table.select(&row_selector) {
        let mut cells: Vec<String> = Vec::new();

        // Try headers first
        let headers: Vec<_> = row.select(&th_selector).collect();
        if !headers.is_empty() {
            has_header = true;
            for th in headers {
                let text: String = th.text().collect();
                cells.push(text.trim().to_string());
            }
        } else {
            // Then data cells
            for td in row.select(&td_selector) {
                let mut cell_output = String::new();
                convert_element(td, base_url, &mut cell_output, links, ctx);
                cells.push(cell_output.trim().replace('\n', " ").to_string());
            }
        }

        if !cells.is_empty() {
            rows.push(cells);
        }
    }

    if rows.is_empty() {
        return;
    }

    // Calculate column widths
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![3; col_count];

    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // Render table
    for (row_idx, row) in rows.iter().enumerate() {
        output.push('|');
        for (i, width) in col_widths.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            output.push_str(&format!(" {:<width$} |", cell, width = width));
        }
        output.push('\n');

        // Add header separator after first row if it's a header
        if row_idx == 0 && has_header {
            output.push('|');
            for width in &col_widths {
                output.push_str(&format!(" {} |", "-".repeat(*width)));
            }
            output.push('\n');
        }
    }
}

fn normalize_whitespace(text: &str) -> String {
    let ws_re = Regex::new(r"\s+").unwrap();
    ws_re.replace_all(text, " ").to_string()
}

fn ensure_newlines(output: &mut String, count: usize) {
    let trailing_newlines = output.chars().rev().take_while(|&c| c == '\n').count();
    for _ in trailing_newlines..count {
        output.push('\n');
    }
}

fn resolve_url(href: &str, base_url: &Url) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with("//") {
        format!("{}:{}", base_url.scheme(), href)
    } else {
        base_url.join(href).map(|u| u.to_string()).unwrap_or_else(|_| href.to_string())
    }
}

fn clean_markdown(md: &str) -> String {
    // Remove excessive newlines
    let re = Regex::new(r"\n{3,}").unwrap();
    let cleaned = re.replace_all(md, "\n\n");

    // Trim and ensure final newline
    let mut result = cleaned.trim().to_string();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_conversion() {
        let html = "<h1>Title</h1>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"));
    }

    #[test]
    fn test_paragraph_conversion() {
        let html = "<p>Hello world</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn test_link_conversion() {
        let html = r#"<a href="https://example.com">Example</a>"#;
        let md = html_to_markdown(html);
        assert!(md.contains("[Example](https://example.com)"));
    }

    #[test]
    fn test_list_conversion() {
        let html = "<ul><li>One</li><li>Two</li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("- One"));
        assert!(md.contains("- Two"));
    }

    #[test]
    fn test_bold_italic() {
        let html = "<p><strong>bold</strong> and <em>italic</em></p>";
        let md = html_to_markdown(html);
        assert!(md.contains("**bold**"));
        assert!(md.contains("*italic*"));
    }

    #[test]
    fn test_markdown_to_plain() {
        let md = "# Title\n\n**bold** [link](url)";
        let plain = markdown_to_plain(md);
        assert_eq!(plain, "Title\n\nbold link");
    }
}
