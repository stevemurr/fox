//! Accessibility tree extraction and conversion
//!
//! Uses Chrome's accessibility tree (AX tree) to extract semantic content
//! from web pages. The AX tree is what Chrome builds for screen readers -
//! it represents the meaningful structure of a page without visual noise.

use crate::{FoxError, Result};
use chromiumoxide::cdp::browser_protocol::accessibility::{
    AxNode as CdpAxNode, AxProperty, AxValue, AxValueType, EnableParams, GetFullAxTreeParams,
};
use chromiumoxide::Page;
use std::collections::HashMap;
use tracing::debug;

/// A node in the accessibility tree
#[derive(Debug, Clone)]
pub struct AXNode {
    /// Unique node ID
    pub node_id: String,
    /// Accessibility role (heading, link, paragraph, etc.)
    pub role: String,
    /// Accessible name (the text content)
    pub name: Option<String>,
    /// Accessible value (for inputs)
    pub value: Option<String>,
    /// Accessible description
    pub description: Option<String>,
    /// Heading level (1-6) for heading roles
    pub level: Option<i64>,
    /// URL for links
    pub url: Option<String>,
    /// Whether the node is focused
    pub focused: bool,
    /// Whether the node is ignored (not rendered)
    pub ignored: bool,
    /// Child node IDs
    pub child_ids: Vec<String>,
    /// Additional properties
    pub properties: HashMap<String, String>,
}

impl AXNode {
    /// Check if this node should be skipped during markdown conversion
    pub fn should_skip(&self) -> bool {
        // Skip ignored nodes
        if self.ignored {
            return true;
        }

        // Skip structural/navigational roles that don't contribute content
        matches!(
            self.role.as_str(),
            "none"
                | "presentation"
                | "generic"  // Usually just containers
                | "navigation"
                | "banner"
                | "contentinfo"
                | "complementary"
                | "search"
                | "form"  // Skip form containers, but not inputs
                | "region"
        )
    }

    /// Check if this is a block-level element (needs newlines around it)
    pub fn is_block(&self) -> bool {
        matches!(
            self.role.as_str(),
            "heading"
                | "paragraph"
                | "article"
                | "main"
                | "section"
                | "blockquote"
                | "list"
                | "listitem"
                | "table"
                | "row"
                | "cell"
                | "columnheader"
                | "rowheader"
                | "figure"
                | "separator"
        )
    }

    /// Check if this node has meaningful content
    pub fn has_content(&self) -> bool {
        self.name.as_ref().map(|n| !n.trim().is_empty()).unwrap_or(false)
            || self.value.as_ref().map(|v| !v.trim().is_empty()).unwrap_or(false)
    }
}

/// The full accessibility tree for a page
#[derive(Debug)]
pub struct AXTree {
    /// All nodes indexed by ID
    pub nodes: HashMap<String, AXNode>,
    /// Root node ID
    pub root_id: Option<String>,
}

impl AXTree {
    /// Create an empty tree
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            root_id: None,
        }
    }

    /// Get a node by ID
    pub fn get(&self, id: &str) -> Option<&AXNode> {
        self.nodes.get(id)
    }

    /// Get the root node
    pub fn root(&self) -> Option<&AXNode> {
        self.root_id.as_ref().and_then(|id| self.nodes.get(id))
    }

    /// Iterate over all nodes in tree order (depth-first)
    pub fn iter_depth_first(&self) -> impl Iterator<Item = &AXNode> {
        DepthFirstIterator::new(self)
    }

    /// Get children of a node
    pub fn children(&self, node: &AXNode) -> Vec<&AXNode> {
        node.child_ids
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    /// Walk the tree, calling a function for each node with its depth
    pub fn walk<F>(&self, mut f: F)
    where
        F: FnMut(&AXNode, usize),
    {
        if let Some(root) = self.root() {
            self.walk_recursive(root, 0, &mut f);
        }
    }

    fn walk_recursive<F>(&self, node: &AXNode, depth: usize, f: &mut F)
    where
        F: FnMut(&AXNode, usize),
    {
        f(node, depth);
        for child in self.children(node) {
            self.walk_recursive(child, depth + 1, f);
        }
    }
}

