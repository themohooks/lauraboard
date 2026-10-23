//! Search, filters and sorting for media lists.

use std::cmp::Ordering;

use lb_core::media::{MediaItem, MediaKind};

/// Library filter state (from the UI).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaFilter {
    pub text: String,
    /// Empty = all kinds.
    pub kinds: Vec<MediaKind>,
    pub category: Option<String>,
    pub problems_only: bool,
    /// Library folder: its items and those of its subfolders.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Title,
    Kind,
    Duration,
    Category,
    Id,
}

impl SortKey {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => SortKey::Kind,
            2 => SortKey::Duration,
            3 => SortKey::Category,
            4 => SortKey::Id,
            _ => SortKey::Title,
        }
    }
}

/// File problems shown to the operator (probe error or disabled item).
pub fn has_problem(m: &MediaItem) -> bool {
    m.probe.error.is_some()
}

impl MediaFilter {
    pub fn matches(&self, m: &MediaItem) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&m.kind) {
            return false;
        }
        if let Some(c) = &self.category
            && m.category.as_deref() != Some(c.as_str())
        {
            return false;
        }
        if self.problems_only && !has_problem(m) {
            return false;
        }
        if let Some(f) = self.folder.as_deref().filter(|f| !f.is_empty())
            && !(m.folder == f || m.folder.strip_prefix(f).is_some_and(|r| r.starts_with('/')))
        {
            return false;
        }
        let t = self.text.trim().to_lowercase();
        t.is_empty()
            || m.title.to_lowercase().contains(&t)
            || m.id.to_lowercase().contains(&t)
            || m.category.as_deref().is_some_and(|c| c.to_lowercase().contains(&t))
            || m.tags.iter().any(|x| x.to_lowercase().contains(&t))
            || m.path.to_lowercase().contains(&t)
            || m.original_name.as_deref().is_some_and(|n| n.to_lowercase().contains(&t))
    }
}

/// Indices of matching items, sorted.
pub fn select(items: &[MediaItem], f: &MediaFilter, key: SortKey, descending: bool) -> Vec<usize> {
    let mut idx: Vec<usize> = items.iter().enumerate().filter(|(_, m)| f.matches(m)).map(|(i, _)| i).collect();
    idx.sort_by(|&a, &b| {
        let (a, b) = (&items[a], &items[b]);
        let o = match key {
            SortKey::Title => cmp_text(&a.title, &b.title),
            SortKey::Kind => (a.kind as u8).cmp(&(b.kind as u8)).then_with(|| cmp_text(&a.title, &b.title)),
            SortKey::Duration => a.play_duration().cmp(&b.play_duration()),
            SortKey::Category => cmp_text(a.category.as_deref().unwrap_or(""), b.category.as_deref().unwrap_or("")),
            SortKey::Id => a.id.cmp(&b.id),
        };
        if descending { o.reverse() } else { o }
    });
    idx
}

fn cmp_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// Distinct categories present in the library (sorted).
pub fn categories(items: &[MediaItem]) -> Vec<String> {
    let mut v: Vec<String> = items.iter().filter_map(|m| m.category.clone()).filter(|c| !c.is_empty()).collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::Dur;

    fn item(id: &str, title: &str, kind: MediaKind, secs: i64) -> MediaItem {
        let mut m = MediaItem::new(id, format!("{id}.mp4"), kind, Dur(secs * 1000));
        m.title = title.into();
        m
    }

    #[test]
    fn filter_and_sort() {
        let items = vec![
            item("b", "Вечер", MediaKind::Program, 60),
            item("a", "утро", MediaKind::Program, 30),
            item("c", "Ролик", MediaKind::Ad, 10),
        ];
        let f = MediaFilter { kinds: vec![MediaKind::Program], ..Default::default() };
        assert_eq!(select(&items, &f, SortKey::Title, false), vec![0, 1]);
        assert_eq!(select(&items, &f, SortKey::Duration, false), vec![1, 0]);
        let f = MediaFilter { text: "УТР".into(), ..Default::default() };
        assert_eq!(select(&items, &f, SortKey::Title, false), vec![1]);
    }

    #[test]
    fn problems_only() {
        let mut items = vec![item("a", "a", MediaKind::Program, 1), item("b", "b", MediaKind::Program, 1)];
        items[1].probe.error = Some("missing".into());
        let f = MediaFilter { problems_only: true, ..Default::default() };
        assert_eq!(select(&items, &f, SortKey::Id, false), vec![1]);
    }
}
