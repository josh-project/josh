pub fn is_trailer_line(line: &str) -> bool {
    let key_len = line
        .bytes()
        .take_while(|&b| b.is_ascii_alphanumeric() || b == b'-')
        .count();
    key_len > 0 && line[key_len..].starts_with(": ")
}

/// Extract change-id metadata from a commit, preferring jj/gitbutler's custom
/// `change-id` commit-object header over any `Change:` / `Change-Id:` trailer
/// in the message body. The series list comes from message trailers regardless.
pub fn commit_change_meta(commit: &git2::Commit) -> (Option<String>, Vec<String>) {
    let (mut id, series) = parse_change_meta(commit.message().unwrap_or(""));
    if let Ok(buf) = commit.header_field_bytes("change-id") {
        if let Ok(s) = std::str::from_utf8(&buf) {
            let s = s.trim();
            if !s.is_empty() {
                id = Some(s.to_string());
            }
        }
    }
    (id, series)
}

pub fn parse_change_meta(message: &str) -> (Option<String>, Vec<String>) {
    let lines: Vec<&str> = message.lines().collect();
    let mut footer_start = lines.len();
    for (i, line) in lines.iter().enumerate().rev() {
        if line.is_empty() || is_trailer_line(line) {
            footer_start = i;
        } else {
            break;
        }
    }

    let mut id: Option<String> = None;
    let mut series: Vec<String> = Vec::new();
    for line in &lines[footer_start..] {
        if let Some(v) = line.strip_prefix("Change: ") {
            id = Some(v.to_string());
        }
        if let Some(v) = line.strip_prefix("Change-Id: ") {
            id = Some(v.to_string());
        }
        if let Some(v) = line.strip_prefix("Change-Series: ") {
            series.push(v.to_string());
        }
    }
    (id, series)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_in_body_is_ignored() {
        let (id, series) =
            parse_change_meta("Subject\n\nbody mentions Change: not-a-trailer\nmore body\n");
        assert_eq!(id, None);
        assert!(series.is_empty());
    }

    #[test]
    fn real_trailing_footer_is_parsed() {
        let (id, _) = parse_change_meta("Subject\n\nBody.\n\nChange: real-id\n");
        assert_eq!(id.as_deref(), Some("real-id"));
    }

    #[test]
    fn single_line_message_is_its_own_footer() {
        let (id, _) = parse_change_meta("Change: only-line");
        assert_eq!(id.as_deref(), Some("only-line"));
    }

    #[test]
    fn footer_followed_by_body_is_ignored() {
        let (id, _) = parse_change_meta("Subject\n\nChange: middle\n\nBody after.\n");
        assert_eq!(id, None);
    }

    #[test]
    fn other_trailers_in_block_do_not_break_change() {
        let msg = "Subject\n\nBody.\n\nSigned-off-by: x <x@y>\nChange: real\n\
                   Reviewed-by: z <z@w>\n";
        let (id, _) = parse_change_meta(msg);
        assert_eq!(id.as_deref(), Some("real"));
    }

    #[test]
    fn series_in_footer_block_is_collected() {
        let msg = "Subject\n\nBody.\n\nChange-Series: s1\nChange-Series: s2\nChange: c\n";
        let (id, series) = parse_change_meta(msg);
        assert_eq!(id.as_deref(), Some("c"));
        assert_eq!(series, vec!["s1".to_string(), "s2".to_string()]);
    }

    #[test]
    fn series_in_body_is_ignored() {
        let msg = "Subject\n\nWe discussed Change-Series: bogus here.\nmore body\n";
        let (_id, series) = parse_change_meta(msg);
        assert!(series.is_empty());
    }

    #[test]
    fn is_trailer_line_basics() {
        assert!(is_trailer_line("Change: foo"));
        assert!(is_trailer_line("Change-Id: foo"));
        assert!(is_trailer_line("Signed-off-by: a <a@b>"));
        assert!(!is_trailer_line("not a trailer"));
        assert!(!is_trailer_line("Change:no-space"));
        assert!(!is_trailer_line(": leading colon"));
        assert!(!is_trailer_line(""));
    }
}