impl Default for AXTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Depth-first iterator over AX tree nodes
struct DepthFirstIterator<'a> {
    tree: &'a AXTree,
    stack: Vec<&'a str>,
}

impl<'a> DepthFirstIterator<'a> {
    fn new(tree: &'a AXTree) -> Self {
        let stack = tree.root_id.as_deref().into_iter().collect();
        Self { tree, stack }
    }
}

impl<'a> Iterator for DepthFirstIterator<'a> {
    type Item = &'a AXNode;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let node = self.tree.nodes.get(id)?;

        // Push children in reverse order so they're visited in order
        for child_id in node.child_ids.iter().rev() {
            self.stack.push(child_id);
        }

        Some(node)
    }
}

/// Fetch the full accessibility tree from a page
pub async fn fetch_ax_tree(page: &Page) -> Result<AXTree> {
    debug!("Fetching accessibility tree");

    // Enable the Accessibility domain first
    page.execute(EnableParams::default())
        .await
        .map_err(|e| FoxError::BrowserError(format!("Failed to enable Accessibility: {}", e)))?;

    // Get the full AX tree
    // Using depth limit to potentially avoid issues with very large trees
    let params = GetFullAxTreeParams::builder().depth(100).build();
    let response = page
        .execute(params)
        .await
        .map_err(|e| FoxError::BrowserError(format!("Failed to get AX tree: {}", e)))?;

    // Convert CDP nodes to our format
    let mut tree = AXTree::new();

    for cdp_node in &response.nodes {
        let node = convert_cdp_node(&cdp_node);

        // First node is typically the root
        if tree.root_id.is_none() {
            tree.root_id = Some(node.node_id.clone());
        }

        debug!(
            "AX node: id={}, role={}, name={:?}",
            node.node_id,
            node.role,
            node.name
        );

        tree.nodes.insert(node.node_id.clone(), node);
    }

    debug!(
        "Fetched AX tree with {} nodes, root={:?}",
        tree.nodes.len(),
        tree.root_id
    );

    Ok(tree)
}

/// Convert a CDP AXNode to our AXNode format
fn convert_cdp_node(cdp: &CdpAxNode) -> AXNode {
    let node_id = cdp.node_id.inner().to_string();

    // Extract role
    let role = cdp
        .role
        .as_ref()
        .and_then(|v| extract_string_value(v))
        .unwrap_or_else(|| "unknown".to_string());

    // Extract name
    let name = cdp.name.as_ref().and_then(|v| extract_string_value(v));

    // Extract value
    let value = cdp.value.as_ref().and_then(|v| extract_string_value(v));

    // Extract description
    let description = cdp
        .description
        .as_ref()
        .and_then(|v| extract_string_value(v));

    // Extract properties
    let mut properties = HashMap::new();
    let mut level = None;
    let mut url = None;
    let mut focused = false;

    if let Some(props) = &cdp.properties {
        for prop in props {
            if let Some(value) = extract_property_value(prop) {
                let name = format!("{:?}", prop.name);
                properties.insert(name.clone(), value.clone());

                // Extract specific properties based on name
                let name_str = format!("{:?}", prop.name);
                match name_str.as_str() {
                    "Level" => {
                        level = value.parse().ok();
                    }
                    "Url" => {
                        url = Some(value.clone());
                    }
                    "Focused" => {
                        focused = value == "true";
                    }
                    _ => {}
                }
            }
        }
    }

    // Extract child IDs
    let child_ids = cdp
        .child_ids
        .as_ref()
        .map(|ids| ids.iter().map(|id| id.inner().to_string()).collect())
        .unwrap_or_default();

    // Check if ignored
    let ignored = cdp.ignored;

    AXNode {
        node_id,
        role,
        name,
        value,
        description,
        level,
        url,
        focused,
        ignored,
        child_ids,
        properties,
    }
}

