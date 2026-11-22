#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanbanCard {
    pub id: u64,
    pub title: String,
    pub done: bool,
    pub notes: Vec<String>,
}

impl KanbanCard {
    pub fn extract_tags(&self) -> Vec<String> {
        let mut tags = Vec::new();
        for tok in self.title.split_whitespace() {
            if let Some(rest) = tok.strip_prefix('#') {
                let clean: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
                    .collect();
                if !clean.is_empty() && !tags.contains(&clean) {
                    tags.push(clean);
                }
            }
        }
        tags
    }

    pub fn display_title(&self) -> String {
        let mut parts = Vec::new();
        for tok in self.title.split_whitespace() {
            if let Some(rest) = tok.strip_prefix('#') {
                let clean_len = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
                    .map(char::len_utf8)
                    .sum::<usize>();
                if clean_len > 0 && clean_len == rest.len() {
                    continue;
                }
            }
            parts.push(tok);
        }
        let joined = parts.join(" ");
        if joined.is_empty() {
            self.title.clone()
        } else {
            joined
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanbanColumn {
    pub id: u64,
    pub title: String,
    pub cards: Vec<KanbanCard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KanbanBoard {
    pub title: Option<String>,
    pub columns: Vec<KanbanColumn>,
    next_id: u64,
}

impl KanbanBoard {
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn parse(raw: &str) -> Self {
        let mut board = Self {
            title: None,
            columns: Vec::new(),
            next_id: 1,
        };

        let mut in_frontmatter = false;
        let mut first_line = true;

        for line in raw.lines() {
            let trimmed = line.trim();
            if first_line && trimmed == "---" {
                in_frontmatter = true;
                first_line = false;
                continue;
            }
            first_line = false;
            if in_frontmatter {
                if trimmed == "---" {
                    in_frontmatter = false;
                }
                continue;
            }

            if board.title.is_none()
                && board.columns.is_empty()
                && let Some(h1) = trimmed.strip_prefix("# ")
            {
                let t = h1.trim();
                if !t.is_empty() {
                    board.title = Some(t.to_string());
                }
                continue;
            }

            if let Some(h2) = trimmed.strip_prefix("## ") {
                let title = h2.trim();
                let id = board.alloc_id();
                board.columns.push(KanbanColumn {
                    id,
                    title: if title.is_empty() {
                        "Column".to_string()
                    } else {
                        title.to_string()
                    },
                    cards: Vec::new(),
                });
                continue;
            }

            // Indented sub-line under an existing card
            if (line.starts_with("  ") || line.starts_with('\t'))
                && let Some(last_col) = board.columns.last_mut()
                && let Some(last_card) = last_col.cards.last_mut()
                && !trimmed.is_empty()
            {
                let note = trimmed
                    .strip_prefix("- ")
                    .or_else(|| trimmed.strip_prefix("* "))
                    .unwrap_or(trimmed);
                last_card.notes.push(note.to_string());
                continue;
            }

            if let Some((done, title)) = parse_card_line(trimmed) {
                if board.columns.is_empty() {
                    let cid = board.alloc_id();
                    board.columns.push(KanbanColumn {
                        id: cid,
                        title: "Backlog".to_string(),
                        cards: Vec::new(),
                    });
                }
                let card_id = board.alloc_id();
                if let Some(col) = board.columns.last_mut() {
                    col.cards.push(KanbanCard {
                        id: card_id,
                        title,
                        done,
                        notes: Vec::new(),
                    });
                }
            }
        }

        if board.columns.is_empty() {
            for default_col in ["Todo", "Doing", "Done"] {
                let cid = board.alloc_id();
                board.columns.push(KanbanColumn {
                    id: cid,
                    title: default_col.to_string(),
                    cards: Vec::new(),
                });
            }
        }

        board
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        if let Some(ref title) = self.title {
            out.push_str("# ");
            out.push_str(title);
            out.push_str("\n\n");
        }
        for (ix, col) in self.columns.iter().enumerate() {
            if ix > 0 {
                out.push('\n');
            }
            out.push_str("## ");
            out.push_str(&col.title);
            out.push('\n');
            for card in &col.cards {
                out.push_str(if card.done { "- [x] " } else { "- [ ] " });
                out.push_str(&card.title);
                out.push('\n');
                for note in &card.notes {
                    out.push_str("  ");
                    out.push_str(note);
                    out.push('\n');
                }
            }
        }
        out
    }

    pub fn add_column(&mut self, title: String) -> u64 {
        let id = self.alloc_id();
        let clean = title.trim();
        self.columns.push(KanbanColumn {
            id,
            title: if clean.is_empty() {
                format!("Column {}", self.columns.len() + 1)
            } else {
                clean.to_string()
            },
            cards: Vec::new(),
        });
        id
    }

    pub fn rename_column(&mut self, col_id: u64, title: String) {
        let clean = title.trim();
        if clean.is_empty() {
            return;
        }
        if let Some(col) = self.columns.iter_mut().find(|c| c.id == col_id) {
            col.title = clean.to_string();
        }
    }

    pub fn remove_column(&mut self, col_id: u64) {
        self.columns.retain(|c| c.id != col_id);
    }

    pub fn move_column(&mut self, col_id: u64, dir: isize) {
        let Some(idx) = self.columns.iter().position(|c| c.id == col_id) else {
            return;
        };
        let target = (idx as isize + dir).clamp(0, self.columns.len().saturating_sub(1) as isize)
            as usize;
        if idx != target {
            let col = self.columns.remove(idx);
            self.columns.insert(target, col);
        }
    }

    pub fn add_card(&mut self, col_id: u64, title: String) -> Option<u64> {
        let clean = title.trim();
        if clean.is_empty() {
            return None;
        }
        let id = self.alloc_id();
        let col = self.columns.iter_mut().find(|c| c.id == col_id)?;
        col.cards.push(KanbanCard {
            id,
            title: clean.to_string(),
            done: false,
            notes: Vec::new(),
        });
        Some(id)
    }

    pub fn toggle_card(&mut self, col_id: u64, card_id: u64) {
        if let Some(col) = self.columns.iter_mut().find(|c| c.id == col_id)
            && let Some(card) = col.cards.iter_mut().find(|k| k.id == card_id)
        {
            card.done = !card.done;
        }
    }

    pub fn update_card(&mut self, col_id: u64, card_id: u64, title: String) {
        let clean = title.trim();
        if clean.is_empty() {
            return;
        }
        if let Some(col) = self.columns.iter_mut().find(|c| c.id == col_id)
            && let Some(card) = col.cards.iter_mut().find(|k| k.id == card_id)
        {
            card.title = clean.to_string();
        }
    }

    pub fn remove_card(&mut self, col_id: u64, card_id: u64) {
        if let Some(col) = self.columns.iter_mut().find(|c| c.id == col_id) {
            col.cards.retain(|k| k.id != card_id);
        }
    }

    pub fn move_card(
        &mut self,
        from_col_id: u64,
        card_id: u64,
        to_col_id: u64,
        to_index: Option<usize>,
    ) {
        let Some(from_col_idx) = self.columns.iter().position(|c| c.id == from_col_id) else {
            return;
        };
        let Some(from_card_idx) = self.columns[from_col_idx]
            .cards
            .iter()
            .position(|k| k.id == card_id)
        else {
            return;
        };
        let Some(to_col_idx) = self.columns.iter().position(|c| c.id == to_col_id) else {
            return;
        };

        let card = self.columns[from_col_idx].cards.remove(from_card_idx);
        let target_cards = &mut self.columns[to_col_idx].cards;
        let insert_idx = match to_index {
            Some(mut ix) => {
                if from_col_idx == to_col_idx && from_card_idx < ix {
                    ix = ix.saturating_sub(1);
                }
                ix.min(target_cards.len())
            }
            None => target_cards.len(),
        };
        target_cards.insert(insert_idx, card);
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let cols = self.columns.len();
        let mut done = 0usize;
        let mut total = 0usize;
        for c in &self.columns {
            total += c.cards.len();
            done += c.cards.iter().filter(|k| k.done).count();
        }
        (cols, done, total)
    }
}

fn parse_card_line(trimmed: &str) -> Option<(bool, String)> {
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))?;
    if let Some(t) = rest.strip_prefix("[ ] ") {
        Some((false, t.trim().to_string()))
    } else if let Some(t) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
        Some((true, t.trim().to_string()))
    } else {
        Some((false, rest.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_moves_cards_and_roundtrips_cleanly() {
        let md = "\
# Release Plan

## Todo
- [ ] Build canvas view #ui #p0
  Supports zoom and edges
- [ ] Write docs

## Done
- [x] Setup GPUI #core
";
        let mut board = KanbanBoard::parse(md);
        assert_eq!(board.title.as_deref(), Some("Release Plan"));
        assert_eq!(board.columns.len(), 2);
        assert_eq!(board.columns[0].cards.len(), 2);
        assert_eq!(board.columns[0].cards[0].notes, vec!["Supports zoom and edges"]);
        assert_eq!(board.columns[0].cards[0].extract_tags(), vec!["ui", "p0"]);
        assert_eq!(board.columns[0].cards[0].display_title(), "Build canvas view");

        let from_col = board.columns[0].id;
        let to_col = board.columns[1].id;
        let card_id = board.columns[0].cards[1].id;
        board.move_card(from_col, card_id, to_col, Some(0));

        assert_eq!(board.columns[0].cards.len(), 1);
        assert_eq!(board.columns[1].cards.len(), 2);
        assert_eq!(board.columns[1].cards[0].title, "Write docs");

        let out = board.to_markdown();
        let reparsed = KanbanBoard::parse(&out);
        assert_eq!(reparsed.columns[0].cards.len(), 1);
        assert_eq!(reparsed.columns[1].cards.len(), 2);
        assert_eq!(reparsed.columns[1].cards[0].title, "Write docs");
    }

    #[test]
    fn creates_default_columns_on_empty_board() {
        let board = KanbanBoard::parse("");
        assert_eq!(board.columns.len(), 3);
        assert_eq!(board.columns[0].title, "Todo");
        assert_eq!(board.columns[1].title, "Doing");
        assert_eq!(board.columns[2].title, "Done");
    }
}
