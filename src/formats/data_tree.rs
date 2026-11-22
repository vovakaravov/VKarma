use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum DataNode {
    Object(Vec<(String, DataNode)>),
    Array(Vec<DataNode>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataValueKind {
    String,
    Number,
    Bool,
    Null,
    Container,
}

#[derive(Debug, Clone)]
pub struct FlatTreeRow {
    pub path: String,
    pub depth: usize,
    pub key: String,
    pub kind_badge: &'static str,
    pub value_preview: String,
    pub copy_value: String,
    pub is_container: bool,
    pub collapsed: bool,
    pub value_kind: DataValueKind,
}

pub fn is_structured_lang(lang: &str) -> bool {
    matches!(lang, "json" | "yaml" | "toml")
}

pub fn parse_structured_data(lang: &str, raw: &str) -> Result<DataNode, String> {
    match lang {
        "json" => {
            let cleaned = strip_jsonc(raw);
            let val: serde_json::Value =
                serde_json::from_str(&cleaned).map_err(|e| format!("JSON error: {e}"))?;
            Ok(from_json_value(val))
        }
        "toml" => {
            let val: toml::Value =
                toml::from_str(raw).map_err(|e| format!("TOML error: {e}"))?;
            Ok(from_toml_value(val))
        }
        "yaml" => parse_yaml(raw),
        _ => Err(format!("Unsupported structured format: {lang}")),
    }
}

fn from_json_value(v: serde_json::Value) -> DataNode {
    match v {
        serde_json::Value::Null => DataNode::Null,
        serde_json::Value::Bool(b) => DataNode::Bool(b),
        serde_json::Value::Number(n) => DataNode::Number(n.to_string()),
        serde_json::Value::String(s) => DataNode::String(s),
        serde_json::Value::Array(arr) => {
            DataNode::Array(arr.into_iter().map(from_json_value).collect())
        }
        serde_json::Value::Object(map) => DataNode::Object(
            map.into_iter()
                .map(|(k, v)| (k, from_json_value(v)))
                .collect(),
        ),
    }
}

fn from_toml_value(v: toml::Value) -> DataNode {
    match v {
        toml::Value::String(s) => DataNode::String(s),
        toml::Value::Integer(i) => DataNode::Number(i.to_string()),
        toml::Value::Float(f) => DataNode::Number(f.to_string()),
        toml::Value::Boolean(b) => DataNode::Bool(b),
        toml::Value::Datetime(d) => DataNode::String(d.to_string()),
        toml::Value::Array(arr) => DataNode::Array(arr.into_iter().map(from_toml_value).collect()),
        toml::Value::Table(tbl) => DataNode::Object(
            tbl.into_iter()
                .map(|(k, v)| (k, from_toml_value(v)))
                .collect(),
        ),
    }
}

/// Strips `//` line comments, `/* ... */` block comments, and trailing commas before `}` or `]`,
/// respecting double-quoted JSON strings.
fn strip_jsonc(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push_str(&raw[start..i.min(bytes.len())]);
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if b == b',' {
            // Look ahead past whitespace/comments to see if the next significant token is `}` or `]`
            let next_sig = next_significant_jsonc_byte(bytes, i + 1);
            if matches!(next_sig, Some(b'}' | b']')) {
                i += 1;
                continue;
            }
        }
        let ch_len = raw[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&raw[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn next_significant_jsonc_byte(bytes: &[u8], mut i: usize) -> Option<u8> {
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else {
            return Some(b);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Clean pure-Rust YAML parser for mappings, sequences, flow values & scalars
// ---------------------------------------------------------------------------

struct YamlLine {
    indent: usize,
    text: String,
    line_no: usize,
}

fn strip_yaml_comment(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut in_sq = false;
    let mut in_dq = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_sq = false;
            }
        } else {
            match b {
                b'\'' => in_sq = true,
                b'"' => in_dq = true,
                b'#' if i == 0 || bytes[i - 1].is_ascii_whitespace() => {
                    return s[..i].trim_end();
                }
                _ => {}
            }
        }
        i += 1;
    }
    s.trim_end()
}

fn find_unquoted_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_sq = false;
    let mut in_dq = false;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\'' {
                if bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                in_sq = false;
            }
        } else {
            match b {
                b'"' => in_dq = true,
                b'\'' => in_sq = true,
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b':' if brace_depth == 0
                    && bracket_depth == 0
                    && matches!(bytes.get(i + 1), None | Some(b' ' | b'\t')) =>
                {
                    return Some(i);
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn unquote_yaml_key(raw: &str) -> String {
    let t = raw.trim();
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        unescape_double_quoted(inner)
    } else if let Some(inner) = t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        t.to_string()
    }
}

fn unescape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => out.push(other),
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn split_flow_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let bytes = inner.as_bytes();
    let mut in_sq = false;
    let mut in_dq = false;
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_dq {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_dq = false;
            }
        } else if in_sq {
            if b == b'\'' {
                in_sq = false;
            }
        } else {
            match b {
                b'"' => in_dq = true,
                b'\'' => in_sq = true,
                b'[' | b'{' => depth += 1,
                b']' | b'}' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    let slice = inner[start..i].trim();
                    if !slice.is_empty() {
                        items.push(slice);
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        items.push(tail);
    }
    items
}

fn parse_yaml_scalar_or_flow(raw: &str) -> DataNode {
    let t = raw.trim();
    if t.is_empty() || matches!(t, "null" | "~" | "Null" | "NULL") {
        return DataNode::Null;
    }
    if matches!(t, "true" | "True" | "TRUE") {
        return DataNode::Bool(true);
    }
    if matches!(t, "false" | "False" | "FALSE") {
        return DataNode::Bool(false);
    }
    if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let items = split_flow_items(inner)
            .into_iter()
            .map(parse_yaml_scalar_or_flow)
            .collect();
        return DataNode::Array(items);
    }
    if let Some(inner) = t.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let mut entries = Vec::new();
        for item in split_flow_items(inner) {
            if let Some(colon) = find_unquoted_colon(item) {
                let k = unquote_yaml_key(&item[..colon]);
                let v = parse_yaml_scalar_or_flow(&item[colon + 1..]);
                entries.push((k, v));
            }
        }
        return DataNode::Object(entries);
    }
    if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return DataNode::String(unescape_double_quoted(inner));
    }
    if let Some(inner) = t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return DataNode::String(inner.replace("''", "'"));
    }
    if t.parse::<i64>().is_ok() || t.parse::<f64>().is_ok() {
        return DataNode::Number(t.to_string());
    }
    DataNode::String(t.to_string())
}

