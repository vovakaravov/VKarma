#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub id: u64,
    pub identifier: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    pub settings: Option<String>,
    pub text: String,
}

impl SubtitleCue {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    pub fn visible_char_count(&self) -> usize {
        let mut count = 0usize;
        let mut in_tag = false;
        for ch in self.text.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                '\n' | '\r' => {}
                _ if !in_tag => count += 1,
                _ => {}
            }
        }
        count
    }

    pub fn cps(&self) -> f32 {
        let dur_ms = self.duration_ms();
        if dur_ms == 0 {
            return 0.0;
        }
        (self.visible_char_count() as f32) / (dur_ms as f32 / 1000.0)
    }
}

#[derive(Debug, Clone)]
pub struct SubtitleDoc {
    pub format: SubtitleFormat,
    pub header: String,
    pub cues: Vec<SubtitleCue>,
    next_id: u64,
}

impl SubtitleDoc {
    pub fn parse(raw: &str, default_format: SubtitleFormat) -> Self {
        let clean = raw.strip_prefix('\u{feff}').unwrap_or(raw);
        let normalized = clean.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.split('\n').collect();

        let first_non_empty = lines.iter().position(|l| !l.trim().is_empty());
        let format = if let Some(ix) = first_non_empty
            && lines[ix].trim().starts_with("WEBVTT")
        {
            SubtitleFormat::Vtt
        } else {
            default_format
        };

        let header;
        let mut cues = Vec::new();
        let mut next_id = 1u64;

        // Split into blank-line-separated blocks
        let mut idx = 0usize;
        if format == SubtitleFormat::Vtt
            && let Some(first_ix) = first_non_empty
            && lines[first_ix].trim().starts_with("WEBVTT")
        {
            let mut h_lines = vec![lines[first_ix]];
            idx = first_ix + 1;
            while idx < lines.len() && !lines[idx].trim().is_empty() {
                h_lines.push(lines[idx]);
                idx += 1;
            }
            header = h_lines.join("\n");
        } else {
            header = match format {
                SubtitleFormat::Vtt => "WEBVTT".to_string(),
                SubtitleFormat::Srt => String::new(),
            };
        }

        while idx < lines.len() {
            while idx < lines.len() && lines[idx].trim().is_empty() {
                idx += 1;
            }
            if idx >= lines.len() {
                break;
            }
            let block_start = idx;
            while idx < lines.len() && !lines[idx].trim().is_empty() {
                idx += 1;
            }
            let block = &lines[block_start..idx];

            // Find the timing line containing `-->`
            let Some(arrow_pos) = block.iter().position(|l| l.contains("-->")) else {
                // Skip non-cue VTT blocks like NOTE / STYLE if there's no timing line
                continue;
            };

            let identifier = if arrow_pos > 0 {
                let first = block[0].trim();
                if first.is_empty() {
                    None
                } else {
                    Some(first.to_string())
                }
            } else {
                None
            };

            if let Some((start_ms, end_ms, settings)) = parse_timing_line(block[arrow_pos]) {
                let text = block[arrow_pos + 1..].join("\n");
                cues.push(SubtitleCue {
                    id: next_id,
                    identifier,
                    start_ms,
                    end_ms,
                    settings,
                    text,
                });
                next_id += 1;
            }
        }

        Self {
            format,
            header,
            cues,
            next_id,
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if self.format == SubtitleFormat::Vtt {
            let hdr = if self.header.trim().is_empty() {
                "WEBVTT"
            } else {
                self.header.trim_end()
            };
            out.push_str(hdr);
            out.push_str("\n\n");
        }

        for (ix, cue) in self.cues.iter().enumerate() {
            if ix > 0 {
                out.push('\n');
            }
            match self.format {
                SubtitleFormat::Srt => {
                    out.push_str(&format!("{}\n", ix + 1));
                }
                SubtitleFormat::Vtt => {
                    if let Some(ref ident) = cue.identifier
                        && !ident.trim().is_empty()
                    {
                        out.push_str(ident.trim());
                        out.push('\n');
                    }
                }
            }
            out.push_str(&format_timestamp_ms(cue.start_ms, self.format));
            out.push_str(" --> ");
            out.push_str(&format_timestamp_ms(cue.end_ms, self.format));
            if let Some(ref settings) = cue.settings
                && !settings.trim().is_empty()
            {
                out.push(' ');
                out.push_str(settings.trim());
            }
            out.push('\n');
            out.push_str(cue.text.trim_end_matches('\n'));
            out.push('\n');
        }
        out
    }

    pub fn cue_overlaps_prev(&self, idx: usize) -> bool {
        idx > 0 && idx < self.cues.len() && self.cues[idx].start_ms < self.cues[idx - 1].end_ms
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.cues.iter().map(|c| c.end_ms).max().unwrap_or(0)
    }

    pub fn shift_all(&mut self, delta_ms: i64) {
        for cue in &mut self.cues {
            cue.start_ms = shift_ms(cue.start_ms, delta_ms);
            cue.end_ms = shift_ms(cue.end_ms, delta_ms).max(cue.start_ms);
        }
    }

    pub fn insert_cue_after(&mut self, after_id: Option<u64>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let insert_idx = after_id
            .and_then(|aid| self.cues.iter().position(|c| c.id == aid).map(|i| i + 1))
            .unwrap_or(self.cues.len());

        let start_ms = if insert_idx > 0 {
            self.cues[insert_idx - 1].end_ms + 100
        } else {
            0
        };
        let end_ms = if insert_idx < self.cues.len() && self.cues[insert_idx].start_ms > start_ms + 500
        {
            (start_ms + 2000).min(self.cues[insert_idx].start_ms.saturating_sub(50))
        } else {
            start_ms + 2000
        };

        self.cues.insert(
            insert_idx,
            SubtitleCue {
                id,
                identifier: None,
                start_ms,
                end_ms,
                settings: None,
                text: "New subtitle line".to_string(),
            },
        );
        id
    }

    pub fn remove_cue(&mut self, id: u64) {
        self.cues.retain(|c| c.id != id);
    }
}

fn shift_ms(base: u64, delta_ms: i64) -> u64 {
    if delta_ms >= 0 {
        base.saturating_add(delta_ms as u64)
    } else {
        base.saturating_sub(delta_ms.unsigned_abs())
    }
}

fn parse_timing_line(line: &str) -> Option<(u64, u64, Option<String>)> {
    let (left, right) = line.split_once("-->")?;
    let start_ms = parse_timestamp_ms(left.trim())?;
    let mut right_parts = right.split_whitespace();
    let end_tok = right_parts.next()?;
    let end_ms = parse_timestamp_ms(end_tok)?;
    let rest: Vec<&str> = right_parts.collect();
    let settings = if rest.is_empty() {
        None
    } else {
        Some(rest.join(" "))
    };
    Some((start_ms, end_ms, settings))
}

pub fn parse_timestamp_ms(s: &str) -> Option<u64> {
    let t = s.trim();
    let (hms, ms_part) = if let Some((a, b)) = t.rsplit_once(',') {
        (a, b)
    } else if let Some((a, b)) = t.rsplit_once('.') {
        (a, b)
    } else {
        (t, "0")
    };

    let ms_digits: String = ms_part.chars().take(3).collect();
    let mut ms = ms_digits.parse::<u64>().ok()?;
    if ms_digits.len() == 1 {
        ms *= 100;
    } else if ms_digits.len() == 2 {
        ms *= 10;
    }

    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [m, sec] => (0u64, m.parse::<u64>().ok()?, sec.parse::<u64>().ok()?),
        [h, m, sec] => (
            h.parse::<u64>().ok()?,
            m.parse::<u64>().ok()?,
            sec.parse::<u64>().ok()?,
        ),
        _ => return None,
    };

