pub fn sanitize_assistant_text(content: &str) -> String {
    let without_hidden_blocks = strip_assistant_hidden_blocks(content);
    without_hidden_blocks
        .lines()
        .filter(|line| !is_assistant_control_line(line))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

pub fn strip_assistant_hidden_blocks(content: &str) -> String {
    let mut remaining = content;
    let mut output = String::new();

    while let Some((start, tag_name, open_end)) = find_hidden_block_open(remaining) {
        output.push_str(&remaining[..start]);
        let close = format!("</{tag_name}>");
        let after_open = &remaining[open_end..];
        let Some(close_start) = after_open.find(&close) else {
            return output.trim().to_owned();
        };
        remaining = &after_open[close_start + close.len()..];
    }

    output.push_str(remaining);
    output
}

fn find_hidden_block_open(content: &str) -> Option<(usize, String, usize)> {
    let mut search_from = 0usize;
    while let Some(relative_start) = content[search_from..].find('<') {
        let start = search_from + relative_start;
        let after_lt = &content[start + 1..];
        if after_lt.starts_with('/') || after_lt.starts_with('!') || after_lt.starts_with('?') {
            search_from = start + 1;
            continue;
        }
        let Some(open_end_relative) = after_lt.find('>') else {
            return None;
        };
        let tag_header = &after_lt[..open_end_relative];
        let Some(tag_name) = tag_header.split_whitespace().next() else {
            search_from = start + 1;
            continue;
        };
        if is_hidden_assistant_tag(tag_name) {
            return Some((
                start,
                tag_name.to_owned(),
                start + 1 + open_end_relative + 1,
            ));
        }
        search_from = start + 1;
    }
    None
}

fn is_hidden_assistant_tag(tag_name: &str) -> bool {
    let local_name = tag_name
        .rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(tag_name)
        .to_ascii_lowercase();
    local_name == "think" || local_name == "tool_call" || local_name == "invoke"
}

fn is_assistant_control_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed
        .strip_prefix('$')
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|command| {
            !command.is_empty()
                && command
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch == '_')
        })
    {
        return true;
    }
    if let Some(tag_name) = xml_like_single_line_tag_name(trimmed) {
        let local_name = tag_name
            .rsplit_once(':')
            .map(|(_, local)| local)
            .unwrap_or(tag_name);
        return matches!(local_name, "parameter");
    }
    false
}

fn xml_like_single_line_tag_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('<')?;
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let end = rest.find([' ', '>'])?;
    Some(&rest[..end])
}