/// Extract a string value from an AxValue
fn extract_string_value(value: &AxValue) -> Option<String> {
    match value.r#type {
        AxValueType::String | AxValueType::ComputedString => {
            value.value.as_ref().and_then(|v| v.as_str().map(String::from))
        }
        AxValueType::Integer => {
            value.value.as_ref().and_then(|v| v.as_i64().map(|i| i.to_string()))
        }
        AxValueType::Number => {
            value.value.as_ref().and_then(|v| v.as_f64().map(|f| f.to_string()))
        }
        AxValueType::Boolean | AxValueType::Tristate | AxValueType::BooleanOrUndefined => {
            value.value.as_ref().and_then(|v| v.as_bool().map(|b| b.to_string()))
        }
        _ => value.value.as_ref().and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else {
                Some(v.to_string())
            }
        }),
    }
}

/// Extract a value from an AxProperty
fn extract_property_value(prop: &AxProperty) -> Option<String> {
    extract_string_value(&prop.value)
}

// ============================================================================
// AX Tree to Markdown Conversion
// ============================================================================

use crate::Link;

/// Convert an accessibility tree to markdown
pub fn ax_tree_to_markdown(tree: &AXTree) -> (String, Vec<Link>) {
    let mut converter = MarkdownConverter::new(tree);
    converter.convert();
    (converter.output, converter.links)
}

/// State machine for converting AX tree to markdown
struct MarkdownConverter<'a> {
    tree: &'a AXTree,
    output: String,
    links: Vec<Link>,
    list_depth: usize,
    list_counters: Vec<usize>,
    in_code_block: bool,
    last_was_block: bool,
}

impl<'a> MarkdownConverter<'a> {
    fn new(tree: &'a AXTree) -> Self {
        Self {
            tree,
            output: String::new(),
            links: Vec::new(),
            list_depth: 0,
            list_counters: Vec::new(),
            in_code_block: false,
            last_was_block: false,
        }
    }

    fn convert(&mut self) {
        if let Some(root) = self.tree.root() {
            self.convert_node(root);
        }
        self.cleanup_output();
    }

