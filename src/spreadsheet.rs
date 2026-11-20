use anyhow::{Context, Result};
use ironcalc::base::{Model, types::CellType};
use ironcalc::export::save_to_xlsx;
use ironcalc::import::load_from_xlsx;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetFormat {
    Xlsx,
    Csv,
    Tsv,
}

impl SheetFormat {
    pub fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "csv" => Self::Csv,
            "tsv" => Self::Tsv,
            _ => Self::Xlsx,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Xlsx => "XLSX",
            Self::Csv => "CSV",
            Self::Tsv => "TSV",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Empty,
    Number,
    Text,
    Formula,
    Error,
    Bool,
}

pub struct SpreadsheetDoc {
    pub model: Model<'static>,
    pub format: SheetFormat,
    pub dirty: bool,
}

impl SpreadsheetDoc {
    pub fn open(path: &Path) -> Result<Self> {
        let format = SheetFormat::from_path(path);

        let mut model = match format {
            SheetFormat::Xlsx => {
                let path_str = path.to_string_lossy().to_string();
                load_from_xlsx(&path_str, "en", "UTC", "en")
                    .map_err(|e| anyhow::anyhow!("Failed to open XLSX {}: {e:?}", path.display()))?
            }
            SheetFormat::Csv | SheetFormat::Tsv => {
                let mut model = Model::new_empty("sheet", "en", "UTC", "en")
                    .map_err(|e| anyhow::anyhow!("IronCalc init error: {e}"))?;
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    model.workbook.name = name.to_string();
                }
                let delimiter = if format == SheetFormat::Tsv { b'\t' } else { b',' };
                let mut reader = csv::ReaderBuilder::new()
                    .has_headers(false)
                    .flexible(true)
                    .delimiter(delimiter)
                    .from_path(path)
                    .with_context(|| format!("read delimited file {}", path.display()))?;

                for (r_idx, record) in reader.records().enumerate() {
                    let record = record?;
                    let row = (r_idx + 1) as i32;
                    for (c_idx, field) in record.iter().enumerate() {
                        if !field.is_empty() {
                            let _ = model.set_user_input(0, row, (c_idx + 1) as i32, field.to_string());
                        }
                    }
                }
                model
            }
        };

