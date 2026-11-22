use std::collections::HashMap;

#[derive(Clone, Copy)]
struct Palette {
    bg: &'static str,
    surface: &'static str,
    surface_alt: &'static str,
    border: &'static str,
    text: &'static str,
    muted: &'static str,
    accent: &'static str,
    slice_colors: [&'static str; 6],
}

const DARK_PALETTE: Palette = Palette {
    bg: "#181616",
    surface: "#222020",
    surface_alt: "#282727",
    border: "#8ba4b0",
    text: "#c5c9c5",
    muted: "#9e9b93",
    accent: "#7fb4ca",
    slice_colors: [
        "#8ba4b0", "#87a987", "#b6927b", "#a292a3", "#c4b28a", "#c4746e",
    ],
};

const LIGHT_PALETTE: Palette = Palette {
    bg: "#f7f4ec",
    surface: "#ffffff",
    surface_alt: "#ece7dc",
    border: "#3b6e8c",
    text: "#1f2328",
    muted: "#656d76",
    accent: "#2d5a7b",
    slice_colors: [
        "#3b6e8c", "#4b7f52", "#c06c3b", "#7a5c8d", "#b5892e", "#b54848",
    ],
};

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn wrap_label(label: &str, max_chars: usize) -> Vec<String> {
    let clean = label.replace("<br/>", "\n").replace("<br>", "\n");
    let mut lines = Vec::new();
    for raw_line in clean.lines() {
        let mut cur = String::new();
        for word in raw_line.split_whitespace() {
            if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max_chars {
                lines.push(cur);
                cur = word.to_string();
            } else {
                if !cur.is_empty() {
                    cur.push(' ');
                }
                cur.push_str(word);
            }
        }
        if !cur.is_empty() || lines.is_empty() {
            lines.push(cur);
        }
    }
    if lines.len() > 4 {
        lines.truncate(4);
    }
    lines
}

fn emit_centered_text(
    out: &mut String,
    cx: f32,
    cy: f32,
    label: &str,
    font_size: f32,
    fill: &str,
    bold: bool,
) {
    let lines = wrap_label(label, 22);
    let line_h = font_size * 1.25;
    let start_y = cy - ((lines.len().saturating_sub(1) as f32) * line_h) * 0.5 + font_size * 0.34;
    let weight = if bold { " font-weight=\"bold\"" } else { "" };
    for (ix, line) in lines.iter().enumerate() {
        let ly = start_y + (ix as f32) * line_h;
        out.push_str(&format!(
            "<text x=\"{cx:.1}\" y=\"{ly:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"{font_size:.1}\"{weight} fill=\"{fill}\">{}</text>\n",
            escape_xml(line)
        ));
    }
}

fn emit_arrowhead(out: &mut String, x1: f32, y1: f32, x2: f32, y2: f32, color: &str) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = dx.hypot(dy);
    if len < 0.5 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let nx = -uy;
    let ny = ux;
    let alen = 9.0;
    let awid = 4.5;
    let bx = x2 - ux * alen;
    let by = y2 - uy * alen;
    out.push_str(&format!(
        "<polygon points=\"{x2:.1},{y2:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{color}\"/>\n",
        bx + nx * awid,
        by + ny * awid,
        bx - nx * awid,
        by - ny * awid
    ));
}

pub fn mermaid_to_svg(src: &str, dark_theme: bool) -> Result<String, String> {
    let pal = if dark_theme {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    };

    let lines: Vec<String> = src
        .lines()
        .map(|l| {
            let trimmed = l.trim();
            if let Some(pos) = trimmed.find("%%") {
                trimmed[..pos].trim().to_string()
            } else {
                trimmed.to_string()
            }
        })
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return Err("Empty Mermaid diagram".to_string());
    }

    let first = lines[0].to_ascii_lowercase();
    if first.starts_with("flowchart") || first.starts_with("graph") {
        render_flowchart(&lines, pal, false)
    } else if first.starts_with("sequencediagram") {
        render_sequence(&lines, pal)
    } else if first.starts_with("statediagram") {
        render_state_diagram(&lines, pal)
    } else if first.starts_with("pie") {
        render_pie(&lines, pal)
    } else if first.starts_with("classdiagram") || first.starts_with("erdiagram") {
        render_class_or_er(&lines, pal)
    } else {
        Err(format!("Unsupported or unrecognized Mermaid header: `{}`", lines[0]))
    }
}

// ---------------------------------------------------------------------------
// 1. Flowchart / State Graph
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowDir {
    Td,
    Bt,
    Lr,
    Rl,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeShape {
    Rect,
    Round,
    Diamond,
    Circle,
    Database,
    StartEndDot,
}

#[derive(Clone)]
struct FlowNode {
    label: String,
    shape: NodeShape,
    w: f32,
    h: f32,
    rank: usize,
    x: f32,
    y: f32,
}

#[derive(Clone)]
struct FlowEdge {
    from: String,
    to: String,
    label: Option<String>,
    dotted: bool,
    thick: bool,
    arrow_end: bool,
}

struct FlowSubgraph {
    title: String,
    members: Vec<String>,
}

fn parse_node_token(tok: &str) -> (String, Option<(String, NodeShape)>) {
    let t = tok.trim().trim_end_matches(';').trim();
    if t == "[*]" {
        return ("__state_boundary__".to_string(), None);
    }

    let bracket_start = t.find(['[', '{', '(']);
    let Some(bs) = bracket_start else {
        let id = t.split(":::").next().unwrap_or(t).trim();
        return (id.to_string(), None);
    };
    let id = t[..bs].trim().to_string();
    let rest = t[bs..].split(":::").next().unwrap_or(&t[bs..]).trim();

    let parsed = if let Some(inner) = rest.strip_prefix("[(").and_then(|s| s.strip_suffix(")]")) {
        (inner, NodeShape::Database)
    } else if let Some(inner) = rest.strip_prefix("([").and_then(|s| s.strip_suffix("])")) {
        (inner, NodeShape::Round)
    } else if let Some(inner) = rest.strip_prefix("((").and_then(|s| s.strip_suffix("))")) {
        (inner, NodeShape::Circle)
    } else if let Some(inner) = rest.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
        (inner, NodeShape::Diamond)
    } else if let Some(inner) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        (inner, NodeShape::Rect)
    } else if let Some(inner) = rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        (inner, NodeShape::Diamond)
    } else if let Some(inner) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        (inner, NodeShape::Round)
    } else {
        (rest, NodeShape::Rect)
    };
    let clean_lbl = parsed
        .0
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    (id, Some((clean_lbl, parsed.1)))
}