    fn convert_node(&mut self, node: &AXNode) {
        // For purely structural nodes, just process children
        if self.should_skip_but_recurse(node) {
            self.convert_children(node);
            return;
        }

        // Skip nodes that shouldn't be rendered at all
        if self.should_skip_node(node) {
            return;
        }

        match node.role.as_str() {
            // Document root - just process children
            "RootWebArea" | "WebArea" | "document" => {
                self.convert_children(node);
            }

            // Headings
            "heading" => {
                self.ensure_block_spacing();
                let level = node.level.unwrap_or(1).min(6) as usize;
                let prefix = "#".repeat(level);
                if let Some(text) = self.get_node_text(node) {
                    self.output.push_str(&prefix);
                    self.output.push(' ');
                    self.output.push_str(&text);
                    self.output.push_str("\n\n");
                    self.last_was_block = true;
                }
            }

            // Paragraphs
            "paragraph" => {
                // Check if paragraph has children that need special handling
                let has_special_children = self.tree.children(node).iter().any(|c| {
                    matches!(c.role.as_str(), "link" | "strong" | "emphasis" | "code" | "image")
                });

                if has_special_children {
                    // Paragraph with structured content (links, etc.)
                    self.ensure_block_spacing();
                    self.convert_children(node);
                    self.output.push_str("\n\n");
                    self.last_was_block = true;
                } else {
                    // Plain text paragraph
                    let text = self.get_node_text(node);
                    if let Some(text) = text {
                        if !text.trim().is_empty() {
                            self.ensure_block_spacing();
                            self.output.push_str(&text);
                            self.output.push_str("\n\n");
                            self.last_was_block = true;
                        }
                    }
                }
            }

            // Links
            "link" => {
                let text = self.get_node_text(node).unwrap_or_default();
                let url = node.url.as_deref().unwrap_or("#");

                if !text.is_empty() {
                    let position = self.output.len();
                    self.output.push('[');
                    self.output.push_str(&text);
                    self.output.push_str("](");
                    self.output.push_str(url);
                    self.output.push(')');

                    self.links.push(Link {
                        text: text.clone(),
                        url: url.to_string(),
                        position,
                    });
                }
            }

            // Lists
            "list" => {
                self.ensure_block_spacing();
                self.list_depth += 1;
                self.list_counters.push(0);
                self.convert_children(node);
                self.list_counters.pop();
                self.list_depth -= 1;
                if self.list_depth == 0 {
                    self.output.push('\n');
                    self.last_was_block = true;
                }
            }

            // List items
            "listitem" => {
                let indent = "  ".repeat(self.list_depth.saturating_sub(1));

                // Increment counter for ordered lists
                if let Some(counter) = self.list_counters.last_mut() {
                    *counter += 1;
                }

                // Check if this is an ordered list item
                let is_ordered = node.properties.get("SetSize").is_some()
                    || node.properties.get("PosInSet").is_some();

                let marker = if is_ordered {
                    let num = self.list_counters.last().copied().unwrap_or(1);
                    format!("{}. ", num)
                } else {
                    "- ".to_string()
                };

                self.output.push_str(&indent);
                self.output.push_str(&marker);

                // Get list item content
                if let Some(text) = self.get_node_text(node) {
                    self.output.push_str(&text);
                } else {
                    self.convert_children(node);
                }
                self.output.push('\n');
            }

            // Blockquotes
            "blockquote" => {
                self.ensure_block_spacing();
                let text = self.get_node_text(node).unwrap_or_default();
                for line in text.lines() {
                    self.output.push_str("> ");
                    self.output.push_str(line);
                    self.output.push('\n');
                }
                self.output.push('\n');
                self.last_was_block = true;
            }

            // Code blocks
            "code" | "pre" => {
                if self.in_code_block {
                    // Inline code within a code block - just add text
                    if let Some(text) = self.get_node_text(node) {
                        self.output.push_str(&text);
                    }
                } else {
                    let text = self.get_node_text(node).unwrap_or_default();
                    if text.contains('\n') {
                        // Multi-line code block
                        self.ensure_block_spacing();
                        self.output.push_str("```\n");
                        self.in_code_block = true;
                        self.output.push_str(&text);
                        if !text.ends_with('\n') {
                            self.output.push('\n');
                        }
                        self.output.push_str("```\n\n");
                        self.in_code_block = false;
                        self.last_was_block = true;
                    } else {
                        // Inline code
                        self.output.push('`');
                        self.output.push_str(&text);
                        self.output.push('`');
                    }
                }
            }

            // Images
            "image" | "img" => {
                let alt = node.name.as_deref().unwrap_or("image");
                let src = node.url.as_deref().unwrap_or("");
                if !src.is_empty() {
                    self.output.push_str("![");
                    self.output.push_str(alt);
                    self.output.push_str("](");
                    self.output.push_str(src);
                    self.output.push(')');
                }
            }

            // Tables
            "table" => {
                self.ensure_block_spacing();
                self.convert_table(node);
                self.output.push('\n');
                self.last_was_block = true;
            }

            // Horizontal rule / separator
            "separator" => {
                self.ensure_block_spacing();
                self.output.push_str("---\n\n");
                self.last_was_block = true;
            }

            // Strong/bold
            "strong" => {
                self.output.push_str("**");
                if let Some(text) = self.get_node_text(node) {
                    self.output.push_str(&text);
                } else {
                    self.convert_children(node);
                }
                self.output.push_str("**");
            }

            // Emphasis/italic
            "emphasis" => {
                self.output.push('*');
                if let Some(text) = self.get_node_text(node) {
                    self.output.push_str(&text);
                } else {
                    self.convert_children(node);
                }
                self.output.push('*');
            }

            // Static text - the actual text content
            "StaticText" => {
                if let Some(text) = &node.name {
                    self.output.push_str(text);
                }
            }

            // Inline text boxes - ignore, content comes from parent StaticText
            "InlineTextBox" => {
                // Skip - handled by parent StaticText
            }

            // Generic containers - just process children
            "generic" | "group" | "section" | "article" | "main" | "region" => {
                self.convert_children(node);
            }

            // Form elements
            "textbox" | "searchbox" => {
                let label = node.name.as_deref().unwrap_or("input");
                self.output.push_str(&format!("[Input: {}]", label));
            }

            "button" => {
                let label = node.name.as_deref().unwrap_or("button");
                self.output.push_str(&format!("[Button: {}]", label));
            }

            "checkbox" => {
                let label = node.name.as_deref().unwrap_or("checkbox");
                let checked = node.properties.get("Checked")
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let marker = if checked { "[x]" } else { "[ ]" };
                self.output.push_str(&format!("{} {}", marker, label));
            }

            // Skip these roles entirely
            "navigation" | "banner" | "contentinfo" | "complementary" | "search"
            | "form" | "toolbar" | "menubar" | "menu" | "menuitem" => {
                // Skip navigation/structural elements
            }

            // Default: try to get text or recurse
            _ => {
                if let Some(text) = self.get_node_text(node) {
                    if !text.trim().is_empty() {
                        self.output.push_str(&text);
                    }
                } else {
                    self.convert_children(node);
                }
            }
        }
    }