fn parse_yaml(raw: &str) -> Result<DataNode, String> {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let mut lines: Vec<YamlLine> = Vec::new();
    for (idx, line) in raw_lines.iter().enumerate() {
        if line.contains('\t') && line.trim_start() != *line && line.starts_with('\t') {
            return Err(format!("YAML error at line {}: tabs are not valid indentation", idx + 1));
        }
        let clean = strip_yaml_comment(line);
        let trimmed = clean.trim();
        if trimmed.is_empty() || trimmed == "---" || trimmed == "..." {
            continue;
        }
        let indent = clean.len() - clean.trim_start_matches(' ').len();
        lines.push(YamlLine {
            indent,
            text: clean[indent..].to_string(),
            line_no: idx + 1,
        });
    }

    if lines.is_empty() {
        return Ok(DataNode::Object(Vec::new()));
    }

    let mut pos = 0usize;
    parse_yaml_block(&lines, &raw_lines, &mut pos, lines[0].indent)
}

fn is_seq_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

fn parse_yaml_block(
    lines: &[YamlLine],
    raw_lines: &[&str],
    pos: &mut usize,
    expected_indent: usize,
) -> Result<DataNode, String> {
    if *pos >= lines.len() {
        return Ok(DataNode::Null);
    }
    if is_seq_item(&lines[*pos].text) {
        parse_yaml_sequence(lines, raw_lines, pos, expected_indent)
    } else if find_unquoted_colon(&lines[*pos].text).is_some() {
        parse_yaml_mapping(lines, raw_lines, pos, expected_indent)
    } else {
        let val = parse_yaml_scalar_or_flow(&lines[*pos].text);
        *pos += 1;
        Ok(val)
    }
}

