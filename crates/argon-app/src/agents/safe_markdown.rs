use std::path::PathBuf;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::text::TextView;
use gpui_kit::{ElementId, ImageSource, SharedString};

use super::panel_labels as labels;
use crate::review::{Confirm, confirm};

const ALLOWED_SCHEMES: [&str; 2] = ["http://", "https://"];

pub fn is_safe_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    ALLOWED_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
}

fn closing(text: &str, from: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in text[from..].char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            if depth == 0 {
                return Some(from + offset);
            }
            depth -= 1;
        }
    }
    None
}

pub fn neutralize_images(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("![") {
        let start = cursor + found;
        let image = closing(text, start + 2, '[', ']').and_then(|alt_end| {
            text[alt_end + 1..].starts_with('(').then_some(())?;
            let url_end = closing(text, alt_end + 2, '(', ')')?;
            Some((alt_end, url_end))
        });
        let Some((alt_end, url_end)) = image else {
            cursor = start + 2;
            continue;
        };
        out.push_str(&text[rest..start]);
        let alt = text[start + 2..alt_end].trim();
        let url = text[alt_end + 2..url_end].trim();
        out.push_str(&labels::blocked_image(alt, url));
        rest = url_end + 1;
        cursor = rest;
    }
    out.push_str(&text[rest..]);
    out
}

pub fn agent_markdown(id: impl Into<ElementId>, body: &str) -> TextView {
    TextView::markdown(id, SharedString::from(neutralize_images(body)))
        .selectable(true)
        .image_source(|_| ImageSource::from(PathBuf::new()))
        .on_link_click(|url, _, window, cx| {
            let url = url.to_string();
            if !is_safe_link(&url) {
                window.push_notification(Notification::warning(labels::blocked_link(&url)), cx);
                return;
            }
            let request = Confirm {
                title: labels::OPEN_LINK_TITLE,
                message: url.clone(),
                confirm: labels::OPEN_LINK,
            };
            confirm(request, window, cx, move |_, _| {
                if let Err(error) = open::that_detached(&url) {
                    tracing::warn!(%error, "failed to open agent link");
                }
            });
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_links_are_safe() {
        assert!(is_safe_link("https://example.com/a"));
        assert!(is_safe_link("HTTP://example.com"));
        assert!(!is_safe_link("https://"));
        assert!(!is_safe_link("file:///C:/Windows"));
        assert!(!is_safe_link("javascript:alert(1)"));
        assert!(!is_safe_link("vscode://open"));
    }

    #[test]
    fn images_become_plain_text() {
        let text = "antes ![logo [v2]](https://x.io/a(1).png) depois ![](data:x) fim";
        let out = neutralize_images(text);
        assert!(!out.contains("!["));
        assert!(out.starts_with("antes "));
        assert!(out.contains("logo [v2]"));
        assert!(out.contains("https://x.io/a(1).png"));
        assert!(out.ends_with(" fim"));
    }

    #[test]
    fn text_without_images_is_unchanged() {
        assert_eq!(neutralize_images("a ![b] c [d](e)"), "a ![b] c [d](e)");
    }
}