    fn convert_children(&mut self, node: &AXNode) {
        for child in self.tree.children(node) {
            self.convert_node(child);
        }
    }

    /// Nodes we skip but still recurse into their children
    fn should_skip_but_recurse(&self, node: &AXNode) -> bool {
        matches!(
            node.role.as_str(),
            "none" | "presentation" | "generic"
        )
    }

    fn should_skip_node(&self, node: &AXNode) -> bool {
        if node.ignored {
            return true;
        }

        // Skip purely visual nodes with no semantic meaning
        matches!(
            node.role.as_str(),
            "LineBreak" | "InlineTextBox"
        )
    }

    /// Get the text content of a node, either from name or by collecting from children
    fn get_node_text(&self, node: &AXNode) -> Option<String> {
        // If node has a name, use it
        if let Some(name) = &node.name {
            if !name.trim().is_empty() {
                return Some(name.clone());
            }
        }

        // Otherwise, try to collect text from StaticText children
        let mut text = String::new();
        self.collect_text(node, &mut text);

        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn collect_text(&self, node: &AXNode, text: &mut String) {
        if node.role == "StaticText" {
            if let Some(name) = &node.name {
                text.push_str(name);
            }
            return;
        }

        // Don't collect text from InlineTextBox (duplicates StaticText)
        if node.role == "InlineTextBox" {
            return;
        }

        for child in self.tree.children(node) {
            self.collect_text(child, text);
        }
    }

    fn ensure_block_spacing(&mut self) {
        if !self.output.is_empty() && !self.last_was_block {
            if !self.output.ends_with("\n\n") {
                if self.output.ends_with('\n') {
                    self.output.push('\n');
                } else {
                    self.output.push_str("\n\n");
                }
            }
        }
    }

    fn convert_table(&mut self, table_node: &AXNode) {
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut header_row: Option<usize> = None;

        // Collect all rows
        for child in self.tree.children(table_node) {
            if child.role == "row" || child.role == "rowgroup" {
                if child.role == "rowgroup" {
                    // Process rows within rowgroup
                    for row_child in self.tree.children(child) {
                        if row_child.role == "row" {
                            let is_header = self.tree.children(row_child)
                                .iter()
                                .any(|c| c.role == "columnheader" || c.role == "rowheader");

                            let cells: Vec<String> = self.tree.children(row_child)
                                .iter()
                                .filter(|c| matches!(c.role.as_str(), "cell" | "columnheader" | "rowheader" | "gridcell"))
                                .map(|c| self.get_node_text(c).unwrap_or_default())
                                .collect();

                            if is_header && header_row.is_none() {
                                header_row = Some(rows.len());
                            }
                            rows.push(cells);
                        }
                    }
                } else {
                    let is_header = self.tree.children(child)
                        .iter()
                        .any(|c| c.role == "columnheader" || c.role == "rowheader");

                    let cells: Vec<String> = self.tree.children(child)
                        .iter()
                        .filter(|c| matches!(c.role.as_str(), "cell" | "columnheader" | "rowheader" | "gridcell"))
                        .map(|c| self.get_node_text(c).unwrap_or_default())
                        .collect();

                    if is_header && header_row.is_none() {
                        header_row = Some(rows.len());
                    }
                    rows.push(cells);
                }
            }
        }

        if rows.is_empty() {
            return;
        }

        // Calculate column widths
        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut col_widths: Vec<usize> = vec![3; num_cols]; // minimum width of 3

        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Output table
        for (row_idx, row) in rows.iter().enumerate() {
            self.output.push('|');
            for (col_idx, cell) in row.iter().enumerate() {
                let width = col_widths.get(col_idx).copied().unwrap_or(3);
                self.output.push(' ');
                self.output.push_str(&format!("{:width$}", cell, width = width));
                self.output.push_str(" |");
            }
            // Pad missing cells
            for col_idx in row.len()..num_cols {
                let width = col_widths.get(col_idx).copied().unwrap_or(3);
                self.output.push(' ');
                self.output.push_str(&" ".repeat(width));
                self.output.push_str(" |");
            }
            self.output.push('\n');

            // Add separator after header row
            if Some(row_idx) == header_row || (header_row.is_none() && row_idx == 0) {
                self.output.push('|');
                for width in &col_widths {
                    self.output.push_str(&format!(" {} |", "-".repeat(*width)));
                }
                self.output.push('\n');
            }
        }
    }

    fn cleanup_output(&mut self) {
        // Remove excessive newlines (more than 2 consecutive)
        while self.output.contains("\n\n\n") {
            self.output = self.output.replace("\n\n\n", "\n\n");
        }

        // Trim leading/trailing whitespace
        self.output = self.output.trim().to_string();

        // Ensure single trailing newline
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ax_node_should_skip() {
        let mut node = AXNode {
            node_id: "1".to_string(),
            role: "heading".to_string(),
            name: Some("Title".to_string()),
            value: None,
            description: None,
            level: Some(1),
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        assert!(!node.should_skip());

        node.role = "navigation".to_string();
        assert!(node.should_skip());

        node.role = "paragraph".to_string();
        node.ignored = true;
        assert!(node.should_skip());
    }

    #[test]
    fn test_ax_node_is_block() {
        let mut node = AXNode {
            node_id: "1".to_string(),
            role: "heading".to_string(),
            name: None,
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        assert!(node.is_block());

        node.role = "link".to_string();
        assert!(!node.is_block());

        node.role = "paragraph".to_string();
        assert!(node.is_block());
    }

    #[test]
    fn test_ax_tree_iteration() {
        let mut tree = AXTree::new();

        let root = AXNode {
            node_id: "root".to_string(),
            role: "RootWebArea".to_string(),
            name: None,
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec!["child1".to_string(), "child2".to_string()],
            properties: HashMap::new(),
        };

        let child1 = AXNode {
            node_id: "child1".to_string(),
            role: "heading".to_string(),
            name: Some("Title".to_string()),
            value: None,
            description: None,
            level: Some(1),
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        let child2 = AXNode {
            node_id: "child2".to_string(),
            role: "paragraph".to_string(),
            name: Some("Content".to_string()),
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        tree.root_id = Some("root".to_string());
        tree.nodes.insert("root".to_string(), root);
        tree.nodes.insert("child1".to_string(), child1);
        tree.nodes.insert("child2".to_string(), child2);

        let nodes: Vec<_> = tree.iter_depth_first().collect();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].node_id, "root");
        assert_eq!(nodes[1].node_id, "child1");
        assert_eq!(nodes[2].node_id, "child2");
    }

    #[test]
    fn test_ax_to_markdown_heading() {
        let mut tree = AXTree::new();

        let root = AXNode {
            node_id: "root".to_string(),
            role: "RootWebArea".to_string(),
            name: None,
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec!["h1".to_string()],
            properties: HashMap::new(),
        };

        let heading = AXNode {
            node_id: "h1".to_string(),
            role: "heading".to_string(),
            name: Some("Hello World".to_string()),
            value: None,
            description: None,
            level: Some(1),
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        tree.root_id = Some("root".to_string());
        tree.nodes.insert("root".to_string(), root);
        tree.nodes.insert("h1".to_string(), heading);

        let (markdown, _links) = ax_tree_to_markdown(&tree);
        assert_eq!(markdown.trim(), "# Hello World");
    }

    #[test]
    fn test_ax_to_markdown_link() {
        let mut tree = AXTree::new();

        let root = AXNode {
            node_id: "root".to_string(),
            role: "RootWebArea".to_string(),
            name: None,
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec!["p".to_string()],
            properties: HashMap::new(),
        };

        let paragraph = AXNode {
            node_id: "p".to_string(),
            role: "paragraph".to_string(),
            name: Some("".to_string()),
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec!["link".to_string()],
            properties: HashMap::new(),
        };

        let link = AXNode {
            node_id: "link".to_string(),
            role: "link".to_string(),
            name: Some("Click here".to_string()),
            value: None,
            description: None,
            level: None,
            url: Some("https://example.com".to_string()),
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        tree.root_id = Some("root".to_string());
        tree.nodes.insert("root".to_string(), root);
        tree.nodes.insert("p".to_string(), paragraph);
        tree.nodes.insert("link".to_string(), link);

        let (markdown, links) = ax_tree_to_markdown(&tree);
        assert!(markdown.contains("[Click here](https://example.com)"));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "Click here");
        assert_eq!(links[0].url, "https://example.com");
    }

    #[test]
    fn test_ax_to_markdown_list() {
        let mut tree = AXTree::new();

        let root = AXNode {
            node_id: "root".to_string(),
            role: "RootWebArea".to_string(),
            name: None,
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec!["list".to_string()],
            properties: HashMap::new(),
        };

        let list = AXNode {
            node_id: "list".to_string(),
            role: "list".to_string(),
            name: None,
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec!["item1".to_string(), "item2".to_string()],
            properties: HashMap::new(),
        };

        let item1 = AXNode {
            node_id: "item1".to_string(),
            role: "listitem".to_string(),
            name: Some("First item".to_string()),
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        let item2 = AXNode {
            node_id: "item2".to_string(),
            role: "listitem".to_string(),
            name: Some("Second item".to_string()),
            value: None,
            description: None,
            level: None,
            url: None,
            focused: false,
            ignored: false,
            child_ids: vec![],
            properties: HashMap::new(),
        };

        tree.root_id = Some("root".to_string());
        tree.nodes.insert("root".to_string(), root);
        tree.nodes.insert("list".to_string(), list);
        tree.nodes.insert("item1".to_string(), item1);
        tree.nodes.insert("item2".to_string(), item2);

        let (markdown, _links) = ax_tree_to_markdown(&tree);
        assert!(markdown.contains("- First item"));
        assert!(markdown.contains("- Second item"));
    }
}
