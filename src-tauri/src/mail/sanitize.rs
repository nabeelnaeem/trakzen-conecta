//! HTML mail is untrusted input. We strip it down before it ever reaches the
//! webview; the reading pane additionally renders it inside a sandboxed
//! iframe with no script permission, so this is defence in depth rather than
//! the only line.

use std::collections::HashSet;

use ammonia::{Builder, UrlRelative};

pub fn html(input: &str) -> String {
    let mut b = Builder::default();
    b.add_tags([
        "center", "font", "u", "s", "strike", "small", "big", "tt", "map", "area", "picture",
        "source", "html", "body",
    ])
    .add_generic_attributes([
        "style", "class", "id", "width", "height", "align", "valign", "bgcolor", "border",
        "cellpadding", "cellspacing", "color", "face", "size", "dir", "lang", "role", "nowrap",
        "colspan", "rowspan", "background",
    ])
    .add_tag_attributes("img", ["srcset", "sizes", "loading"])
    .add_tag_attributes("a", ["target"])
    .add_tag_attributes("source", ["srcset", "media", "type"])
    // Mail clients love `<a href="..." target="_blank">`; with the iframe
    // sandbox links are blocked anyway unless the user chooses to open them.
    .link_rel(Some("noopener noreferrer"))
    .url_schemes(HashSet::from(["http", "https", "mailto", "cid", "data"]))
    .url_relative(UrlRelative::Deny);
    b.clean(input).to_string()
}

pub fn text_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<br>");
        }
        out.push_str(&escape(line.trim_end_matches('\r')));
    }
    out
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn html_to_text(html: &str) -> String {
    html2text::from_read(html.as_bytes(), 100).unwrap_or_default()
}