    Some((((h * 60 + m) * 60) + sec) * 1000 + ms)
}

pub fn format_timestamp_ms(ms: u64, format: SubtitleFormat) -> String {
    let total_secs = ms / 1000;
    let rem_ms = ms % 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    let sep = match format {
        SubtitleFormat::Srt => ',',
        SubtitleFormat::Vtt => '.',
    };
    format!("{h:02}:{m:02}:{s:02}{sep}{rem_ms:03}")
}

pub fn format_duration_short(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_roundtrips_srt_and_checks_cps_overlap() {
        let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,000\r\nHello world!\r\n\r\n2\r\n00:00:02,500 --> 00:00:04,500\r\nLine two\r\nLine three\r\n";
        let mut doc = SubtitleDoc::parse(srt, SubtitleFormat::Srt);
        assert_eq!(doc.format, SubtitleFormat::Srt);
        assert_eq!(doc.cues.len(), 2);
        assert_eq!(doc.cues[0].duration_ms(), 2000);
        assert!((doc.cues[0].cps() - 6.0).abs() < 0.01);
        assert!(doc.cue_overlaps_prev(1));

        doc.shift_all(-1500);
        assert_eq!(doc.cues[0].start_ms, 0);
        assert_eq!(doc.cues[0].end_ms, 1500);

        let serialized = doc.to_text();
        assert!(serialized.contains("00:00:00,000 --> 00:00:01,500"));
    }

    #[test]
    fn parses_vtt_with_short_timestamps_and_settings() {
        let vtt = "WEBVTT - Sample\n\nintro\n00:05.250 --> 00:08.500 align:start\n<i>Opening</i> line\n";
        let doc = SubtitleDoc::parse(vtt, SubtitleFormat::Vtt);
        assert_eq!(doc.format, SubtitleFormat::Vtt);
        assert_eq!(doc.header, "WEBVTT - Sample");
        assert_eq!(doc.cues.len(), 1);
        assert_eq!(doc.cues[0].identifier.as_deref(), Some("intro"));
        assert_eq!(doc.cues[0].settings.as_deref(), Some("align:start"));
        assert_eq!(doc.cues[0].visible_char_count(), 12);
        let out = doc.to_text();
        assert!(out.starts_with("WEBVTT - Sample\n\n"));
        assert!(out.contains("00:00:05.250 --> 00:00:08.500 align:start"));
    }
}