        model.evaluate();
        Ok(Self {
            model,
            format,
            dirty: false,
        })
    }

    pub fn new_empty(path: &Path) -> Result<Self> {
        let format = SheetFormat::from_path(path);
        let mut model = Model::new_empty("sheet", "en", "UTC", "en")
            .map_err(|e| anyhow::anyhow!("IronCalc init error: {e}"))?;
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            model.workbook.name = name.to_string();
        }
        model.evaluate();
        Ok(Self {
            model,
            format,
            dirty: false,
        })
    }

    pub fn save(&mut self, path: &Path) -> Result<()> {
        match self.format {
            SheetFormat::Xlsx => self.export_xlsx(path)?,
            SheetFormat::Csv => self.export_delimited(0, path, b',')?,
            SheetFormat::Tsv => self.export_delimited(0, path, b'\t')?,
        }
        self.dirty = false;
        Ok(())
    }

    pub fn export_xlsx(&self, out: &Path) -> Result<()> {
        if let Some(parent) = out.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let path_str = out.to_string_lossy().to_string();
        save_to_xlsx(&self.model, &path_str)
            .map_err(|e| anyhow::anyhow!("Failed to save XLSX {}: {e:?}", out.display()))
    }

    pub fn export_csv(&self, sheet: u32, out: &Path) -> Result<()> {
        self.export_delimited(sheet, out, b',')
    }

    fn export_delimited(&self, sheet: u32, out: &Path, delimiter: u8) -> Result<()> {
        if let Some(parent) = out.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let (max_row, max_col) = self.sheet_bounds(sheet);
        let mut writer = csv::WriterBuilder::new()
            .delimiter(delimiter)
            .from_path(out)
            .with_context(|| format!("write delimited file {}", out.display()))?;

        if max_row > 0 && max_col > 0 {
            for r in 1..=max_row {
                let mut row_vals = Vec::with_capacity(max_col as usize);
                for c in 1..=max_col {
                    row_vals.push(self.display_value(sheet, r, c));
                }
                writer.write_record(&row_vals)?;
            }
        }
        writer.flush()?;
        Ok(())
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.model
            .get_worksheets_properties()
            .into_iter()
            .map(|p| p.name)
            .collect()
    }

    pub fn sheet_bounds(&self, sheet: u32) -> (i32, i32) {
        let mut max_row = 0;
        let mut max_col = 0;
        for cell in self.model.get_all_cells() {
            if cell.index == sheet {
                max_row = max_row.max(cell.row);
                max_col = max_col.max(cell.column);
            }
        }
        (max_row, max_col)
    }

    pub fn grid_dimensions(&self, sheet: u32) -> (i32, i32) {
        let (max_row, max_col) = self.sheet_bounds(sheet);
        ((max_row + 40).max(100), (max_col + 12).max(26))
    }

    pub fn display_value(&self, sheet: u32, row: i32, col: i32) -> String {
        if self.model.is_empty_cell(sheet, row, col).unwrap_or(true) {
            return String::new();
        }
        self.model
            .get_formatted_cell_value(sheet, row, col)
            .unwrap_or_default()
    }

    pub fn raw_input(&self, sheet: u32, row: i32, col: i32) -> String {
        if self.model.is_empty_cell(sheet, row, col).unwrap_or(true) {
            return String::new();
        }
        if let Ok(Some(formula)) = self.model.get_cell_formula(sheet, row, col) {
            return formula;
        }
        self.model
            .get_localized_cell_content(sheet, row, col)
            .unwrap_or_default()
    }

    pub fn cell_kind(&self, sheet: u32, row: i32, col: i32) -> CellKind {
        if self.model.is_empty_cell(sheet, row, col).unwrap_or(true) {
            return CellKind::Empty;
        }
        if let Ok(display) = self.model.get_formatted_cell_value(sheet, row, col)
            && display.starts_with('#')
        {
            return CellKind::Error;
        }
        if matches!(self.model.get_cell_formula(sheet, row, col), Ok(Some(_))) {
            return CellKind::Formula;
        }
        match self.model.get_cell_type(sheet, row, col) {
            Ok(CellType::Number) => CellKind::Number,
            Ok(CellType::LogicalValue) => CellKind::Bool,
            Ok(CellType::ErrorValue) => CellKind::Error,
            _ => CellKind::Text,
        }
    }

    pub fn set_cell(&mut self, sheet: u32, row: i32, col: i32, input: &str) -> Result<()> {
        if row < 1 || col < 1 {
            anyhow::bail!("Row and column indices must be >= 1");
        }
        self.model
            .set_user_input(sheet, row, col, input.to_string())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.model.evaluate();
        self.dirty = true;
        Ok(())
    }

    pub fn paste_tsv(&mut self, sheet: u32, start_row: i32, start_col: i32, tsv: &str) -> Result<usize> {
        let mut count = 0usize;
        for (r_off, line) in tsv.lines().enumerate() {
            for (c_off, val) in line.split('\t').enumerate() {
                let row = start_row + r_off as i32;
                let col = start_col + c_off as i32;
                if row >= 1 && col >= 1 {
                    self.model
                        .set_user_input(sheet, row, col, val.to_string())
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    count += 1;
                }
            }
        }
        self.model.evaluate();
        if count > 0 {
            self.dirty = true;
        }
        Ok(count)
    }

    pub fn insert_row(&mut self, sheet: u32, row: i32) -> Result<()> {
        self.model
            .insert_rows(sheet, row.max(1), 1)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.model.evaluate();
        self.dirty = true;
        Ok(())
    }

    pub fn delete_row(&mut self, sheet: u32, row: i32) -> Result<()> {
        self.model
            .delete_rows(sheet, row.max(1), 1)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.model.evaluate();
        self.dirty = true;
        Ok(())
    }

    pub fn insert_col(&mut self, sheet: u32, col: i32) -> Result<()> {
        self.model
            .insert_columns(sheet, col.max(1), 1)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.model.evaluate();
        self.dirty = true;
        Ok(())
    }

    pub fn delete_col(&mut self, sheet: u32, col: i32) -> Result<()> {
        self.model
            .delete_columns(sheet, col.max(1), 1)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.model.evaluate();
        self.dirty = true;
        Ok(())
    }

    pub fn add_sheet(&mut self, name: Option<&str>) -> Result<String> {
        let created = match name {
            Some(n) if !n.trim().is_empty() => {
                let trimmed = n.trim().to_string();
                self.model
                    .add_sheet(&trimmed)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                trimmed
            }
            _ => self.model.new_sheet().0,
        };
        self.dirty = true;
        Ok(created)
    }

    pub fn rename_sheet(&mut self, old_name: &str, new_name: &str) -> Result<()> {
        self.model
            .rename_sheet(old_name, new_name.trim())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.dirty = true;
        Ok(())
    }

    pub fn delete_sheet(&mut self, sheet_index: u32) -> Result<()> {
        self.model
            .delete_sheet(sheet_index)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.model.evaluate();
        self.dirty = true;
        Ok(())
    }
}

