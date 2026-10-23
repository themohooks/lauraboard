//! Virtual folders of the media library. A folder is a `/`-separated path
//! stored on each item (`folder`); empty folders are kept in
//! `Library::folders`. Files on disk are not moved.

use std::collections::BTreeSet;

use super::Library;

/// Canonical folder path: trimmed parts, no empty parts, `/` separators.
pub fn normalize(path: &str) -> String {
    path.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("/")
}

/// `a/b` is `a/b` itself or below it.
fn within(path: &str, folder: &str) -> bool {
    path == folder || path.strip_prefix(folder).is_some_and(|r| r.starts_with('/'))
}

/// Replace the leading `from` of `path` with `to`.
fn reparent(path: &str, from: &str, to: &str) -> String {
    let rest = &path[from.len()..];
    normalize(&format!("{to}{rest}"))
}

impl Library {
    /// Every folder (explicit and used by items) including parents, sorted.
    pub fn all_folders(&self) -> Vec<String> {
        let mut set = BTreeSet::new();
        for f in self.folders.iter().map(String::as_str).chain(self.items.iter().map(|m| m.folder.as_str())) {
            let f = normalize(f);
            let mut acc = String::new();
            for part in f.split('/').filter(|p| !p.is_empty()) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                set.insert(acc.clone());
            }
        }
        set.into_iter().collect()
    }

    /// Create a folder (returns its canonical path).
    pub fn create_folder(&mut self, path: &str) -> Result<String, String> {
        let p = normalize(path);
        if p.is_empty() {
            return Err("folder name is empty".into());
        }
        if !self.all_folders().contains(&p) {
            self.folders.push(p.clone());
            self.folders.sort();
        }
        Ok(p)
    }

    /// Rename / move a folder with everything inside it.
    pub fn rename_folder(&mut self, from: &str, to: &str) -> Result<String, String> {
        let (from, to) = (normalize(from), normalize(to));
        if from.is_empty() || to.is_empty() {
            return Err("folder name is empty".into());
        }
        if within(&to, &from) && to != from {
            return Err("a folder cannot be moved into itself".into());
        }
        for f in &mut self.folders {
            if within(f, &from) {
                *f = reparent(f, &from, &to);
            }
        }
        for m in &mut self.items {
            if within(&m.folder, &from) {
                m.folder = reparent(&m.folder, &from, &to);
            }
        }
        self.dedup_folders();
        Ok(to)
    }

    /// Remove a folder; its items and subfolders move to the parent.
    pub fn delete_folder(&mut self, path: &str) {
        let p = normalize(path);
        if p.is_empty() {
            return;
        }
        let parent = p.rsplit_once('/').map(|(a, _)| a.to_string()).unwrap_or_default();
        self.folders.retain(|f| *f != p);
        for f in &mut self.folders {
            if within(f, &p) {
                *f = reparent(f, &p, &parent);
            }
        }
        for m in &mut self.items {
            if within(&m.folder, &p) {
                m.folder = reparent(&m.folder, &p, &parent);
            }
        }
        self.dedup_folders();
    }

    /// Put items into a folder (created when missing).
    pub fn move_to_folder(&mut self, ids: &[String], folder: &str) -> usize {
        let f = normalize(folder);
        let mut n = 0;
        for m in self.items.iter_mut().filter(|m| ids.contains(&m.id)) {
            m.folder = f.clone();
            n += 1;
        }
        if !f.is_empty() && !self.folders.contains(&f) {
            self.folders.push(f);
            self.dedup_folders();
        }
        n
    }

    fn dedup_folders(&mut self) {
        self.folders.retain(|f| !f.is_empty());
        self.folders.sort();
        self.folders.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dur;
    use crate::media::{MediaItem, MediaKind};

    fn lib() -> Library {
        let mut a = MediaItem::new("a", "a.mp4", MediaKind::Program, Dur::secs(1));
        a.folder = "Фильмы/Комедии".into();
        let b = MediaItem::new("b", "b.mp4", MediaKind::Program, Dur::secs(1));
        let mut l = Library::new(vec![a, b]);
        l.folders = vec!["Пусто".into()];
        l
    }

    #[test]
    fn lists_parents_and_empty_folders() {
        assert_eq!(normalize(" a / /b\\c/ "), "a/b/c");
        assert_eq!(lib().all_folders(), ["Пусто", "Фильмы", "Фильмы/Комедии"]);
    }

    #[test]
    fn rename_and_delete_move_contents() {
        let mut l = lib();
        l.rename_folder("Фильмы", "Кино").unwrap();
        assert_eq!(l.get("a").unwrap().folder, "Кино/Комедии");
        assert!(l.rename_folder("Кино", "Кино/x").is_err());
        l.delete_folder("Кино/Комедии");
        assert_eq!(l.get("a").unwrap().folder, "Кино");
        l.move_to_folder(&["b".into()], "Новое");
        assert_eq!(l.get("b").unwrap().folder, "Новое");
        assert!(l.all_folders().contains(&"Новое".to_string()));
        // "Фильмы2" is not inside "Фильмы"
        assert!(!within("Фильмы2", "Фильмы"));
    }
}
