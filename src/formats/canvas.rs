use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CanvasDoc {
    #[serde(default)]
    pub nodes: Vec<CanvasNode>,
    #[serde(default)]
    pub edges: Vec<CanvasEdge>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasNode {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasEdge {
    pub id: String,
    #[serde(rename = "fromNode")]
    pub from_node: String,
    #[serde(rename = "fromSide", default, skip_serializing_if = "Option::is_none")]
    pub from_side: Option<String>,
    #[serde(rename = "fromEnd", default, skip_serializing_if = "Option::is_none")]
    pub from_end: Option<String>,
    #[serde(rename = "toNode")]
    pub to_node: String,
    #[serde(rename = "toSide", default, skip_serializing_if = "Option::is_none")]
    pub to_side: Option<String>,
    #[serde(rename = "toEnd", default, skip_serializing_if = "Option::is_none")]
    pub to_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl CanvasDoc {
    pub fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self::default());
        }
        let mut doc: Self = serde_json::from_str(trimmed).context("Invalid JSON Canvas")?;
        for node in &mut doc.nodes {
            node.width = node.width.max(80.0);
            node.height = node.height.max(40.0);
        }
        Ok(doc)
    }

    pub fn to_json_pretty(&self) -> String {
        let mut out = serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{\n  \"nodes\": [],\n  \"edges\": []\n}".to_string());
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    pub fn next_id(&self) -> String {
        let used: HashSet<&str> = self
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .chain(self.edges.iter().map(|e| e.id.as_str()))
            .collect();
        let mut seq = (self.nodes.len() + self.edges.len() + 1) as u64;
        loop {
            let cand = format!("{seq:016x}");
            if !used.contains(cand.as_str()) {
                return cand;
            }
            seq += 1;
        }
    }

    pub fn add_text_node(&mut self, x: f64, y: f64, text: String) -> String {
        let id = self.next_id();
        self.nodes.push(CanvasNode {
            id: id.clone(),
            kind: "text".to_string(),
            x,
            y,
            width: 280.0,
            height: 140.0,
            color: None,
            text: Some(text),
            file: None,
            subpath: None,
            url: None,
            label: None,
            extra: serde_json::Map::new(),
        });
        id
    }

    pub fn add_group_node(&mut self, x: f64, y: f64, label: String) -> String {
        let id = self.next_id();
        self.nodes.push(CanvasNode {
            id: id.clone(),
            kind: "group".to_string(),
            x,
            y,
            width: 440.0,
            height: 300.0,
            color: Some("5".to_string()),
            text: None,
            file: None,
            subpath: None,
            url: None,
            label: Some(label),
            extra: serde_json::Map::new(),
        });
        id
    }

    pub fn add_edge(&mut self, from_node: String, to_node: String) -> String {
        let id = self.next_id();
        self.edges.push(CanvasEdge {
            id: id.clone(),
            from_node,
            from_side: None,
            from_end: None,
            to_node,
            to_side: None,
            to_end: Some("arrow".to_string()),
            color: None,
            label: None,
            extra: serde_json::Map::new(),
        });
        id
    }

    pub fn remove_node(&mut self, id: &str) {
        self.nodes.retain(|n| n.id != id);
        self.edges
            .retain(|e| e.from_node != id && e.to_node != id);
    }

    pub fn remove_edge(&mut self, id: &str) {
        self.edges.retain(|e| e.id != id);
    }

    pub fn bounding_box(&self) -> Option<(f64, f64, f64, f64)> {
        let first = self.nodes.first()?;
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut max_x = first.x + first.width;
        let mut max_y = first.y + first.height;
        for n in &self.nodes[1..] {
            min_x = min_x.min(n.x);
            min_y = min_y.min(n.y);
            max_x = max_x.max(n.x + n.width);
            max_y = max_y.max(n.y + n.height);
        }
        Some((min_x, min_y, max_x, max_y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn from_opt(s: Option<&str>) -> Option<Self> {
        match s?.trim().to_ascii_lowercase().as_str() {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    fn normal(self) -> (f32, f32) {
        match self {
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::Top => (0.0, -1.0),
            Self::Bottom => (0.0, 1.0),
        }
    }
}

fn side_anchor(node: &CanvasNode, side: Side) -> (f32, f32) {
    let x = node.x as f32;
    let y = node.y as f32;
    let w = node.width as f32;
    let h = node.height as f32;
    match side {
        Side::Left => (x, y + h * 0.5),
        Side::Right => (x + w, y + h * 0.5),
        Side::Top => (x + w * 0.5, y),
        Side::Bottom => (x + w * 0.5, y + h),
    }
}

fn auto_sides(from: &CanvasNode, to: &CanvasNode) -> (Side, Side) {
    let fcx = from.x + from.width * 0.5;
    let fcy = from.y + from.height * 0.5;
    let tcx = to.x + to.width * 0.5;
    let tcy = to.y + to.height * 0.5;
    let dx = tcx - fcx;
    let dy = tcy - fcy;
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            (Side::Right, Side::Left)
        } else {
            (Side::Left, Side::Right)
        }
    } else if dy >= 0.0 {
        (Side::Bottom, Side::Top)
    } else {
        (Side::Top, Side::Bottom)
    }
}

/// Computes `(start, ctrl_a, ctrl_b, end)` in canvas coordinates for an edge.
pub fn edge_endpoints(
    from: &CanvasNode,
    to: &CanvasNode,
    edge: &CanvasEdge,
) -> ((f32, f32), (f32, f32), (f32, f32), (f32, f32)) {
    let (auto_from, auto_to) = auto_sides(from, to);
    let f_side = Side::from_opt(edge.from_side.as_deref()).unwrap_or(auto_from);
    let t_side = Side::from_opt(edge.to_side.as_deref()).unwrap_or(auto_to);

    let p0 = side_anchor(from, f_side);
    let p3 = side_anchor(to, t_side);

    let dist = ((p3.0 - p0.0).hypot(p3.1 - p0.1) * 0.35).clamp(32.0, 140.0);
    let (fnx, fny) = f_side.normal();
    let (tnx, tny) = t_side.normal();

    let ctrl_a = (p0.0 + fnx * dist, p0.1 + fny * dist);
    let ctrl_b = (p3.0 + tnx * dist, p3.1 + tny * dist);
    (p0, ctrl_a, ctrl_b, p3)
}

/// Maps JSON Canvas palette presets ("1".."6") or `#RRGGBB` hex colors to `0xRRGGBB`.
pub fn canvas_color_u32(color: Option<&str>) -> Option<u32> {
    let c = color?.trim();
    match c {
        "1" => Some(0xc4746e), // Kanagawa Red
        "2" => Some(0xb6927b), // Orange
        "3" => Some(0xc4b28a), // Yellow
        "4" => Some(0x87a987), // Green
        "5" => Some(0x7ca6d8), // Blue / Aqua
        "6" => Some(0xa292a3), // Violet
        _ => {
            let hex = c.strip_prefix('#')?;
            if hex.len() == 6 {
                u32::from_str_radix(hex, 16).ok()
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_roundtrips_and_cascades_node_deletes() {
        let raw = r##"{
            "customMeta": 42,
            "nodes": [
                {"id": "a", "type": "text", "x": 0, "y": 0, "width": 200, "height": 100, "text": "Hello", "customField": true},
                {"id": "b", "type": "file", "x": 400, "y": 20, "width": 220, "height": 120, "file": "Notes/Test.md", "color": "4"}
            ],
            "edges": [
                {"id": "e1", "fromNode": "a", "toNode": "b", "label": "links", "toEnd": "arrow"}
            ]
        }"##;

        let mut doc = CanvasDoc::parse(raw).unwrap();
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.edges.len(), 1);
        assert_eq!(doc.extra.get("customMeta").and_then(|v| v.as_i64()), Some(42));
        assert_eq!(
            doc.nodes[0].extra.get("customField").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(canvas_color_u32(doc.nodes[1].color.as_deref()), Some(0x87a987));
        assert_eq!(canvas_color_u32(Some("#112233")), Some(0x112233));

        let (p0, _, _, p3) = edge_endpoints(&doc.nodes[0], &doc.nodes[1], &doc.edges[0]);
        assert!((p0.0 - 200.0).abs() < 0.01);
        assert!((p3.0 - 400.0).abs() < 0.01);

        let new_id = doc.add_text_node(50.0, 250.0, "Extra".into());
        assert_eq!(doc.nodes.len(), 3);
        let serialized = doc.to_json_pretty();
        let reparsed = CanvasDoc::parse(&serialized).unwrap();
        assert_eq!(reparsed.nodes.len(), 3);
        assert_eq!(reparsed.extra.get("customMeta").and_then(|v| v.as_i64()), Some(42));

        doc.remove_node("a");
        assert_eq!(doc.nodes.len(), 2);
        assert!(doc.edges.is_empty());
        assert!(doc.nodes.iter().any(|n| n.id == new_id));
    }

    #[test]
    fn handles_empty_canvas() {
        let doc = CanvasDoc::parse("   \n ").unwrap();
        assert!(doc.nodes.is_empty());
        assert!(doc.edges.is_empty());
        assert!(doc.bounding_box().is_none());
    }
}