fn ensure_flow_node(
    nodes: &mut Vec<FlowNode>,
    idx_map: &mut HashMap<String, usize>,
    id: &str,
    info: Option<(String, NodeShape)>,
    current_sub: Option<&mut Vec<String>>,
) {
    if id.is_empty() {
        return;
    }
    if let Some(&existing) = idx_map.get(id) {
        if let Some((lbl, shape)) = info {
            nodes[existing].label = lbl;
            nodes[existing].shape = shape;
        }
    } else {
        let (lbl, shape) = info.unwrap_or_else(|| (id.to_string(), NodeShape::Rect));
        let lines = wrap_label(&lbl, 22);
        let max_line = lines.iter().map(|l| l.chars().count()).max().unwrap_or(4);
        let (w, h) = match shape {
            NodeShape::StartEndDot => (28.0, 28.0),
            NodeShape::Circle => {
                let d = ((max_line as f32) * 8.0 + 36.0).clamp(64.0, 140.0);
                (d, d)
            }
            NodeShape::Diamond => {
                let w = ((max_line as f32) * 8.5 + 64.0).clamp(110.0, 220.0);
                let h = ((lines.len() as f32) * 20.0 + 48.0).clamp(68.0, 120.0);
                (w, h)
            }
            _ => {
                let w = ((max_line as f32) * 8.2 + 34.0).clamp(96.0, 220.0);
                let h = ((lines.len() as f32) * 18.0 + 28.0).clamp(44.0, 112.0);
                (w, h)
            }
        };
        let ix = nodes.len();
        idx_map.insert(id.to_string(), ix);
        nodes.push(FlowNode {
            label: lbl,
            shape,
            w,
            h,
            rank: 0,
            x: 0.0,
            y: 0.0,
        });
    }
    if let Some(sub) = current_sub
        && !sub.iter().any(|m| m == id)
    {
        sub.push(id.to_string());
    }
}

struct ParsedConn {
    left_end: usize,
    right_start: usize,
    label: Option<String>,
    dotted: bool,
    thick: bool,
    arrow_end: bool,
}

fn find_next_connector(line: &str) -> Option<ParsedConn> {
    // Supported arrow operators outside quotes/brackets
    const OPS: &[(&str, bool, bool, bool)] = &[
        ("-.->", true, false, true),
        ("-.-", true, false, false),
        ("==>", false, true, true),
        ("===", false, true, false),
        ("-->", false, false, true),
        ("---", false, false, false),
        ("->", false, false, true),
    ];

    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut in_quotes = false;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            in_quotes = !in_quotes;
            i += 1;
            continue;
        }
        if !in_quotes {
            match b {
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                _ => {}
            }
            if bracket_depth == 0 && brace_depth == 0 && paren_depth == 0 {
                // Check inline `-- text -->` or `-. text .->` pattern
                if line[i..].starts_with("-- ")
                    && let Some(end_off) = line[i + 3..].find("-->")
                {
                    let lbl = line[i + 3..i + 3 + end_off].trim();
                    return Some(ParsedConn {
                        left_end: i,
                        right_start: i + 3 + end_off + 3,
                        label: if lbl.is_empty() {
                            None
                        } else {
                            Some(lbl.to_string())
                        },
                        dotted: false,
                        thick: false,
                        arrow_end: true,
                    });
                }

                for &(op, dotted, thick, arrow_end) in OPS {
                    if line[i..].starts_with(op) {
                        let after = i + op.len();
                        let trimmed_after = line[after..].trim_start();
                        let ws_len = line[after..].len() - trimmed_after.len();
                        if let Some(after_pipe) = trimmed_after.strip_prefix('|')
                            && let Some(close_pipe) = after_pipe.find('|')
                        {
                            let lbl = after_pipe[..close_pipe].trim();
                            return Some(ParsedConn {
                                left_end: i,
                                right_start: after + ws_len + 1 + close_pipe + 1,
                                label: if lbl.is_empty() {
                                    None
                                } else {
                                    Some(lbl.to_string())
                                },
                                dotted,
                                thick,
                                arrow_end,
                            });
                        }
                        return Some(ParsedConn {
                            left_end: i,
                            right_start: after,
                            label: None,
                            dotted,
                            thick,
                            arrow_end,
                        });
                    }
                }
            }
        }
        i += 1;
    }
    None
}

