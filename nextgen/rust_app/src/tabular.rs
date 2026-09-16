//! Reader for the delimited text tables used to feed catalogues, detection lists and
//! photometric measurements into the command line tools.
//!
//! The format is deliberately minimal and editable by hand: one header line naming the
//! columns, then one record per line. Fields are separated by commas, semicolons, tabs
//! or runs of spaces. Blank lines and lines starting with `#` are ignored.

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub source: String,
}

fn split_record(line: &str) -> Vec<String> {
    line.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect()
}

impl Table {
    pub fn parse(text: &str, source: &str) -> Result<Self, String> {
        let mut records = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));

        let header = records
            .next()
            .ok_or_else(|| format!("table '{source}' has no header line"))?;
        let columns: Vec<String> = split_record(header)
            .into_iter()
            .map(|name| name.to_ascii_lowercase())
            .collect();

        if columns.is_empty() {
            return Err(format!("table '{source}' has an empty header line"));
        }

        let mut rows = Vec::new();
        for (offset, line) in records.enumerate() {
            let fields = split_record(line);
            if fields.len() != columns.len() {
                return Err(format!(
                    "table '{source}': record {} has {} fields but the header declares {}",
                    offset + 1,
                    fields.len(),
                    columns.len()
                ));
            }
            rows.push(fields);
        }

        if rows.is_empty() {
            return Err(format!("table '{source}' contains no record"));
        }

        Ok(Self {
            columns,
            rows,
            source: source.to_string(),
        })
    }

    pub fn read(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read table '{path}': {error}"))?;
        Table::parse(&text, path)
    }

    pub fn column_index(&self, name: &str) -> Result<usize, String> {
        let wanted = name.to_ascii_lowercase();
        self.columns
            .iter()
            .position(|column| *column == wanted)
            .ok_or_else(|| {
                format!(
                    "table '{}' has no column '{name}'; available columns: {}",
                    self.source,
                    self.columns.join(", ")
                )
            })
    }

    /// Index of the first column present among the accepted aliases.
    pub fn column_index_any(&self, names: &[&str]) -> Result<usize, String> {
        for name in names {
            if let Ok(index) = self.column_index(name) {
                return Ok(index);
            }
        }
        Err(format!(
            "table '{}' has none of the columns {}; available columns: {}",
            self.source,
            names.join(" / "),
            self.columns.join(", ")
        ))
    }

    pub fn text(&self, row: usize, column: usize) -> Result<&str, String> {
        self.rows
            .get(row)
            .and_then(|fields| fields.get(column))
            .map(String::as_str)
            .ok_or_else(|| {
                format!(
                    "table '{}': no field at row {row} column {column}",
                    self.source
                )
            })
    }

    pub fn number(&self, row: usize, column: usize) -> Result<f64, String> {
        let raw = self.text(row, column)?;
        raw.parse::<f64>().map_err(|_| {
            format!(
                "table '{}': field '{raw}' at row {} column '{}' is not a number",
                self.source,
                row + 1,
                self.columns[column]
            )
        })
    }

    pub fn integer(&self, row: usize, column: usize) -> Result<i64, String> {
        let raw = self.text(row, column)?;
        raw.parse::<i64>().map_err(|_| {
            format!(
                "table '{}': field '{raw}' at row {} column '{}' is not an integer",
                self.source,
                row + 1,
                self.columns[column]
            )
        })
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_separated_table_with_comments() {
        let text = "# detections\nx, y, flux, snr\n250.0, 250.0, 50000.0, 60.0\n450.5,250.0,30000,45\n";
        let table = Table::parse(text, "memory").unwrap();
        assert_eq!(table.columns, vec!["x", "y", "flux", "snr"]);
        assert_eq!(table.row_count(), 2);
        let x = table.column_index("X").unwrap();
        assert!((table.number(1, x).unwrap() - 450.5).abs() < 1e-12);
    }

    #[test]
    fn accepts_whitespace_and_tab_separators() {
        let text = "id\tra_deg\tdec_deg\n1001   10.6847\t41.2687\n";
        let table = Table::parse(text, "memory").unwrap();
        assert_eq!(table.row_count(), 1);
        assert_eq!(table.integer(0, 0).unwrap(), 1001);
        assert!((table.number(0, 2).unwrap() - 41.2687).abs() < 1e-12);
    }

    #[test]
    fn column_alias_resolution_reports_available_columns() {
        let table = Table::parse("name,mag\nHD1,7.2\n", "memory").unwrap();
        assert_eq!(table.column_index_any(&["magnitude", "mag"]).unwrap(), 1);
        let error = table.column_index_any(&["flux", "adu"]).unwrap_err();
        assert!(error.contains("name, mag"), "{error}");
    }

    #[test]
    fn rejects_records_with_a_wrong_field_count() {
        let error = Table::parse("x,y\n1,2\n3\n", "memory").unwrap_err();
        assert!(error.contains("record 2"), "{error}");
    }

    #[test]
    fn rejects_empty_and_header_only_tables() {
        assert!(Table::parse("", "memory").is_err());
        assert!(Table::parse("# only a comment\n", "memory").is_err());
        assert!(Table::parse("x,y\n", "memory").is_err());
    }

    #[test]
    fn reports_non_numeric_fields_with_their_location() {
        let table = Table::parse("x,y\n1.0,abc\n", "memory").unwrap();
        let error = table.number(0, 1).unwrap_err();
        assert!(error.contains("'abc'"), "{error}");
        assert!(error.contains("row 1"), "{error}");
    }
}
