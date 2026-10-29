use zenkit_rs_core::text;

/// The player's progress over time (`LOG.SAV`): rows the engine adds as the game goes on, each
/// with the game time, level, experience, attributes and talents.
///
/// The engine writes it as tab-separated text. Values are kept as text, since the guild column
/// holds the localised guild name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Statistics {
    /// Like `minutes`, `level` or `strength`.
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Statistics {
    pub fn from_bytes(data: &[u8]) -> Self {
        // Every line ends in a tab, which would add an empty last field.
        let fields = |line: &[u8]| -> Vec<String> {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = line.strip_suffix(b"\t").unwrap_or(line);
            line.split(|&b| b == b'\t').map(text::latin1).collect()
        };
        let mut lines = data.split(|&b| b == b'\n').filter(|line| !line.trim_ascii().is_empty());
        let columns = lines.next().map(fields).unwrap_or_default();
        Self { columns, rows: lines.map(fields).collect() }
    }

    /// One column's values, from the first row to the last, or nothing if there is no such
    /// column.
    pub fn column<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> + 'a {
        let index = self.columns.iter().position(|c| c.eq_ignore_ascii_case(name));
        self.rows.iter().filter_map(move |row| Some(row.get(index?)?.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tab_separated_rows() {
        let stats = Statistics::from_bytes(
            b"minutes\tlevel\tguild\t\r\n0\t1\tGildenlos\t\r\n60\t2\t\t\r\n",
        );
        assert_eq!(stats.columns, ["minutes", "level", "guild"]);
        assert_eq!(stats.rows.len(), 2);
        assert_eq!(stats.column("LEVEL").collect::<Vec<_>>(), ["1", "2"]);
        assert_eq!(stats.column("guild").collect::<Vec<_>>(), ["Gildenlos", ""]);
        assert_eq!(stats.column("missing").count(), 0);
        assert_eq!(Statistics::from_bytes(b""), Statistics::default());
    }
}