fn render_flowchart(
    lines: &[String],
    pal: &Palette,
    is_state_mode: bool,
) -> Result<String, String> {
    let dir_tok = lines[0]
        .split_whitespace()
        .nth(1)
        .unwrap_or("TD")
        .to_ascii_uppercase();
    let dir = match dir_tok.as_str() {
        "LR" => FlowDir::Lr,
        "RL" => FlowDir::Rl,
        "BT" => FlowDir::Bt,
        _ => FlowDir::Td,
    };

    let mut nodes: Vec<FlowNode> = Vec::new();
    let mut idx_map: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<FlowEdge> = Vec::new();
    let mut subgraphs: Vec<FlowSubgraph> = Vec::new();
    let mut active_sub_idx: Option<usize> = None;
    let mut state_start_cnt = 0usize;

    for line in &lines[1..] {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("direction ")
            || lower.starts_with("classdef ")
            || lower.starts_with("class ")
            || lower.starts_with("style ")
            || lower.starts_with("linkstyle ")
            || lower.starts_with("click ")
        {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("subgraph ")
            .or_else(|| line.strip_prefix("SUBGRAPH "))
        {
            let title = rest.trim().trim_matches('"').to_string();
            subgraphs.push(FlowSubgraph {
                title,
                members: Vec::new(),
            });
            active_sub_idx = Some(subgraphs.len() - 1);
            continue;
        }
        if lower == "end" {
            active_sub_idx = None;
            continue;
        }
        if is_state_mode
            && let Some(rest) = line.strip_prefix("state ")
            && let Some((lhs, rhs)) = rest.split_once(" as ")
        {
            let lbl = lhs.trim().trim_matches('"').to_string();
            let id = rhs.trim();
            let sub_vec = active_sub_idx.map(|ix| &mut subgraphs[ix].members);
            ensure_flow_node(
                &mut nodes,
                &mut idx_map,
                id,
                Some((lbl, NodeShape::Round)),
                sub_vec,
            );
            continue;
        }

        // Parse edge chains: A --> B -->|label| C
        let mut rest = line.as_str();
        let mut had_edge = false;
        let mut prev_id: Option<String> = None;

        while let Some(conn) = find_next_connector(rest) {
            had_edge = true;
            let left_str = rest[..conn.left_end].trim();
            let right_full = rest[conn.right_start..].trim();

            // Check right side split at next connector if chained
            let (right_str, trailing_conn) = if let Some(next_c) = find_next_connector(right_full) {
                (
                    right_full[..next_c.left_end].trim(),
                    &right_full[next_c.left_end..],
                )
            } else {
                (right_full, "")
            };

            // Handle `: label` suffix on stateDiagram lines (e.g. `A --> B : go`)
            let (right_node_part, edge_label) = if let Some((rn, lbl)) = right_str.split_once(" : ")
            {
                (rn.trim(), Some(lbl.trim().to_string()))
            } else {
                (right_str, conn.label.clone())
            };

            let from_id = if let Some(prev) = prev_id.take() {
                prev
            } else {
                let (mut fid, mut finfo) = parse_node_token(left_str);
                if fid == "__state_boundary__" {
                    state_start_cnt += 1;
                    fid = format!("__start_{state_start_cnt}");
                    finfo = Some((String::new(), NodeShape::StartEndDot));
                }
                let sub_vec = active_sub_idx.map(|ix| &mut subgraphs[ix].members);
                ensure_flow_node(&mut nodes, &mut idx_map, &fid, finfo, sub_vec);
                fid
            };

            let (mut tid, mut tinfo) = parse_node_token(right_node_part);
            if tid == "__state_boundary__" {
                state_start_cnt += 1;
                tid = format!("__end_{state_start_cnt}");
                tinfo = Some((String::new(), NodeShape::StartEndDot));
            } else if is_state_mode && tinfo.is_none() {
                tinfo = Some((tid.clone(), NodeShape::Round));
            }
            let sub_vec = active_sub_idx.map(|ix| &mut subgraphs[ix].members);
            ensure_flow_node(&mut nodes, &mut idx_map, &tid, tinfo, sub_vec);

            edges.push(FlowEdge {
                from: from_id,
                to: tid.clone(),
                label: edge_label,
                dotted: conn.dotted,
                thick: conn.thick,
                arrow_end: conn.arrow_end,
            });

            if trailing_conn.is_empty() {
                break;
            }
            prev_id = Some(tid);
            rest = right_full;
        }

        if !had_edge {
            let (id, mut info) = parse_node_token(line);
            if is_state_mode && info.is_none() && id != "__state_boundary__" {
                info = Some((id.clone(), NodeShape::Round));
            }
            if id != "__state_boundary__" {
                let sub_vec = active_sub_idx.map(|ix| &mut subgraphs[ix].members);
                ensure_flow_node(&mut nodes, &mut idx_map, &id, info, sub_vec);
            }
        }
    }

    if nodes.is_empty() {
        return Err("No nodes found in Mermaid diagram".to_string());
    }

    // Layered rank assignment (cycle-safe Bellman relaxation bounded by node count)
    let max_iters = nodes.len().min(64);
    for _ in 0..max_iters {
        let mut changed = false;
        for edge in &edges {
            if let (Some(&fi), Some(&ti)) = (idx_map.get(&edge.from), idx_map.get(&edge.to)) {
                let target_rank = (nodes[fi].rank + 1).min(nodes.len());
                if target_rank > nodes[ti].rank {
                    nodes[ti].rank = target_rank;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let max_rank = nodes.iter().map(|n| n.rank).max().unwrap_or(0);
    let mut by_rank: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (ix, n) in nodes.iter().enumerate() {
        by_rank[n.rank].push(ix);
    }

    // Place coordinates cleanly per direction
    let rank_gap = 78.0f32;
    let sibling_gap = 36.0f32;
    let margin = 48.0f32;
    let horizontal = matches!(dir, FlowDir::Lr | FlowDir::Rl);

    let mut layer_breadths = Vec::with_capacity(by_rank.len());
    let mut layer_depths = Vec::with_capacity(by_rank.len());
    for layer in &by_rank {
        let mut breadth = 0.0f32;
        let mut depth = 40.0f32;
        for (pos, &ni) in layer.iter().enumerate() {
            if pos > 0 {
                breadth += sibling_gap;
            }
            if horizontal {
                breadth += nodes[ni].h;
                depth = depth.max(nodes[ni].w);
            } else {
                breadth += nodes[ni].w;
                depth = depth.max(nodes[ni].h);
            }
        }
        layer_breadths.push(breadth);
        layer_depths.push(depth);
    }
    let max_breadth = layer_breadths.iter().copied().fold(200.0f32, f32::max);

    let mut depth_cursor = margin;
    let total_depth: f32 = layer_depths.iter().sum::<f32>()
        + (by_rank.len().saturating_sub(1) as f32) * rank_gap
        + margin * 2.0;

    for (r_ix, layer) in by_rank.iter().enumerate() {
        let d = layer_depths[r_ix];
        let breadth = layer_breadths[r_ix];
        let mut b_cursor = margin + (max_breadth - breadth) * 0.5;
        let center_depth = if matches!(dir, FlowDir::Bt | FlowDir::Rl) {
            total_depth - depth_cursor - d * 0.5
        } else {
            depth_cursor + d * 0.5
        };
        for &ni in layer {
            if horizontal {
                nodes[ni].x = center_depth - nodes[ni].w * 0.5;
                nodes[ni].y = b_cursor;
                b_cursor += nodes[ni].h + sibling_gap;
            } else {
                nodes[ni].x = b_cursor;
                nodes[ni].y = center_depth - nodes[ni].h * 0.5;
                b_cursor += nodes[ni].w + sibling_gap;
            }
        }
        depth_cursor += d + rank_gap;
    }

    let (svg_w, svg_h) = if horizontal {
        (total_depth.max(320.0), (max_breadth + margin * 2.0).max(200.0))
    } else {
        ((max_breadth + margin * 2.0).max(320.0), total_depth.max(200.0))
    };

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" viewBox=\"0 0 {svg_w:.0} {svg_h:.0}\">\n\
         <rect x=\"0\" y=\"0\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" fill=\"{}\"/>\n",
        pal.bg
    );

    // Subgraph background frames
    for sg in &subgraphs {
        let mut coords: Option<(f32, f32, f32, f32)> = None;
        for mid in &sg.members {
            if let Some(&ni) = idx_map.get(mid) {
                let n = &nodes[ni];
                coords = Some(match coords {
                    Some((x0, y0, x1, y1)) => (
                        x0.min(n.x),
                        y0.min(n.y),
                        x1.max(n.x + n.w),
                        y1.max(n.y + n.h),
                    ),
                    None => (n.x, n.y, n.x + n.w, n.y + n.h),
                });
            }
        }
        if let Some((x0, y0, x1, y1)) = coords {
            let fx = x0 - 18.0;
            let fy = y0 - 30.0;
            let fw = (x1 - x0) + 36.0;
            let fh = (y1 - y0) + 46.0;
            svg.push_str(&format!(
                "<rect x=\"{fx:.1}\" y=\"{fy:.1}\" width=\"{fw:.1}\" height=\"{fh:.1}\" rx=\"10\" fill=\"{}\" fill-opacity=\"0.45\" stroke=\"{}\" stroke-width=\"1.4\" stroke-dasharray=\"5,4\"/>\n",
                pal.surface_alt, pal.muted
            ));
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"12\" font-weight=\"bold\" fill=\"{}\">{}</text>\n",
                fx + 10.0,
                fy + 18.0,
                pal.accent,
                escape_xml(&sg.title)
            ));
        }
    }

    // Edges
    for edge in &edges {
        let Some(&fi) = idx_map.get(&edge.from) else {
            continue;
        };
        let Some(&ti) = idx_map.get(&edge.to) else {
            continue;
        };
        let fn_node = &nodes[fi];
        let tn_node = &nodes[ti];
        let (x1, y1, cx1, cy1, cx2, cy2, x2, y2) = match dir {
            FlowDir::Td => {
                let sx = fn_node.x + fn_node.w * 0.5;
                let sy = fn_node.y + fn_node.h;
                let tx = tn_node.x + tn_node.w * 0.5;
                let ty = tn_node.y;
                let mid = (ty - sy).abs().max(40.0) * 0.45;
                (sx, sy, sx, sy + mid, tx, ty - mid, tx, ty)
            }
            FlowDir::Bt => {
                let sx = fn_node.x + fn_node.w * 0.5;
                let sy = fn_node.y;
                let tx = tn_node.x + tn_node.w * 0.5;
                let ty = tn_node.y + tn_node.h;
                let mid = (sy - ty).abs().max(40.0) * 0.45;
                (sx, sy, sx, sy - mid, tx, ty + mid, tx, ty)
            }
            FlowDir::Lr => {
                let sx = fn_node.x + fn_node.w;
                let sy = fn_node.y + fn_node.h * 0.5;
                let tx = tn_node.x;
                let ty = tn_node.y + tn_node.h * 0.5;
                let mid = (tx - sx).abs().max(40.0) * 0.45;
                (sx, sy, sx + mid, sy, tx - mid, ty, tx, ty)
            }
            FlowDir::Rl => {
                let sx = fn_node.x;
                let sy = fn_node.y + fn_node.h * 0.5;
                let tx = tn_node.x + tn_node.w;
                let ty = tn_node.y + tn_node.h * 0.5;
                let mid = (sx - tx).abs().max(40.0) * 0.45;
                (sx, sy, sx - mid, sy, tx + mid, ty, tx, ty)
            }
        };

        let sw = if edge.thick { "3.0" } else { "1.8" };
        let dash = if edge.dotted {
            " stroke-dasharray=\"6,4\""
        } else {
            ""
        };
        svg.push_str(&format!(
            "<path d=\"M {x1:.1} {y1:.1} C {cx1:.1} {cy1:.1}, {cx2:.1} {cy2:.1}, {x2:.1} {y2:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{sw}\"{dash}/>\n",
            pal.muted
        ));
        if edge.arrow_end {
            emit_arrowhead(&mut svg, cx2, cy2, x2, y2, pal.muted);
        }
        if let Some(ref lbl) = edge.label {
            let mx = 0.125 * x1 + 0.375 * cx1 + 0.375 * cx2 + 0.125 * x2;
            let my = 0.125 * y1 + 0.375 * cy1 + 0.375 * cy2 + 0.125 * y2;
            let pw = (lbl.chars().count() as f32 * 7.0 + 14.0).max(34.0);
            svg.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{pw:.1}\" height=\"20\" rx=\"4\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                mx - pw * 0.5,
                my - 10.0,
                pal.bg,
                pal.muted
            ));
            svg.push_str(&format!(
                "<text x=\"{mx:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"11\" fill=\"{}\">{}</text>\n",
                my + 4.0,
                pal.text,
                escape_xml(lbl)
            ));
        }
    }

    // Nodes
    for (ix, n) in nodes.iter().enumerate() {
        let stroke = pal.slice_colors[ix % pal.slice_colors.len()];
        let cx = n.x + n.w * 0.5;
        let cy = n.y + n.h * 0.5;
        match n.shape {
            NodeShape::StartEndDot => {
                svg.push_str(&format!(
                    "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
                    pal.accent, pal.text
                ));
            }
            NodeShape::Round => {
                let rx = (n.h * 0.45).min(20.0);
                svg.push_str(&format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{rx:.1}\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    n.x, n.y, n.w, n.h, pal.surface
                ));
                emit_centered_text(&mut svg, cx, cy, &n.label, 12.5, pal.text, false);
            }
            NodeShape::Diamond => {
                let top = (cx, n.y);
                let right = (n.x + n.w, cy);
                let bot = (cx, n.y + n.h);
                let left = (n.x, cy);
                svg.push_str(&format!(
                    "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    top.0, top.1, right.0, right.1, bot.0, bot.1, left.0, left.1, pal.surface
                ));
                emit_centered_text(&mut svg, cx, cy, &n.label, 12.0, pal.text, false);
            }
            NodeShape::Circle => {
                let r = (n.w.min(n.h)) * 0.5;
                svg.push_str(&format!(
                    "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r:.1}\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    pal.surface
                ));
                emit_centered_text(&mut svg, cx, cy, &n.label, 12.0, pal.text, false);
            }
            NodeShape::Database => {
                let rx = n.w * 0.5;
                let ry = 8.0f32;
                let bx = n.x;
                let by = n.y + ry;
                let bh = (n.h - ry * 2.0).max(18.0);
                svg.push_str(&format!(
                    "<rect x=\"{bx:.1}\" y=\"{by:.1}\" width=\"{:.1}\" height=\"{bh:.1}\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    n.w, pal.surface
                ));
                svg.push_str(&format!(
                    "<ellipse cx=\"{cx:.1}\" cy=\"{:.1}\" rx=\"{rx:.1}\" ry=\"{ry:.1}\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    n.y + n.h - ry, pal.surface
                ));
                svg.push_str(&format!(
                    "<ellipse cx=\"{cx:.1}\" cy=\"{by:.1}\" rx=\"{rx:.1}\" ry=\"{ry:.1}\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    pal.surface_alt
                ));
                emit_centered_text(&mut svg, cx, cy + 2.0, &n.label, 12.0, pal.text, false);
            }
            NodeShape::Rect => {
                svg.push_str(&format!(
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"6\" fill=\"{}\" stroke=\"{stroke}\" stroke-width=\"1.8\"/>\n",
                    n.x, n.y, n.w, n.h, pal.surface
                ));
                emit_centered_text(&mut svg, cx, cy, &n.label, 12.5, pal.text, false);
            }
        }
    }

    svg.push_str("</svg>\n");
    Ok(svg)
}

