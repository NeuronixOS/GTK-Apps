//! Shared Markdown rendering for the editor split view and side-panel plugin.
//! Renders into a GtkTextBuffer with styled tags (not flattened plain text).

use std::path::Path;

use gtk4 as gtk;
use gtk::pango;
use gtk::prelude::*;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::document::Document;

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            e == "md" || e == "markdown" || e == "mdown"
        })
        .unwrap_or(false)
}

pub fn is_markdown_document(doc: &Document) -> bool {
    if let Some(path) = doc.path() {
        if is_markdown_path(&path) {
            return true;
        }
    }
    doc.language_id()
        .map(|id| id.to_ascii_lowercase().contains("markdown"))
        .unwrap_or(false)
}

/// Clear `buffer` and render Markdown as a styled document.
pub fn render_markdown_to_buffer(buffer: &gtk::TextBuffer, md: &str) {
    ensure_tags(buffer);
    buffer.set_text("");

    if md.trim().is_empty() {
        insert_plain(buffer, "Start typing Markdown to see the preview…");
        return;
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, options);

    let mut inline_tags: Vec<&'static str> = Vec::new();
    let mut list_depth = 0usize;
    // None = unordered; Some(n) = next ordered index.
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut table: Option<TableBuild> = None;
    let mut link_dest: Option<String> = None;
    let mut in_code_block = false;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    ensure_block_gap(buffer);
                    inline_tags.push(heading_tag(level as u32));
                }
                Tag::Paragraph => {
                    if table.is_none() {
                        ensure_block_gap(buffer);
                    }
                }
                Tag::BlockQuote(_) => {
                    ensure_block_gap(buffer);
                    inline_tags.push("md-quote");
                }
                Tag::CodeBlock(kind) => {
                    ensure_block_gap(buffer);
                    in_code_block = true;
                    inline_tags.push("md-codeblock");
                    if let CodeBlockKind::Fenced(lang) = kind {
                        if !lang.is_empty() {
                            insert_tagged(buffer, &format!("{lang}\n"), &["md-code-lang"]);
                        }
                    }
                }
                Tag::List(start) => {
                    ensure_block_gap(buffer);
                    list_depth += 1;
                    list_counters.push(start);
                }
                Tag::Item => {
                    let indent = "    ".repeat(list_depth.saturating_sub(1));
                    let bullet = match list_counters.last_mut() {
                        Some(Some(n)) => {
                            let cur = *n;
                            *n += 1;
                            format!("{indent}{cur}. ")
                        }
                        _ => format!("{indent}• "),
                    };
                    insert_tagged(buffer, &bullet, &["md-list-marker"]);
                }
                Tag::Emphasis => inline_tags.push("md-em"),
                Tag::Strong => inline_tags.push("md-strong"),
                Tag::Strikethrough => inline_tags.push("md-strike"),
                Tag::Link { dest_url, .. } => {
                    link_dest = Some(dest_url.to_string());
                    inline_tags.push("md-link");
                }
                Tag::Image { dest_url, title, .. } => {
                    let label = if title.is_empty() {
                        format!("[image: {dest_url}]")
                    } else {
                        format!("[image: {title}]")
                    };
                    insert_tagged(buffer, &label, &["md-link"]);
                }
                Tag::Table(_) => {
                    ensure_block_gap(buffer);
                    table = Some(TableBuild::default());
                }
                Tag::TableHead => {
                    if let Some(t) = table.as_mut() {
                        t.in_header = true;
                    }
                }
                Tag::TableRow => {
                    if let Some(t) = table.as_mut() {
                        t.current_row.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(t) = table.as_mut() {
                        t.current_cell.clear();
                        t.capturing_cell = true;
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => {
                    inline_tags.pop();
                    insert_plain(buffer, "\n");
                }
                TagEnd::Paragraph => {
                    if table.as_ref().map(|t| !t.capturing_cell).unwrap_or(true) && table.is_none()
                    {
                        insert_plain(buffer, "\n");
                    }
                }
                TagEnd::BlockQuote(_) => {
                    inline_tags.pop();
                    insert_plain(buffer, "\n");
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    inline_tags.pop();
                    insert_plain(buffer, "\n");
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    list_counters.pop();
                    insert_plain(buffer, "\n");
                }
                TagEnd::Item => insert_plain(buffer, "\n"),
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    inline_tags.pop();
                }
                TagEnd::Link => {
                    if let Some(dest) = link_dest.take() {
                        insert_tagged(buffer, &format!(" ({dest})"), &["md-link-url"]);
                    }
                    inline_tags.pop();
                }
                TagEnd::TableCell => {
                    if let Some(t) = table.as_mut() {
                        t.current_row.push(std::mem::take(&mut t.current_cell));
                        t.capturing_cell = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(t) = table.as_mut() {
                        let row = std::mem::take(&mut t.current_row);
                        if t.in_header {
                            t.header = row;
                            t.in_header = false;
                        } else {
                            t.body.push(row);
                        }
                    }
                }
                TagEnd::TableHead => {}
                TagEnd::Table => {
                    if let Some(t) = table.take() {
                        render_table(buffer, &t);
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some(t) = table.as_mut().filter(|t| t.capturing_cell) {
                    t.current_cell.push_str(&text);
                } else if in_code_block {
                    insert_tagged(buffer, &text, &["md-codeblock"]);
                } else {
                    insert_tagged(buffer, &text, &inline_tags);
                }
            }
            Event::Code(code) => {
                if let Some(t) = table.as_mut().filter(|t| t.capturing_cell) {
                    t.current_cell.push_str(&code);
                } else {
                    let mut tags = inline_tags.clone();
                    tags.push("md-code");
                    insert_tagged(buffer, &code, &tags);
                }
            }
            Event::SoftBreak => {
                if let Some(t) = table.as_mut().filter(|t| t.capturing_cell) {
                    t.current_cell.push(' ');
                } else if in_code_block {
                    insert_tagged(buffer, "\n", &["md-codeblock"]);
                } else {
                    insert_plain(buffer, " ");
                }
            }
            Event::HardBreak => {
                if let Some(t) = table.as_mut().filter(|t| t.capturing_cell) {
                    t.current_cell.push(' ');
                } else {
                    insert_plain(buffer, "\n");
                }
            }
            Event::Rule => {
                ensure_block_gap(buffer);
                insert_tagged(buffer, "────────────────────────────────\n", &["md-hr"]);
            }
            Event::TaskListMarker(checked) => {
                insert_tagged(
                    buffer,
                    if checked { "☑ " } else { "☐ " },
                    &["md-list-marker"],
                );
            }
            _ => {}
        }
    }
}

#[derive(Default)]
struct TableBuild {
    header: Vec<String>,
    body: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
    in_header: bool,
    capturing_cell: bool,
}

fn render_table(buffer: &gtk::TextBuffer, table: &TableBuild) {
    let cols = table
        .header
        .len()
        .max(table.body.iter().map(|r| r.len()).max().unwrap_or(0));
    if cols == 0 {
        return;
    }

    let mut widths = vec![0usize; cols];
    let bump = |widths: &mut [usize], row: &[String]| {
        for (i, cell) in row.iter().enumerate().take(cols) {
            widths[i] = widths[i].max(display_width(cell));
        }
    };
    bump(&mut widths, &table.header);
    for row in &table.body {
        bump(&mut widths, row);
    }

    let format_row = |row: &[String]| -> String {
        let mut line = String::from("│ ");
        for i in 0..cols {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let pad = widths[i].saturating_sub(display_width(cell));
            line.push_str(cell);
            line.push_str(&" ".repeat(pad));
            line.push_str(" │ ");
        }
        line.push('\n');
        line
    };

    let mut top = String::from("┌─");
    for (i, w) in widths.iter().enumerate() {
        top.push_str(&"─".repeat(*w));
        if i + 1 < cols {
            top.push_str("─┬─");
        }
    }
    top.push_str("─┐\n");

    let mut rule = String::from("├─");
    for (i, w) in widths.iter().enumerate() {
        rule.push_str(&"─".repeat(*w));
        if i + 1 < cols {
            rule.push_str("─┼─");
        }
    }
    rule.push_str("─┤\n");

    let mut bottom = String::from("└─");
    for (i, w) in widths.iter().enumerate() {
        bottom.push_str(&"─".repeat(*w));
        if i + 1 < cols {
            bottom.push_str("─┴─");
        }
    }
    bottom.push_str("─┘\n");

    insert_tagged(buffer, &top, &["md-table"]);
    if !table.header.is_empty() {
        insert_tagged(
            buffer,
            &format_row(&table.header),
            &["md-table", "md-table-header"],
        );
        insert_tagged(buffer, &rule, &["md-table"]);
    }
    for row in &table.body {
        insert_tagged(buffer, &format_row(row), &["md-table"]);
    }
    insert_tagged(buffer, &bottom, &["md-table"]);
    insert_plain(buffer, "\n");
}

fn display_width(s: &str) -> usize {
    s.chars().count().max(1)
}

fn heading_tag(level: u32) -> &'static str {
    match level {
        1 => "md-h1",
        2 => "md-h2",
        3 => "md-h3",
        4 => "md-h4",
        _ => "md-h5",
    }
}

