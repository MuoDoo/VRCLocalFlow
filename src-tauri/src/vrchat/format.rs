use unicode_width::UnicodeWidthStr;

/// Maximum inner width in display columns.
/// VRChat chatbox limit is ~144 chars. Two-line format: "original\ntranslated".
/// 1 char for newline → 143 chars for content → ~70 per line.
pub const MAX_INNER_WIDTH: usize = 70;

/// Build a simple two-line chatbox message: original on top, translation below.
pub fn format_chatbox(original: &str, translated: &str, inner_width: usize) -> String {
    let line1 = truncate_to_width(original, inner_width);
    let line2 = truncate_to_width(translated, inner_width);
    format!("{line1}\n{line2}")
}

/// Return the display width of a string (CJK chars count as 2 columns).
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate or pad a string to exactly `target_width` display columns.
pub fn pad_to_width(s: &str, target_width: usize) -> String {
    let w = display_width(s);
    if w <= target_width {
        // Pad with spaces
        let pad = target_width - w;
        format!("{s}{}", " ".repeat(pad))
    } else {
        // Truncate to fit
        truncate_to_width(s, target_width)
    }
}

/// Truncate a string to at most `max_width` display columns, padding with spaces if shorter.
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut current_width = 0;
    for ch in s.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > max_width {
            break;
        }
        result.push(ch);
        current_width += ch_width;
    }
    // Pad if we stopped short (e.g., CJK char didn't fit in remaining 1 column)
    if current_width < max_width {
        let pad = max_width - current_width;
        result.push_str(&" ".repeat(pad));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
    }

    #[test]
    fn test_display_width_cjk() {
        assert_eq!(display_width("你好"), 4);
    }

    #[test]
    fn test_pad_short_string() {
        let padded = pad_to_width("hi", 10);
        assert_eq!(display_width(&padded), 10);
        assert_eq!(padded, "hi        ");
    }

    #[test]
    fn test_pad_long_string() {
        let padded = pad_to_width("hello world this is long", 10);
        assert_eq!(display_width(&padded), 10);
    }

    #[test]
    fn test_format_chatbox() {
        let result = format_chatbox("hi", "你好", 10);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "hi        ");
        assert_eq!(lines[1], "你好      ");
    }
}