fn render_state_diagram(lines: &[String], pal: &Palette) -> Result<String, String> {
    let mut synth = vec!["flowchart TD".to_string()];
    synth.extend(lines[1..].iter().cloned());
    render_flowchart(&synth, pal, true)
}

// ---------------------------------------------------------------------------
// 2. Sequence Diagram
// ---------------------------------------------------------------------------

struct SeqMsg {
    from: usize,
    to: usize,
    text: String,
    dashed: bool,
}

struct SeqNote {
    span_start: usize,
    span_end: usize,
    text: String,
    after_msg_count: usize,
}

fn ensure_participant(
    list: &mut Vec<(String, String)>,
    map: &mut HashMap<String, usize>,
    id: &str,
    label: Option<&str>,
) -> usize {
    if let Some(&ix) = map.get(id) {
        if let Some(l) = label {
            list[ix].1 = l.to_string();
        }
        return ix;
    }
    let ix = list.len();
    map.insert(id.to_string(), ix);
    list.push((id.to_string(), label.unwrap_or(id).to_string()));
    ix
}

fn render_sequence(lines: &[String], pal: &Palette) -> Result<String, String> {
    let mut participants: Vec<(String, String)> = Vec::new();
    let mut p_map: HashMap<String, usize> = HashMap::new();
    let mut messages: Vec<SeqMsg> = Vec::new();
    let mut notes: Vec<SeqNote> = Vec::new();

    for line in &lines[1..] {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = trimmed
            .strip_prefix("participant ")
            .or_else(|| trimmed.strip_prefix("actor "))
        {
            if let Some((id, lbl)) = rest.split_once(" as ") {
                ensure_participant(&mut participants, &mut p_map, id.trim(), Some(lbl.trim()));
            } else {
                ensure_participant(&mut participants, &mut p_map, rest.trim(), None);
            }
            continue;
        }
        if lower.starts_with("note ")
            && let Some((head, msg)) = trimmed.split_once(':')
        {
            let h = head.trim();
            let target = h
                .split(" of ")
                .nth(1)
                .or_else(|| h.split(" over ").nth(1))
                .unwrap_or("")
                .trim();
            let mut ids: Vec<usize> = target
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| ensure_participant(&mut participants, &mut p_map, s.trim(), None))
                .collect();
            if ids.is_empty() {
                continue;
            }
            ids.sort_unstable();
            notes.push(SeqNote {
                span_start: *ids.first().unwrap_or(&0),
                span_end: *ids.last().unwrap_or(&0),
                text: msg.trim().to_string(),
                after_msg_count: messages.len(),
            });
            continue;
        }

        const SEQ_ARROWS: &[(&str, bool)] =
            &[("-->>", true), ("->>", false), ("-->", true), ("->", false)];
        for &(arrow, dashed) in SEQ_ARROWS {
            if let Some((lhs, rhs_all)) = trimmed.split_once(arrow) {
                let (to_id, msg_text) = rhs_all.split_once(':').unwrap_or((rhs_all, ""));
                let fi = ensure_participant(&mut participants, &mut p_map, lhs.trim(), None);
                let ti = ensure_participant(
                    &mut participants,
                    &mut p_map,
                    to_id.trim().trim_start_matches('+').trim_start_matches('-'),
                    None,
                );
                messages.push(SeqMsg {
                    from: fi,
                    to: ti,
                    text: msg_text.trim().to_string(),
                    dashed,
                });
                break;
            }
        }
    }

    if participants.is_empty() {
        return Err("No participants found in sequenceDiagram".to_string());
    }

    let col_w = 170.0f32;
    let margin_x = 64.0f32;
    let box_w = 128.0f32;
    let box_h = 38.0f32;
    let step_h = 54.0f32;
    let header_y = 28.0f32;

    let mut total_rows = messages.len() + notes.len();
    if total_rows == 0 {
        total_rows = 1;
    }
    let body_h = (total_rows as f32) * step_h + 36.0;
    let svg_w = ((participants.len() as f32) * col_w + margin_x * 2.0).max(360.0);
    let bottom_y = header_y + box_h + body_h;
    let svg_h = bottom_y + box_h + 28.0;

    let px_center = |ix: usize| -> f32 { margin_x + col_w * 0.5 + (ix as f32) * col_w };

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" viewBox=\"0 0 {svg_w:.0} {svg_h:.0}\">\n\
         <rect x=\"0\" y=\"0\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" fill=\"{}\"/>\n",
        pal.bg
    );

    // Lifelines & top/bottom actor boxes
    for (ix, (_, label)) in participants.iter().enumerate() {
        let cx = px_center(ix);
        let col = pal.slice_colors[ix % pal.slice_colors.len()];
        svg.push_str(&format!(
            "<line x1=\"{cx:.1}\" y1=\"{:.1}\" x2=\"{cx:.1}\" y2=\"{bottom_y:.1}\" stroke=\"{}\" stroke-width=\"1.4\" stroke-dasharray=\"5,5\"/>\n",
            header_y + box_h, pal.muted
        ));
        for by in [header_y, bottom_y] {
            svg.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{by:.1}\" width=\"{box_w:.1}\" height=\"{box_h:.1}\" rx=\"6\" fill=\"{}\" stroke=\"{col}\" stroke-width=\"1.8\"/>\n",
                cx - box_w * 0.5, pal.surface
            ));
            emit_centered_text(&mut svg, cx, by + box_h * 0.5, label, 12.5, pal.text, true);
        }
    }

    // Interleave messages & notes chronologically
    let mut cur_y = header_y + box_h + 44.0;
    let mut note_ix = 0usize;
    for (m_ix, msg) in messages.iter().enumerate() {
        while note_ix < notes.len() && notes[note_ix].after_msg_count <= m_ix {
            let n = &notes[note_ix];
            let x0 = px_center(n.span_start) - 60.0;
            let x1 = px_center(n.span_end) + 60.0;
            let nw = (x1 - x0).max(120.0);
            svg.push_str(&format!(
                "<rect x=\"{x0:.1}\" y=\"{:.1}\" width=\"{nw:.1}\" height=\"32\" rx=\"5\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.2\"/>\n",
                cur_y - 18.0, pal.surface_alt, pal.accent
            ));
            emit_centered_text(&mut svg, x0 + nw * 0.5, cur_y - 2.0, &n.text, 11.5, pal.text, false);
            cur_y += step_h;
            note_ix += 1;
        }

        let fx = px_center(msg.from);
        let tx = px_center(msg.to);
        if msg.from == msg.to {
            let lx = fx + 44.0;
            svg.push_str(&format!(
                "<path d=\"M {fx:.1} {:.1} L {lx:.1} {:.1} L {lx:.1} {:.1} L {fx:.1} {:.1}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.8\"/>\n",
                cur_y - 8.0, cur_y - 8.0, cur_y + 14.0, cur_y + 14.0, pal.border
            ));
            emit_arrowhead(&mut svg, lx, cur_y + 14.0, fx, cur_y + 14.0, pal.border);
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"12\" fill=\"{}\">{}</text>\n",
                lx + 8.0, cur_y + 6.0, pal.text, escape_xml(&msg.text)
            ));
        } else {
            let dash = if msg.dashed {
                " stroke-dasharray=\"6,4\""
            } else {
                ""
            };
            svg.push_str(&format!(
                "<line x1=\"{fx:.1}\" y1=\"{cur_y:.1}\" x2=\"{tx:.1}\" y2=\"{cur_y:.1}\" stroke=\"{}\" stroke-width=\"1.8\"{dash}/>\n",
                pal.border
            ));
            emit_arrowhead(&mut svg, fx, cur_y, tx, cur_y, pal.border);
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"12\" fill=\"{}\">{}</text>\n",
                (fx + tx) * 0.5,
                cur_y - 8.0,
                pal.text,
                escape_xml(&msg.text)
            ));
        }
        cur_y += step_h;
    }

    while note_ix < notes.len() {
        let n = &notes[note_ix];
        let x0 = px_center(n.span_start) - 60.0;
        let x1 = px_center(n.span_end) + 60.0;
        let nw = (x1 - x0).max(120.0);
        svg.push_str(&format!(
            "<rect x=\"{x0:.1}\" y=\"{:.1}\" width=\"{nw:.1}\" height=\"32\" rx=\"5\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.2\"/>\n",
            cur_y - 18.0, pal.surface_alt, pal.accent
        ));
        emit_centered_text(&mut svg, x0 + nw * 0.5, cur_y - 2.0, &n.text, 11.5, pal.text, false);
        cur_y += step_h;
        note_ix += 1;
    }

    svg.push_str("</svg>\n");
    Ok(svg)
}

