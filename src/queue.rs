//! The playback queue: an ordered list of files, a cursor, and what is known
//! about each entry so far.

use std::path::{Path, PathBuf};

use crate::library::TrackMeta;

#[derive(Debug, Clone)]
pub struct TrackEntry {
    /// Stable identity across inserts and removals, used to match loader
    /// results and the playing track to a row.
    pub id: u64,
    pub path: PathBuf,
    pub meta: Option<TrackMeta>,
    /// Why the engine could not play it, when it could not.
    pub failed: Option<String>,
}

impl TrackEntry {
    pub fn title(&self) -> &str {
        self.meta
            .as_ref()
            .and_then(|m| m.title.as_deref())
            .unwrap_or_else(|| file_stem(&self.path))
    }

    pub fn artist(&self) -> Option<&str> {
        self.meta.as_ref().and_then(|m| m.artist.as_deref())
    }

    pub fn album(&self) -> Option<&str> {
        self.meta.as_ref().and_then(|m| m.album.as_deref())
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.meta.as_ref().and_then(|m| m.duration_ms)
    }
}

fn file_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown track")
}

#[derive(Default)]
pub struct Queue {
    entries: Vec<TrackEntry>,
    current: Option<usize>,
    next_id: u64,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&TrackEntry> {
        self.entries.get(index)
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current(&self) -> Option<&TrackEntry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    pub fn set_current(&mut self, index: Option<usize>) {
        self.current = index.filter(|i| *i < self.entries.len());
    }

    pub fn entry_mut(&mut self, id: u64) -> Option<&mut TrackEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Append and return the new entry's id.
    pub fn push(&mut self, path: PathBuf) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.entries.push(TrackEntry {
            id,
            path,
            meta: None,
            failed: None,
        });
        id
    }

    /// Remove one row, keeping the cursor on the same track when it
    /// survives. Returns whether the removed row was the current one.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.entries.remove(index);
        match self.current {
            Some(c) if c == index => {
                // The cursor keeps its position so Next plays what moved
                // up into it; clamp when the last row went.
                if self.entries.is_empty() {
                    self.current = None;
                } else if c >= self.entries.len() {
                    self.current = Some(self.entries.len() - 1);
                }
                true
            }
            Some(c) if c > index => {
                self.current = Some(c - 1);
                false
            }
            _ => false,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
    }

    /// The row after the cursor, skipping rows already known to fail.
    pub fn next_index(&self) -> Option<usize> {
        let start = self.current.map_or(0, |c| c + 1);
        (start..self.entries.len()).find(|i| self.entries[*i].failed.is_none())
    }

    /// The row before the cursor, skipping rows already known to fail.
    pub fn prev_index(&self) -> Option<usize> {
        let end = self.current?;
        (0..end).rev().find(|i| self.entries[*i].failed.is_none())
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.entries.iter().filter_map(|e| e.duration_ms()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_of(n: usize) -> Queue {
        let mut q = Queue::default();
        for i in 0..n {
            q.push(PathBuf::from(format!("{i}.flac")));
        }
        q
    }

    #[test]
    fn removing_before_the_cursor_keeps_the_same_track_current() {
        let mut q = queue_of(4);
        q.set_current(Some(2));
        let was_current = q.remove(0);
        assert!(!was_current);
        assert_eq!(q.current_index(), Some(1));
        assert_eq!(q.current().map(|e| e.title()), Some("2"));
    }

    #[test]
    fn removing_the_last_current_row_clamps_the_cursor() {
        let mut q = queue_of(2);
        q.set_current(Some(1));
        assert!(q.remove(1));
        assert_eq!(q.current_index(), Some(0));
        assert!(q.remove(0));
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn next_and_prev_skip_failed_rows() {
        let mut q = queue_of(4);
        q.set_current(Some(0));
        q.entries[1].failed = Some("nope".into());
        assert_eq!(q.next_index(), Some(2));
        q.set_current(Some(3));
        assert_eq!(q.prev_index(), Some(2));
        q.entries[2].failed = Some("nope".into());
        assert_eq!(q.prev_index(), Some(0));
    }
}