pub fn col_label(mut col: i32) -> String {
    if col < 1 {
        return String::new();
    }
    let mut chars = Vec::new();
    while col > 0 {
        let rem = ((col - 1) % 26) as u8;
        chars.push((b'A' + rem) as char);
        col = (col - 1) / 26;
    }
    chars.into_iter().rev().collect()
}

pub fn cell_ref(row: i32, col: i32) -> String {
    format!("{}{}", col_label(col), row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn col_labels_convert_correctly() {
        assert_eq!(col_label(1), "A");
        assert_eq!(col_label(26), "Z");
        assert_eq!(col_label(27), "AA");
        assert_eq!(cell_ref(12, 28), "AB12");
    }

    #[test]
    fn xlsx_formula_evaluation_and_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("budget.xlsx");

        let mut doc = SpreadsheetDoc::new_empty(&path).unwrap();
        doc.set_cell(0, 1, 1, "10").unwrap();
        doc.set_cell(0, 2, 1, "20").unwrap();
        doc.set_cell(0, 3, 1, "30").unwrap();
        doc.set_cell(0, 4, 1, "=SUM(A1:A3)").unwrap();
        doc.set_cell(0, 5, 1, "=AVERAGE(A1:A3)").unwrap();

        assert_eq!(doc.display_value(0, 4, 1), "60");
        assert_eq!(doc.display_value(0, 5, 1), "20");
        assert_eq!(doc.cell_kind(0, 4, 1), CellKind::Formula);
        assert_eq!(doc.raw_input(0, 4, 1), "=SUM(A1:A3)");

        doc.save(&path).unwrap();
        assert!(!doc.dirty);

        let reopened = SpreadsheetDoc::open(&path).unwrap();
        assert_eq!(reopened.display_value(0, 4, 1), "60");
        assert_eq!(reopened.display_value(0, 5, 1), "20");
        assert_eq!(reopened.raw_input(0, 4, 1), "=SUM(A1:A3)");
    }

    #[test]
    fn csv_load_save_paste_and_clear() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("table.csv");
        std::fs::write(&path, "Item,Cost\nSword,50\nShield,30\n").unwrap();

        let mut doc = SpreadsheetDoc::open(&path).unwrap();
        assert_eq!(doc.display_value(0, 1, 1), "Item");
        assert_eq!(doc.display_value(0, 3, 2), "30");

        doc.paste_tsv(0, 4, 1, "Potion\t15\nBow\t95").unwrap();
        assert_eq!(doc.display_value(0, 5, 2), "95");

        doc.set_cell(0, 2, 2, "").unwrap();
        assert_eq!(doc.display_value(0, 2, 2), "");

        doc.save(&path).unwrap();
        let csv_out = std::fs::read_to_string(&path).unwrap();
        assert!(csv_out.contains("Bow,95"));
    }
}