// ---------------------------------------------------------------------------
// 3. Pie Chart
// ---------------------------------------------------------------------------

fn render_pie(lines: &[String], pal: &Palette) -> Result<String, String> {
    let mut title = lines[0]
        .split_once("title ")
        .map(|(_, t)| t.trim().to_string())
        .unwrap_or_default();
    let mut slices: Vec<(String, f64)> = Vec::new();

    for line in &lines[1..] {
        if let Some(t) = line.strip_prefix("title ") {
            title = t.trim().to_string();
        } else if let Some((lbl, val_str)) = line.split_once(':') {
            let clean_lbl = lbl.trim().trim_matches('"').trim_matches('\'').to_string();
            if let Ok(val) = val_str.trim().parse::<f64>()
                && val > 0.0
            {
                slices.push((clean_lbl, val));
            }
        }
    }

    if slices.is_empty() {
        return Err("No valid slices in pie diagram".to_string());
    }
    let total: f64 = slices.iter().map(|(_, v)| *v).sum();
    let svg_w = 560.0f32;
    let svg_h = ((slices.len() as f32) * 28.0 + 110.0).max(320.0);
    let cx = 155.0f32;
    let cy = svg_h * 0.5 + if title.is_empty() { 0.0 } else { 14.0 };
    let r = 110.0f32;

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" viewBox=\"0 0 {svg_w:.0} {svg_h:.0}\">\n\
         <rect x=\"0\" y=\"0\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" fill=\"{}\"/>\n",
        pal.bg
    );
    if !title.is_empty() {
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"32\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"16\" font-weight=\"bold\" fill=\"{}\">{}</text>\n",
            svg_w * 0.5, pal.text, escape_xml(&title)
        ));
    }

    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (ix, (lbl, val)) in slices.iter().enumerate() {
        let frac = *val / total;
        let span = frac * std::f64::consts::TAU;
        let col = pal.slice_colors[ix % pal.slice_colors.len()];

        if (frac - 1.0).abs() < 1e-6 {
            svg.push_str(&format!(
                "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"{r:.1}\" fill=\"{col}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
                pal.bg
            ));
        } else {
            let x0 = cx + r * (angle.cos() as f32);
            let y0 = cy + r * (angle.sin() as f32);
            let end_a = angle + span;
            let x1 = cx + r * (end_a.cos() as f32);
            let y1 = cy + r * (end_a.sin() as f32);
            let large_arc = if span > std::f64::consts::PI { 1 } else { 0 };
            svg.push_str(&format!(
                "<path d=\"M {cx:.1} {cy:.1} L {x0:.1} {y0:.1} A {r:.1} {r:.1} 0 {large_arc} 1 {x1:.1} {y1:.1} Z\" fill=\"{col}\" stroke=\"{}\" stroke-width=\"2\"/>\n",
                pal.bg
            ));
        }

        // Legend item
        let lx = 310.0f32;
        let ly = 72.0 + (ix as f32) * 28.0;
        svg.push_str(&format!(
            "<rect x=\"{lx:.1}\" y=\"{:.1}\" width=\"14\" height=\"14\" rx=\"3\" fill=\"{col}\"/>\n",
            ly - 11.0
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{ly:.1}\" font-family=\"sans-serif\" font-size=\"12.5\" fill=\"{}\">{} ({:.1}%)</text>\n",
            lx + 22.0, pal.text, escape_xml(lbl), frac * 100.0
        ));

        angle += span;
    }

    svg.push_str("</svg>\n");
    Ok(svg)
}

