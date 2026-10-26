//! Minimal aligned text tables for terminal output.

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Table { headers: headers.iter().map(|s| s.to_string()).collect(), rows: vec![] }
    }

    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    pub fn render(&self) -> String {
        let n = self.headers.len();
        let mut w: Vec<usize> = self.headers.iter().map(|h| h.chars().count()).collect();
        for r in &self.rows {
            for (i, c) in r.iter().enumerate().take(n) {
                w[i] = w[i].max(c.chars().count());
            }
        }
        let line = |cells: &[String]| {
            let mut s = String::new();
            for (i, c) in cells.iter().enumerate().take(n) {
                let pad = w[i].saturating_sub(c.chars().count());
                s.push_str(c);
                if i + 1 < n {
                    s.push_str(&" ".repeat(pad + 2));
                }
            }
            s.trim_end().to_string()
        };
        let mut out = line(&self.headers);
        out.push('\n');
        out.push_str(&w.iter().map(|x| "-".repeat(*x)).collect::<Vec<_>>().join("  "));
        out.push('\n');
        for r in &self.rows {
            out.push_str(&line(r));
            out.push('\n');
        }
        out
    }

    pub fn print(&self) {
        print!("{}", self.render());
    }
}

/// Shorten long text for table cells.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_columns() {
        let mut t = Table::new(&["id", "title"]);
        t.row(vec!["a".into(), "Alpha".into()]);
        t.row(vec!["long-id".into(), "B".into()]);
        let r = t.render();
        assert!(r.contains("a        Alpha"));
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
