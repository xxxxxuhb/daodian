//! 把笔记文档渲成纯文本、Markdown 和 HTML。
use serde_json::Value;

use super::Note;

pub(super) fn plain_text(node: &Value) -> String {
    let mut out = String::new();
    walk_text(node, &mut out);
    out
}

fn walk_text(node: &Value, out: &mut String) {
    let kind = node
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if kind == "text" {
        if let Some(text) = node.get("text").and_then(|value| value.as_str()) {
            out.push_str(text);
        }
        return;
    }
    if kind == "hardBreak" {
        out.push('\n');
        return;
    }
    if kind == "image" {
        out.push_str("[图片]");
    }
    if kind == "noteRef" {
        out.push_str(&attr_str(node, "title"));
    }
    if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
        for child in content {
            walk_text(child, out);
        }
    }
    if matches!(
        kind,
        "paragraph" | "heading" | "blockquote" | "codeBlock" | "listItem" | "taskItem"
    ) {
        out.push('\n');
    }
}

fn attr_str(node: &Value, key: &str) -> String {
    node.get("attrs")
        .and_then(|attrs| attrs.get(key))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

pub(super) fn render_markdown(note: &Note) -> String {
    let mut out = format!("# {}\n\n", note.title);
    if !note.tags.is_empty() {
        out.push_str(
            &note
                .tags
                .iter()
                .map(|tag| format!("#{tag}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        out.push_str("\n\n");
    }
    render_md(&note.doc, &mut out);
    out
}

fn render_md(node: &Value, out: &mut String) {
    let kind = node
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match kind {
        "doc" => render_children_md(node, out),
        "paragraph" => {
            render_inline_md(node, out);
            out.push_str("\n\n");
        }
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(|value| value.as_u64())
                .unwrap_or(1)
                .clamp(1, 6);
            for _ in 0..level {
                out.push('#');
            }
            out.push(' ');
            render_inline_md(node, out);
            out.push_str("\n\n");
        }
        "blockquote" => {
            let mut inner = String::new();
            render_children_md(node, &mut inner);
            for line in inner.lines() {
                if line.is_empty() {
                    continue;
                }
                out.push_str("> ");
                out.push_str(line.trim());
                out.push('\n');
            }
            out.push('\n');
        }
        "codeBlock" => {
            out.push_str("```\n");
            render_inline_md(node, out);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        "bulletList" => render_list_md(node, out, false),
        "orderedList" => render_list_md(node, out, true),
        "taskList" => render_tasks_md(node, out),
        "horizontalRule" => out.push_str("---\n\n"),
        "image" => {
            let alt = attr_str(node, "alt");
            let hash = attr_str(node, "hash");
            let ext = attr_str(node, "ext");
            out.push_str(&format!("![{alt}](attachment:{hash}.{ext})\n\n"));
        }
        "noteRef" => {
            out.push_str(&format!("[[{}]]\n\n", attr_str(node, "title")));
        }
        "table" => render_table_md(node, out),
        _ => render_children_md(node, out),
    }
}

fn render_children_md(node: &Value, out: &mut String) {
    if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
        for child in content {
            render_md(child, out);
        }
    }
}

fn render_inline_md(node: &Value, out: &mut String) {
    let Some(content) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for child in content {
        let kind = child
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if kind == "hardBreak" {
            out.push_str("  \n");
            continue;
        }
        if kind != "text" {
            render_md(child, out);
            continue;
        }
        let text = child
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let mut wrapped = text.to_string();
        if let Some(marks) = child.get("marks").and_then(|value| value.as_array()) {
            for mark in marks {
                let mark_type = mark
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                wrapped = match mark_type {
                    "bold" => format!("**{wrapped}**"),
                    "italic" => format!("*{wrapped}*"),
                    "strike" => format!("~~{wrapped}~~"),
                    "code" => format!("`{wrapped}`"),
                    "highlight" => format!("=={wrapped}=="),
                    "link" => {
                        let href = mark
                            .get("attrs")
                            .and_then(|attrs| attrs.get("href"))
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        format!("[{wrapped}]({href})")
                    }
                    _ => wrapped,
                };
            }
        }
        out.push_str(&wrapped);
    }
}

fn render_list_md(node: &Value, out: &mut String, ordered: bool) {
    let Some(items) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let mut inner = String::new();
        render_children_md(item, &mut inner);
        let marker = if ordered {
            format!("{}. ", index + 1)
        } else {
            "- ".to_string()
        };
        out.push_str(&marker);
        out.push_str(inner.trim());
        out.push('\n');
    }
    out.push('\n');
}

fn render_tasks_md(node: &Value, out: &mut String) {
    let Some(items) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for item in items {
        let checked = item
            .get("attrs")
            .and_then(|attrs| attrs.get("checked"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let mut inner = String::new();
        render_children_md(item, &mut inner);
        out.push_str(if checked { "- [x] " } else { "- [ ] " });
        out.push_str(inner.trim());
        out.push('\n');
    }
    out.push('\n');
}

fn render_table_md(node: &Value, out: &mut String) {
    let Some(rows) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for (index, row) in rows.iter().enumerate() {
        let mut cells = Vec::new();
        if let Some(content) = row.get("content").and_then(|value| value.as_array()) {
            for cell in content {
                let mut text = String::new();
                render_inline_md(cell, &mut text);
                cells.push(text.replace('|', "\\|"));
            }
        }
        out.push('|');
        out.push_str(&cells.join("|"));
        out.push_str("|\n");
        if index == 0 {
            out.push('|');
            out.push_str(&vec!["---"; cells.len()].join("|"));
            out.push_str("|\n");
        }
    }
    out.push('\n');
}

pub(super) fn render_html(note: &Note) -> String {
    let mut body = String::new();
    render_html_node(&note.doc, &mut body);
    format!(
        "<article><h1>{}</h1>{}</article>",
        escape(&note.title),
        body
    )
}

fn render_html_node(node: &Value, out: &mut String) {
    let kind = node
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match kind {
        "doc" => {
            if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
                for child in content {
                    render_html_node(child, out);
                }
            }
        }
        "paragraph" => wrap(out, "p", node),
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(|value| value.as_u64())
                .unwrap_or(2)
                .clamp(1, 6);
            wrap(out, &format!("h{level}"), node);
        }
        "blockquote" => wrap_block(out, "blockquote", node),
        "codeBlock" => {
            out.push_str("<pre><code>");
            out.push_str(&escape(&plain_text(node)));
            out.push_str("</code></pre>");
        }
        "bulletList" => wrap_block(out, "ul", node),
        "orderedList" => wrap_block(out, "ol", node),
        "listItem" | "taskItem" => wrap_block(out, "li", node),
        "taskList" => wrap_block(out, "ul", node),
        "horizontalRule" => out.push_str("<hr>"),
        "image" => {
            out.push_str(&format!(
                "<img alt=\"{}\" data-hash=\"{}\">",
                escape(&attr_str(node, "alt")),
                escape(&attr_str(node, "hash"))
            ));
        }
        "noteRef" => {
            out.push_str(&format!("<p>{}</p>", escape(&attr_str(node, "title"))));
        }
        "table" => wrap_block(out, "table", node),
        "tableRow" => wrap_block(out, "tr", node),
        "tableHeader" => wrap(out, "th", node),
        "tableCell" => wrap(out, "td", node),
        _ => {
            if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
                for child in content {
                    render_html_node(child, out);
                }
            }
        }
    }
}

fn wrap(out: &mut String, tag: &str, node: &Value) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    render_inline_html(node, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn wrap_block(out: &mut String, tag: &str, node: &Value) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    if let Some(content) = node.get("content").and_then(|value| value.as_array()) {
        for child in content {
            render_html_node(child, out);
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn render_inline_html(node: &Value, out: &mut String) {
    let Some(content) = node.get("content").and_then(|value| value.as_array()) else {
        return;
    };
    for child in content {
        let kind = child
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if kind == "hardBreak" {
            out.push_str("<br>");
            continue;
        }
        if kind != "text" {
            render_html_node(child, out);
            continue;
        }
        let mut text = escape(
            child
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        );
        if let Some(marks) = child.get("marks").and_then(|value| value.as_array()) {
            for mark in marks.iter().rev() {
                let mark_type = mark
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                text = match mark_type {
                    "bold" => format!("<strong>{text}</strong>"),
                    "italic" => format!("<em>{text}</em>"),
                    "underline" => format!("<u>{text}</u>"),
                    "strike" => format!("<s>{text}</s>"),
                    "code" => format!("<code>{text}</code>"),
                    "highlight" => format!("<mark>{text}</mark>"),
                    "link" => {
                        let href = escape(
                            mark.get("attrs")
                                .and_then(|attrs| attrs.get("href"))
                                .and_then(|value| value.as_str())
                                .unwrap_or(""),
                        );
                        format!("<a href=\"{href}\">{text}</a>")
                    }
                    _ => text,
                };
            }
        }
        out.push_str(&text);
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