fn consume_block_scalar(
    lines: &[YamlLine],
    raw_lines: &[&str],
    pos: &mut usize,
    parent_indent: usize,
    start_line_no: usize,
    folded: bool,
) -> String {
    let min_indent = if *pos < lines.len() && lines[*pos].indent > parent_indent {
        lines[*pos].indent
    } else {
        return String::new();
    };
    let end_line_no = {
        let mut p = *pos;
        while p < lines.len() && lines[p].indent >= min_indent {
            p += 1;
        }
        let end = if p < lines.len() {
            lines[p].line_no - 1
        } else {
            raw_lines.len()
        };
        *pos = p;
        end
    };

    let mut out = Vec::new();
    for line in &raw_lines[start_line_no..end_line_no] {
        if line.trim().is_empty() {
            out.push(String::new());
        } else if line.len() >= min_indent {
            out.push(line[min_indent..].to_string());
        } else {
            out.push(line.trim_start().to_string());
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    if folded {
        out.join(" ")
    } else {
        out.join("\n")
    }
}

fn parse_yaml_mapping(
    lines: &[YamlLine],
    raw_lines: &[&str],
    pos: &mut usize,
    expected_indent: usize,
) -> Result<DataNode, String> {
    let mut entries = Vec::new();
    while *pos < lines.len() {
        let cur = &lines[*pos];
        if cur.indent < expected_indent || is_seq_item(&cur.text) {
            break;
        }
        let Some(colon) = find_unquoted_colon(&cur.text) else {
            return Err(format!(
                "YAML error at line {}: expected `key: value`",
                cur.line_no
            ));
        };
        let key = unquote_yaml_key(&cur.text[..colon]);
        let rest = cur.text[colon + 1..].trim();
        let line_no = cur.line_no;
        let cur_indent = cur.indent;
        *pos += 1;

        let val = if matches!(rest, "|" | "|-" | "|+" | ">" | ">-" | ">+") {
            let folded = rest.starts_with('>');
            DataNode::String(consume_block_scalar(
                lines, raw_lines, pos, cur_indent, line_no, folded,
            ))
        } else if rest.is_empty() {
            if *pos < lines.len()
                && (lines[*pos].indent > cur_indent
                    || (lines[*pos].indent == cur_indent && is_seq_item(&lines[*pos].text)))
            {
                let child_indent = lines[*pos].indent;
                parse_yaml_block(lines, raw_lines, pos, child_indent)?
            } else {
                DataNode::Null
            }
        } else {
            parse_yaml_scalar_or_flow(rest)
        };
        entries.push((key, val));
    }
    Ok(DataNode::Object(entries))
}

fn parse_yaml_sequence(
    lines: &[YamlLine],
    raw_lines: &[&str],
    pos: &mut usize,
    expected_indent: usize,
) -> Result<DataNode, String> {
    let mut items = Vec::new();
    while *pos < lines.len() {
        let cur = &lines[*pos];
        if cur.indent < expected_indent || !is_seq_item(&cur.text) {
            break;
        }
        let cur_indent = cur.indent;
        let line_no = cur.line_no;
        let rest = cur.text.strip_prefix('-').unwrap_or("").trim();
        *pos += 1;

        let item = if matches!(rest, "|" | "|-" | ">" | ">-") {
            let folded = rest.starts_with('>');
            DataNode::String(consume_block_scalar(
                lines, raw_lines, pos, cur_indent, line_no, folded,
            ))
        } else if rest.is_empty() {
            if *pos < lines.len() && lines[*pos].indent > cur_indent {
                let child_indent = lines[*pos].indent;
                parse_yaml_block(lines, raw_lines, pos, child_indent)?
            } else {
                DataNode::Null
            }
        } else if let Some(colon) = find_unquoted_colon(rest) {
            // Inline mapping started on the `- key: val` line
            let inline_indent = cur_indent + 2;
            let k = unquote_yaml_key(&rest[..colon]);
            let v_rest = rest[colon + 1..].trim();
            let first_val = if matches!(v_rest, "|" | "|-" | ">" | ">-") {
                let folded = v_rest.starts_with('>');
                DataNode::String(consume_block_scalar(
                    lines,
                    raw_lines,
                    pos,
                    inline_indent,
                    line_no,
                    folded,
                ))
            } else if v_rest.is_empty() {
                if *pos < lines.len() && lines[*pos].indent > inline_indent {
                    let child_indent = lines[*pos].indent;
                    parse_yaml_block(lines, raw_lines, pos, child_indent)?
                } else {
                    DataNode::Null
                }
            } else {
                parse_yaml_scalar_or_flow(v_rest)
            };
            let mut entries = vec![(k, first_val)];
            if *pos < lines.len()
                && lines[*pos].indent >= inline_indent
                && !is_seq_item(&lines[*pos].text)
                && find_unquoted_colon(&lines[*pos].text).is_some()
            {
                let sibling_indent = lines[*pos].indent;
                if let DataNode::Object(more) =
                    parse_yaml_mapping(lines, raw_lines, pos, sibling_indent)?
                {
                    entries.extend(more);
                }
            }
            DataNode::Object(entries)
        } else {
            parse_yaml_scalar_or_flow(rest)
        };
        items.push(item);
    }
    Ok(DataNode::Array(items))
}

// ---------------------------------------------------------------------------
// Flattened tree & table extraction for GPUI rendering
// ---------------------------------------------------------------------------

pub fn collect_container_paths(node: &DataNode, path: &str, out: &mut HashSet<String>) {
    match node {
        DataNode::Object(entries) => {
            if !path.is_empty() {
                out.insert(path.to_string());
            }
            for (k, v) in entries {
                let cp = if path.is_empty() {
                    format!("$.{k}")
                } else {
                    format!("{path}.{k}")
                };
                collect_container_paths(v, &cp, out);
            }
        }
        DataNode::Array(items) => {
            if !path.is_empty() {
                out.insert(path.to_string());
            }
            for (ix, v) in items.iter().enumerate() {
                let cp = if path.is_empty() {
                    format!("$[{ix}]")
                } else {
                    format!("{path}[{ix}]")
                };
                collect_container_paths(v, &cp, out);
            }
        }
        _ => {}
    }
}

pub fn flatten_tree(
    root: &DataNode,
    collapsed: &HashSet<String>,
    filter: &str,
    max_rows: usize,
) -> Vec<FlatTreeRow> {
    let q = filter.trim().to_ascii_lowercase();
    let mut rows = Vec::new();
    match root {
        DataNode::Object(entries) => {
            for (k, v) in entries {
                if rows.len() >= max_rows {
                    break;
                }
                let path = format!("$.{k}");
                push_tree_node(k, &path, 0, v, collapsed, &q, max_rows, &mut rows);
            }
        }
        DataNode::Array(items) => {
            for (ix, v) in items.iter().enumerate() {
                if rows.len() >= max_rows {
                    break;
                }
                let path = format!("$[{ix}]");
                push_tree_node(
                    &format!("[{ix}]"),
                    &path,
                    0,
                    v,
                    collapsed,
                    &q,
                    max_rows,
                    &mut rows,
                );
            }
        }
        scalar => {
            push_tree_node("root", "$", 0, scalar, collapsed, &q, max_rows, &mut rows);
        }
    }
    rows
}

fn node_matches_filter(key: &str, node: &DataNode, q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    if key.to_ascii_lowercase().contains(q) {
        return true;
    }
    match node {
        DataNode::String(s) | DataNode::Number(s) => s.to_ascii_lowercase().contains(q),
        DataNode::Bool(b) => (if *b { "true" } else { "false" }).contains(q),
        DataNode::Null => "null".contains(q),
        DataNode::Object(entries) => entries
            .iter()
            .any(|(k, v)| node_matches_filter(k, v, q)),
        DataNode::Array(items) => items
            .iter()
            .enumerate()
            .any(|(ix, v)| node_matches_filter(&format!("[{ix}]"), v, q)),
    }
}

#[allow(clippy::too_many_arguments)]
fn push_tree_node(
    key: &str,
    path: &str,
    depth: usize,
    node: &DataNode,
    collapsed: &HashSet<String>,
    q: &str,
    max_rows: usize,
    out: &mut Vec<FlatTreeRow>,
) {
    if out.len() >= max_rows || !node_matches_filter(key, node, q) {
        return;
    }

    let is_collapsed = collapsed.contains(path) && q.is_empty();
    match node {
        DataNode::Object(entries) => {
            out.push(FlatTreeRow {
                path: path.to_string(),
                depth,
                key: key.to_string(),
                kind_badge: "{}",
                value_preview: format!("{} keys", entries.len()),
                copy_value: path.to_string(),
                is_container: !entries.is_empty(),
                collapsed: is_collapsed,
                value_kind: DataValueKind::Container,
            });
            if !is_collapsed {
                for (ck, cv) in entries {
                    if out.len() >= max_rows {
                        break;
                    }
                    let cp = format!("{path}.{ck}");
                    push_tree_node(ck, &cp, depth + 1, cv, collapsed, q, max_rows, out);
                }
            }
        }
        DataNode::Array(items) => {
            out.push(FlatTreeRow {
                path: path.to_string(),
                depth,
                key: key.to_string(),
                kind_badge: "[]",
                value_preview: format!("{} items", items.len()),
                copy_value: path.to_string(),
                is_container: !items.is_empty(),
                collapsed: is_collapsed,
                value_kind: DataValueKind::Container,
            });
            if !is_collapsed {
                for (ix, cv) in items.iter().enumerate() {
                    if out.len() >= max_rows {
                        break;
                    }
                    let cp = format!("{path}[{ix}]");
                    push_tree_node(
                        &format!("[{ix}]"),
                        &cp,
                        depth + 1,
                        cv,
                        collapsed,
                        q,
                        max_rows,
                        out,
                    );
                }
            }
        }
        DataNode::String(s) => {
            let preview = if s.chars().count() > 140 {
                let trunc: String = s.chars().take(140).collect();
                format!("\"{trunc}…\"")
            } else {
                format!("\"{s}\"")
            };
            out.push(FlatTreeRow {
                path: path.to_string(),
                depth,
                key: key.to_string(),
                kind_badge: "str",
                value_preview: preview,
                copy_value: s.clone(),
                is_container: false,
                collapsed: false,
                value_kind: DataValueKind::String,
            });
        }
        DataNode::Number(n) => {
            out.push(FlatTreeRow {
                path: path.to_string(),
                depth,
                key: key.to_string(),
                kind_badge: "num",
                value_preview: n.clone(),
                copy_value: n.clone(),
                is_container: false,
                collapsed: false,
                value_kind: DataValueKind::Number,
            });
        }
        DataNode::Bool(b) => {
            let s = if *b { "true" } else { "false" }.to_string();
            out.push(FlatTreeRow {
                path: path.to_string(),
                depth,
                key: key.to_string(),
                kind_badge: "bool",
                value_preview: s.clone(),
                copy_value: s,
                is_container: false,
                collapsed: false,
                value_kind: DataValueKind::Bool,
            });
        }
        DataNode::Null => {
            out.push(FlatTreeRow {
                path: path.to_string(),
                depth,
                key: key.to_string(),
                kind_badge: "null",
                value_preview: "null".to_string(),
                copy_value: "null".to_string(),
                is_container: false,
                collapsed: false,
                value_kind: DataValueKind::Null,
            });
        }
    }
}

fn scalar_cell_string(node: &DataNode) -> String {
    match node {
        DataNode::String(s) => s.clone(),
        DataNode::Number(n) => n.clone(),
        DataNode::Bool(b) => b.to_string(),
        DataNode::Null => "null".to_string(),
        DataNode::Array(a) => format!("[{}]", a.len()),
        DataNode::Object(o) => format!("{{{}}}", o.len()),
    }
}

pub fn extract_table(root: &DataNode) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let items: &[DataNode] = match root {
        DataNode::Array(arr) => arr.as_slice(),
        DataNode::Object(entries) => entries
            .iter()
            .find_map(|(_, v)| match v {
                DataNode::Array(arr) if arr.iter().any(|it| matches!(it, DataNode::Object(_))) => {
                    Some(arr.as_slice())
                }
                _ => None,
            })?,
        _ => return None,
    };

    if items.is_empty() {
        return None;
    }

    let mut cols: Vec<String> = Vec::new();
    for item in items.iter().take(250) {
        if let DataNode::Object(entries) = item {
            for (k, _) in entries {
                if !cols.contains(k) && cols.len() < 32 {
                    cols.push(k.clone());
                }
            }
        }
    }
    if cols.is_empty() {
        return None;
    }

    let mut rows = Vec::new();
    for item in items.iter().take(500) {
        if let DataNode::Object(entries) = item {
            let row = cols
                .iter()
                .map(|c| {
                    entries
                        .iter()
                        .find(|(k, _)| k == c)
                        .map(|(_, v)| scalar_cell_string(v))
                        .unwrap_or_default()
                })
                .collect();
            rows.push(row);
        }
    }
    Some((cols, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonc_toml_and_yaml_and_extracts_table() {
        let jsonc = r#"{
            // Line comment
            "name": "VKarma", /* block */
            "items": [
                {"id": 1, "ok": true,},
                {"id": 2, "extra": "hi",},
            ],
        }"#;
        let node = parse_structured_data("json", jsonc).unwrap();
        let (cols, rows) = extract_table(&node).unwrap();
        assert_eq!(cols, vec!["id", "ok", "extra"]);
        assert_eq!(rows[0], vec!["1", "true", ""]);
        assert_eq!(rows[1], vec!["2", "", "hi"]);

        let toml_src = "[package]\nname = \"vkarma\"\nports = [80, 443]\n";
        let tnode = parse_structured_data("toml", toml_src).unwrap();
        let flat = flatten_tree(&tnode, &HashSet::new(), "443", 500);
        assert!(flat.iter().any(|r| r.value_preview == "443"));

        let yaml_src = "\
---
app:
  name: VKarma
  tags: [notes, typst]
  notes: |
    Line A
    Line B
servers:
  - host: localhost
    port: 8080
  - host: prod
    port: 443
";
        let ynode = parse_structured_data("yaml", yaml_src).unwrap();
        let (ycols, yrows) = extract_table(&ynode).unwrap();
        assert_eq!(ycols, vec!["host", "port"]);
        assert_eq!(yrows[0], vec!["localhost", "8080"]);
        assert_eq!(yrows[1], vec!["prod", "443"]);

        let flat_all = flatten_tree(&ynode, &HashSet::new(), "", 500);
        assert!(flat_all.iter().any(|r| r.key == "notes" && r.value_preview.contains("Line A")));
    }

    #[test]
    fn flatten_tree_respects_collapsed_paths() {
        let node = parse_structured_data("json", r#"{"a":{"b":1,"c":2},"d":3}"#).unwrap();
        let mut collapsed = HashSet::new();
        collapsed.insert("$.a".to_string());
        let rows = flatten_tree(&node, &collapsed, "", 500);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "a");
        assert!(rows[0].collapsed);
        assert_eq!(rows[1].key, "d");
    }
}