// ---------------------------------------------------------------------------
// 4. Class / ER Diagram
// ---------------------------------------------------------------------------

struct BoxEntity {
    name: String,
    members: Vec<String>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn render_class_or_er(lines: &[String], pal: &Palette) -> Result<String, String> {
    let mut boxes: Vec<BoxEntity> = Vec::new();
    let mut box_map: HashMap<String, usize> = HashMap::new();
    let mut relations: Vec<(usize, usize, String)> = Vec::new();

    let ensure_box = |name: &str, boxes: &mut Vec<BoxEntity>, map: &mut HashMap<String, usize>| -> usize {
        let clean = name.trim();
        if let Some(&ix) = map.get(clean) {
            return ix;
        }
        let ix = boxes.len();
        map.insert(clean.to_string(), ix);
        boxes.push(BoxEntity {
            name: clean.to_string(),
            members: Vec::new(),
            x: 0.0,
            y: 0.0,
            w: 160.0,
            h: 48.0,
        });
        ix
    };

    let mut active_box: Option<usize> = None;
    for line in &lines[1..] {
        let t = line.trim();
        if let Some(idx) = active_box {
            if t == "}" {
                active_box = None;
            } else if !t.is_empty() {
                boxes[idx].members.push(t.to_string());
            }
            continue;
        }

        if t.ends_with('{') {
            let header = t.trim_end_matches('{').trim();
            let name = header.strip_prefix("class ").unwrap_or(header).trim();
            let ix = ensure_box(name, &mut boxes, &mut box_map);
            active_box = Some(ix);
            continue;
        }

        if let Some(rest) = t.strip_prefix("class ") {
            ensure_box(rest, &mut boxes, &mut box_map);
            continue;
        }

        // Look for relationship symbols
        const REL_OPS: &[&str] = &[
            "<|--", "--|>", "*--", "--*", "o--", "--o", "..|>", "<|..", "..>", "<..", "-->",
            "<--", "||--o{", "}o--||", "||--|{", "}|--||", "||--||", "}o--o{", "--", "..",
        ];
        let mut matched_rel = false;
        for &op in REL_OPS {
            if let Some((lhs, rhs_all)) = t.split_once(op) {
                let (rhs, lbl) = rhs_all.split_once(':').unwrap_or((rhs_all, ""));
                let a = ensure_box(lhs.trim(), &mut boxes, &mut box_map);
                let b = ensure_box(rhs.trim(), &mut boxes, &mut box_map);
                relations.push((a, b, lbl.trim().to_string()));
                matched_rel = true;
                break;
            }
        }
        if !matched_rel && t.contains(':') {
            if let Some((cname, member)) = t.split_once(':') {
                let ix = ensure_box(cname.trim(), &mut boxes, &mut box_map);
                boxes[ix].members.push(member.trim().to_string());
            }
        }
    }

    if boxes.is_empty() {
        return Err("No classes or entities found in Mermaid diagram".to_string());
    }

    // Compute box sizes & lay out in a clean 3-column grid
    let cols = boxes.len().clamp(1, 3);
    let mut col_widths = vec![160.0f32; cols];
    let mut row_heights: Vec<f32> = Vec::new();
    for (ix, b) in boxes.iter_mut().enumerate() {
        let longest = std::iter::once(&b.name)
            .chain(b.members.iter())
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(6);
        b.w = ((longest as f32) * 7.6 + 28.0).clamp(150.0, 290.0);
        b.h = 34.0 + (b.members.len() as f32) * 19.0 + if b.members.is_empty() { 4.0 } else { 12.0 };
        let c = ix % cols;
        col_widths[c] = col_widths[c].max(b.w);
        let r = ix / cols;
        if r >= row_heights.len() {
            row_heights.push(b.h);
        } else {
            row_heights[r] = row_heights[r].max(b.h);
        }
    }

    let gap_x = 56.0f32;
    let gap_y = 56.0f32;
    let margin = 44.0f32;
    let mut col_x = vec![margin; cols];
    for c in 1..cols {
        col_x[c] = col_x[c - 1] + col_widths[c - 1] + gap_x;
    }
    let mut row_y = vec![margin; row_heights.len()];
    for r in 1..row_heights.len() {
        row_y[r] = row_y[r - 1] + row_heights[r - 1] + gap_y;
    }

    for (ix, b) in boxes.iter_mut().enumerate() {
        b.x = col_x[ix % cols] + (col_widths[ix % cols] - b.w) * 0.5;
        b.y = row_y[ix / cols];
    }

    let svg_w = col_x.last().unwrap_or(&margin) + col_widths.last().unwrap_or(&160.0) + margin;
    let svg_h = row_y.last().unwrap_or(&margin) + row_heights.last().unwrap_or(&60.0) + margin;

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" viewBox=\"0 0 {svg_w:.0} {svg_h:.0}\">\n\
         <rect x=\"0\" y=\"0\" width=\"{svg_w:.0}\" height=\"{svg_h:.0}\" fill=\"{}\"/>\n",
        pal.bg
    );