fn ensure_tags(buffer: &gtk::TextBuffer) {
    let table = buffer.tag_table();
    if table.lookup("md-h1").is_some() {
        return;
    }

    let add = |name: &str, configure: &dyn Fn(&gtk::TextTag)| {
        let tag = gtk::TextTag::new(Some(name));
        configure(&tag);
        table.add(&tag);
    };

    add("md-h1", &|t| {
        t.set_weight(700);
        t.set_scale(1.55);
        t.set_pixels_above_lines(10);
        t.set_pixels_below_lines(4);
    });
    add("md-h2", &|t| {
        t.set_weight(700);
        t.set_scale(1.35);
        t.set_pixels_above_lines(8);
        t.set_pixels_below_lines(3);
    });
    add("md-h3", &|t| {
        t.set_weight(700);
        t.set_scale(1.2);
        t.set_pixels_above_lines(6);
        t.set_pixels_below_lines(2);
    });
    add("md-h4", &|t| {
        t.set_weight(700);
        t.set_scale(1.1);
        t.set_pixels_above_lines(4);
        t.set_pixels_below_lines(2);
    });
    add("md-h5", &|t| {
        t.set_weight(700);
        t.set_scale(1.05);
    });
    add("md-strong", &|t| {
        t.set_weight(700);
    });
    add("md-em", &|t| {
        t.set_style(pango::Style::Italic);
    });
    add("md-strike", &|t| {
        t.set_strikethrough(true);
    });
    add("md-code", &|t| {
        t.set_family(Some("monospace"));
        t.set_scale(0.95);
    });
    add("md-codeblock", &|t| {
        t.set_family(Some("monospace"));
        t.set_scale(0.95);
        t.set_left_margin(12);
        t.set_pixels_above_lines(2);
        t.set_pixels_below_lines(2);
    });
    add("md-code-lang", &|t| {
        t.set_family(Some("monospace"));
        t.set_scale(0.85);
        t.set_style(pango::Style::Italic);
        t.set_left_margin(12);
    });
    add("md-link", &|t| {
        t.set_underline(pango::Underline::Single);
        t.set_foreground(Some("#6cb6ff"));
    });
    add("md-link-url", &|t| {
        t.set_scale(0.85);
        t.set_foreground(Some("#8a8a8a"));
    });
    add("md-quote", &|t| {
        t.set_style(pango::Style::Italic);
        t.set_left_margin(16);
        t.set_foreground(Some("#b0b0b0"));
    });
    add("md-list-marker", &|t| {
        t.set_weight(700);
    });
    add("md-hr", &|t| {
        t.set_foreground(Some("#666666"));
        t.set_pixels_above_lines(6);
        t.set_pixels_below_lines(6);
    });
    add("md-table", &|t| {
        t.set_family(Some("monospace"));
        t.set_scale(0.92);
    });
    add("md-table-header", &|t| {
        t.set_weight(700);
    });
}

fn tag_refs(buffer: &gtk::TextBuffer, names: &[&str]) -> Vec<gtk::TextTag> {
    let table = buffer.tag_table();
    names.iter().filter_map(|n| table.lookup(n)).collect()
}

fn insert_plain(buffer: &gtk::TextBuffer, text: &str) {
    let mut end = buffer.end_iter();
    buffer.insert(&mut end, text);
}

fn insert_tagged(buffer: &gtk::TextBuffer, text: &str, tag_names: &[&str]) {
    if text.is_empty() {
        return;
    }
    let tags = tag_refs(buffer, tag_names);
    let mut end = buffer.end_iter();
    if tags.is_empty() {
        buffer.insert(&mut end, text);
    } else {
        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
        buffer.insert_with_tags(&mut end, text, &refs);
    }
}

fn ensure_block_gap(buffer: &gtk::TextBuffer) {
    let end = buffer.end_iter();
    if end.offset() == 0 {
        return;
    }
    let text = buffer.text(&buffer.start_iter(), &end, false);
    if !text.ends_with('\n') {
        insert_plain(buffer, "\n");
    }
}
