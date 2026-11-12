//! Output helpers every listing command shares: the rows and cells a `list` or `show` is built from
//! and the one place that turns them into a table or tab-separated lines.

use std::fmt::Write as _;

use tabled::builder::Builder;
use tabled::settings::Style;

use crate::style;

/// A show row of field, value and note.
#[must_use]
pub fn row(
    field: impl Into<String>,
    value: impl Into<String>,
    note: impl Into<String>,
) -> Vec<String> {
    vec![field.into(), value.into(), note.into()]
}

/// A note in the failure style on a terminal.
#[must_use]
pub fn flag(text: &str, terminal: bool) -> String {
    if terminal {
        let style = style::failure_style();
        format!("{style}{text}{style:#}")
    } else {
        text.to_owned()
    }
}

/// `rows` under `header`: a rounded table on a terminal, else tab-separated with one header line.
#[must_use]
pub fn render_rows(header: &[&str], rows: Vec<Vec<String>>, terminal: bool) -> String {
    if terminal {
        let mut builder = Builder::new();
        builder.push_record(header.iter().map(|h| (*h).to_owned()));
        for row in rows {
            builder.push_record(row);
        }
        let mut table = builder.build();
        table.with(Style::rounded());
        return format!("{table}\n");
    }
    let mut out = String::new();
    let _ = writeln!(out, "{}", header.join("\t"));
    for row in rows {
        let _ = writeln!(out, "{}", row.join("\t"));
    }
    out
}

/// The date of an RFC 3339 timestamp.
#[must_use]
pub fn fetch_date(timestamp: &str) -> String {
    timestamp.split('T').next().unwrap_or(timestamp).to_owned()
}

/// A byte size: MiB with one decimal on a terminal, bytes when piped.
#[must_use]
pub fn size(bytes: u64, terminal: bool) -> String {
    if !terminal {
        return bytes.to_string();
    }
    let tenths = bytes.saturating_mul(10).saturating_add(1 << 19) >> 20;
    format!("{}.{} MiB", tenths / 10, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_rounds_to_one_decimal_mib() {
        assert_eq!(size(1_572_864, true), "1.5 MiB");
        assert_eq!(size(123_456_789, true), "117.7 MiB");
        assert_eq!(size(1_572_864, false), "1572864");
    }
}