    // Relationships
    for (a, b, lbl) in &relations {
        let ba = &boxes[*a];
        let bb = &boxes[*b];
        let x1 = ba.x + ba.w * 0.5;
        let y1 = ba.y + ba.h * 0.5;
        let x2 = bb.x + bb.w * 0.5;
        let y2 = bb.y + bb.h * 0.5;
        svg.push_str(&format!(
            "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" stroke=\"{}\" stroke-width=\"1.6\"/>\n",
            pal.muted
        ));
        if !lbl.is_empty() {
            svg.push_str(&format!(
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"11\" fill=\"{}\">{}</text>\n",
                (x1 + x2) * 0.5,
                (y1 + y2) * 0.5 - 6.0,
                pal.accent,
                escape_xml(lbl)
            ));
        }
    }

    // Boxes on top of connector lines
    for (ix, b) in boxes.iter().enumerate() {
        let col = pal.slice_colors[ix % pal.slice_colors.len()];
        svg.push_str(&format!(
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"6\" fill=\"{}\" stroke=\"{col}\" stroke-width=\"1.8\"/>\n",
            b.x, b.y, b.w, b.h, pal.surface
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"13\" font-weight=\"bold\" fill=\"{}\">{}</text>\n",
            b.x + b.w * 0.5,
            b.y + 22.0,
            pal.text,
            escape_xml(&b.name)
        ));
        if !b.members.is_empty() {
            let sep_y = b.y + 32.0;
            svg.push_str(&format!(
                "<line x1=\"{:.1}\" y1=\"{sep_y:.1}\" x2=\"{:.1}\" y2=\"{sep_y:.1}\" stroke=\"{col}\" stroke-width=\"1.0\" stroke-opacity=\"0.6\"/>\n",
                b.x, b.x + b.w
            ));
            for (mi, mem) in b.members.iter().enumerate() {
                svg.push_str(&format!(
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"monospace\" font-size=\"11.5\" fill=\"{}\">{}</text>\n",
                    b.x + 12.0,
                    sep_y + 16.0 + (mi as f32) * 19.0,
                    pal.muted,
                    escape_xml(mem)
                ));
            }
        }
    }

    svg.push_str("</svg>\n");
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::svg_engine::render_svg_to_image;

    #[test]
    fn renders_all_supported_mermaid_diagrams_through_resvg() {
        let flow = "\
flowchart TD
    subgraph Core
        A[Start & Init] --> B{Ready?}
    end
    B -->|Yes| C([Run Loop])
    B -.->|No| D[(Database)]
";
        let svg = mermaid_to_svg(flow, true).unwrap();
        assert!(svg.contains("Start &amp; Init"));
        assert!(render_svg_to_image(&svg, 1600).is_ok());

        let seq = "\
sequenceDiagram
    participant U as User
    participant App as VKarma
    U->>App: Open .mmd
    Note over U,App: Live vector preview
    App-->>U: Rendered SVG
";
        let seq_svg = mermaid_to_svg(seq, false).unwrap();
        assert!(render_svg_to_image(&seq_svg, 1600).is_ok());

        let state = "stateDiagram-v2\n[*] --> Idle\nIdle --> Editing : keydown\nEditing --> [*]\n";
        assert!(render_svg_to_image(&mermaid_to_svg(state, true).unwrap(), 1600).is_ok());

        let pie = "pie title Storage\n\"Notes\" : 45\n\"PDFs\" : 35\n\"Audio\" : 20\n";
        assert!(render_svg_to_image(&mermaid_to_svg(pie, true).unwrap(), 1600).is_ok());

        let class_d = "classDiagram\nclass DocumentTab {\n+usize id\n+save()\n}\nDocumentTab --> PaneSide : lives in\n";
        assert!(render_svg_to_image(&mermaid_to_svg(class_d, true).unwrap(), 1600).is_ok());
    }
}
