//! The playback queue: an ordered list of files, a cursor, what is known
//! about each entry so far, and the order the cursor moves in (repeat,
//! shuffle).

use std::cell::Cell;
use std::collections::HashSet;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};

use crate::config::Repeat;
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

pub struct Queue {
    entries: Vec<TrackEntry>,
    current: Option<usize>,
    /// The current row was removed and the cursor sits on the row that
    /// moved up into its place, which has not played: Next plays that row
    /// rather than the one after it.
    cursor_pending: bool,
    next_id: u64,
    repeat: Repeat,
    shuffle: bool,
    /// Ids played in the current shuffle round, so every track comes once
    /// before any comes again. Emptied when the round is complete.
    played: HashSet<u64>,
    /// Ids in the order they were played, for Previous under shuffle.
    history: Vec<u64>,
    /// xorshift state for the shuffle draws, in a `Cell` so `next_index`
    /// draws from `&self` like its sequential counterpart.
    rng: Cell<u64>,
}

impl Default for Queue {
    fn default() -> Self {
        // A different order on every launch; the hasher's keys are random.
        let seed = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        Self {
            entries: Vec::new(),
            current: None,
            cursor_pending: false,
            next_id: 0,
            repeat: Repeat::Off,
            shuffle: false,
            played: HashSet::new(),
            history: Vec::new(),
            rng: Cell::new(seed | 1),
        }
    }
}

impl Queue {
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Switching shuffle on starts a round with the current track as its
    /// first; switching it off forgets the round.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
        self.played.clear();
        self.history.clear();
        // A row only waiting under the cursor has not played: it stays
        // free for the round's first draw.
        if let Some(id) = self
            .current()
            .filter(|_| !self.cursor_pending)
            .map(|e| e.id)
        {
            self.played.insert(id);
            self.history.push(id);
        }
    }

    fn playable(&self, index: usize) -> bool {
        self.entries[index].failed.is_none()
    }

    /// Every playable track has been played in this shuffle round.
    fn round_complete(&self) -> bool {
        self.entries
            .iter()
            .filter(|e| e.failed.is_none())
            .all(|e| self.played.contains(&e.id))
    }

    /// Playable rows other than the current one; a row only waiting under
    /// the cursor ([`Self::cursor_pending`]) counts among them.
    fn others(&self) -> impl Iterator<Item = usize> + '_ {
        let current_id = self
            .current()
            .filter(|_| !self.cursor_pending)
            .map(|e| e.id);
        (0..self.entries.len())
            .filter(move |i| self.playable(*i) && Some(self.entries[*i].id) != current_id)
    }

    /// Those of [`Self::others`] not yet played in this shuffle round.
    fn unplayed(&self) -> impl Iterator<Item = usize> + '_ {
        self.others()
            .filter(|i| !self.played.contains(&self.entries[*i].id))
    }

    /// Whether [`Self::draw`] would find a row, without drawing one: the
    /// view asks every frame.
    fn can_draw(&self) -> bool {
        self.unplayed().next().is_some()
            || (self.repeat == Repeat::All
                && (self.others().next().is_some()
                    || self.current.is_some_and(|i| self.playable(i))))
    }

    /// A random playable row other than the current one, not yet played in
    /// this round; when the round is complete, any other row if the queue
    /// repeats, none otherwise.
    fn draw(&self) -> Option<usize> {
        let mut pool: Vec<usize> = self.unplayed().collect();
        if pool.is_empty() {
            if self.repeat != Repeat::All {
                return None;
            }
            pool = self.others().collect();
            if pool.is_empty() {
                // Alone in the queue: it comes again.
                return self.current.filter(|i| self.playable(*i));
            }
        }
        // xorshift64*: enough to pick a row.
        let mut x = self.rng.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng.set(x);
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        Some(pool[(r % pool.len() as u64) as usize])
    }

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

    /// The cursor moves on to `index`: Next, the end of a track, a pick
    /// from the list. The track joins the history and the shuffle round;
    /// a round found complete starts over from it.
    pub fn set_current(&mut self, index: Option<usize>) {
        self.move_cursor(index, false);
    }

    /// The cursor goes back to `index`, a track the history holds
    /// (Previous): the history unwinds to it, so Previous keeps walking
    /// back from there, and the round is left as it is, since nothing is
    /// played anew.
    pub fn set_current_back(&mut self, index: usize) {
        self.move_cursor(Some(index), true);
    }

    fn move_cursor(&mut self, index: Option<usize>, back: bool) {
        self.current = index.filter(|i| *i < self.entries.len());
        self.cursor_pending = false;
        let Some(id) = self.current().map(|e| e.id) else {
            return;
        };
        if back {
            match self.history.iter().rposition(|h| *h == id) {
                Some(k) => self.history.truncate(k + 1),
                None => self.history.push(id),
            }
            self.played.insert(id);
            return;
        }
        if self.round_complete() {
            self.played.clear();
        }
        self.played.insert(id);
        if self.history.last() != Some(&id) {
            self.history.push(id);
        }
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
        let id = self.entries.remove(index).id;
        self.history.retain(|h| *h != id);
        self.played.retain(|p| *p != id);
        match self.current {
            Some(c) if c == index => {
                // The cursor keeps its position, on the row that moved up
                // into it: Play and Next both play that row next. When the
                // last row went, nothing moved up: it clamps to the new last
                // row, which Play plays again and Next goes past.
                if self.entries.is_empty() {
                    self.current = None;
                    self.cursor_pending = false;
                } else if c >= self.entries.len() {
                    self.current = Some(self.entries.len() - 1);
                    self.cursor_pending = false;
                } else {
                    self.cursor_pending = true;
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
        self.cursor_pending = false;
        self.played.clear();
        self.history.clear();
    }

    /// The row to play after the current one, skipping rows already known
    /// to fail: the next one down, or the first again when the queue
    /// repeats, or a draw when it is shuffled. `None` when the queue is
    /// over.
    pub fn next_index(&self) -> Option<usize> {
        if self.shuffle {
            return self.draw();
        }
        self.sequential_next()
    }

    /// Whether [`Self::next_index`] would find a row. Under shuffle this
    /// does not draw, so it can be asked every frame.
    pub fn has_next(&self) -> bool {
        if self.shuffle {
            self.can_draw()
        } else {
            self.sequential_next().is_some()
        }
    }

    fn sequential_next(&self) -> Option<usize> {
        let start = match self.current {
            Some(c) if self.cursor_pending => c,
            Some(c) => c + 1,
            None => 0,
        };
        let after = (start..self.entries.len()).find(|i| self.playable(*i));
        match (after, self.repeat) {
            (Some(i), _) => Some(i),
            (None, Repeat::All) => (0..start.min(self.entries.len())).find(|i| self.playable(*i)),
            (None, _) => None,
        }
    }

    /// The row before the current one, skipping rows already known to
    /// fail: the one above, the last one when the queue repeats, or the one
    /// shuffle played before.
    pub fn prev_index(&self) -> Option<usize> {
        let end = self.current?;
        if self.shuffle && self.cursor_pending {
            // The row under the cursor has not played: back is the last
            // track that did.
            return self.history.iter().rev().find_map(|h| {
                self.entries
                    .iter()
                    .position(|e| e.id == *h && e.failed.is_none())
            });
        }
        if self.shuffle {
            let id = self.entries[end].id;
            let at = self.history.iter().rposition(|h| *h == id)?;
            return self.history[..at].iter().rev().find_map(|h| {
                self.entries
                    .iter()
                    .position(|e| e.id == *h && e.failed.is_none())
            });
        }
        let before = (0..end).rev().find(|i| self.playable(*i));
        match (before, self.repeat) {
            (Some(i), _) => Some(i),
            (None, Repeat::All) => (end + usize::from(!self.cursor_pending)..self.entries.len())
                .rev()
                .find(|i| self.playable(*i)),
            (None, _) => None,
        }
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
    fn next_after_removing_the_current_row_plays_what_moved_up() {
        let mut q = queue_of(3);
        q.set_current(Some(0));
        assert!(q.remove(0));
        assert_eq!(q.next_index(), Some(0), "\"1\" moved up and comes next");
        assert_eq!(q.get(0).map(|e| e.title()), Some("1"));
        let mut two = queue_of(2);
        two.set_current(Some(0));
        two.remove(0);
        assert!(two.has_next());
        assert_eq!(two.next_index(), Some(0));
        // Once it plays, the cursor is an ordinary one again.
        q.set_current(Some(0));
        assert_eq!(q.next_index(), Some(1));
        // A track that survives keeps the usual next.
        let mut q = queue_of(3);
        q.set_current(Some(1));
        q.remove(0);
        assert_eq!(q.next_index(), Some(1));
    }

    #[test]
    fn under_shuffle_the_row_that_moved_up_can_be_drawn() {
        let mut q = queue_of(2);
        q.set_current(Some(0));
        q.set_shuffle(true);
        q.remove(0);
        assert!(q.has_next());
        assert_eq!(q.next_index(), Some(0));
        assert_eq!(q.prev_index(), None, "nothing that played is left");
        // Shuffle switched on after the removal: the row is still unplayed.
        let mut q = queue_of(2);
        q.set_current(Some(0));
        q.remove(0);
        q.set_shuffle(true);
        assert_eq!(q.next_index(), Some(0));
        assert_eq!(q.prev_index(), None);
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

    #[test]
    fn repeat_all_wraps_both_ways_and_off_stops() {
        let mut q = queue_of(3);
        q.set_current(Some(2));
        assert_eq!(q.next_index(), None);
        q.set_current(Some(0));
        assert_eq!(q.prev_index(), None);
        q.set_repeat(Repeat::All);
        assert_eq!(q.prev_index(), Some(2));
        q.set_current(Some(2));
        assert_eq!(q.next_index(), Some(0));
        // Alone in the queue, the track comes again.
        let mut one = queue_of(1);
        one.set_repeat(Repeat::All);
        one.set_current(Some(0));
        assert_eq!(one.next_index(), Some(0));
    }

    #[test]
    fn shuffle_plays_every_track_once_per_round() {
        let mut q = queue_of(6);
        q.entries[3].failed = Some("nope".into());
        q.set_current(Some(0));
        q.set_shuffle(true);
        let mut seen = vec![0];
        while let Some(i) = q.next_index() {
            assert_ne!(Some(i), q.current_index(), "never the current one");
            assert!(!seen.contains(&i), "{i} came twice in a round");
            seen.push(i);
            q.set_current(Some(i));
        }
        seen.sort();
        assert_eq!(
            seen,
            vec![0, 1, 2, 4, 5],
            "every playable row, the failed one skipped"
        );
        // Off: the round ends there. All: a new round starts, elsewhere.
        q.set_repeat(Repeat::All);
        let next = q.next_index().expect("a new round");
        assert_ne!(Some(next), q.current_index());
    }

    #[test]
    fn shuffle_previous_walks_back_through_what_played() {
        let mut q = queue_of(5);
        q.set_current(Some(2));
        q.set_shuffle(true);
        let a = q.next_index().unwrap();
        q.set_current(Some(a));
        let b = q.next_index().unwrap();
        q.set_current(Some(b));
        assert_eq!(q.prev_index(), Some(a));
        q.set_current_back(a);
        assert_eq!(q.prev_index(), Some(2));
        q.set_current_back(2);
        assert_eq!(q.prev_index(), None, "nothing played before the first");
    }

    /// A new round under "repeat all" may draw a track already in the
    /// history; moving on to it records it, and Previous still finds the
    /// last track of the previous round.
    #[test]
    fn a_new_round_keeps_the_previous_round_behind_it() {
        let mut q = queue_of(2);
        q.set_repeat(Repeat::All);
        q.set_current(Some(0));
        q.set_shuffle(true);
        assert_eq!(q.next_index(), Some(1));
        q.set_current(Some(1));
        assert_eq!(q.next_index(), Some(0), "the new round");
        q.set_current(Some(0));
        assert_eq!(q.prev_index(), Some(1));
        q.set_current_back(1);
        assert_eq!(q.prev_index(), Some(0));
    }

    /// With repeat off, going back after the round is over does not open
    /// a new round: Next stays unavailable.
    #[test]
    fn going_back_after_the_round_keeps_it_over() {
        let mut q = queue_of(2);
        q.set_current(Some(0));
        q.set_shuffle(true);
        let i = q.next_index().unwrap();
        q.set_current(Some(i));
        assert!(!q.has_next());
        // Playing the last track again (repeat one, Play after the end)
        // goes through the same path and leaves the round over too.
        q.set_current_back(i);
        assert!(!q.has_next());
        let back = q.prev_index().unwrap();
        q.set_current_back(back);
        assert!(!q.has_next());
        assert_eq!(q.next_index(), None);
        // A pick from the list starts a new round from it.
        q.set_current(Some(back));
        assert!(q.has_next());
    }

    /// A track that failed or was removed after it played is skipped on
    /// the way back, and the walk goes on past it instead of bouncing
    /// between its two neighbours.
    #[test]
    fn shuffle_previous_walks_past_a_dead_track() {
        let mut q = queue_of(4);
        q.set_current(Some(0));
        q.set_shuffle(true);
        let a = q.next_index().unwrap();
        q.set_current(Some(a));
        let b = q.next_index().unwrap();
        q.set_current(Some(b));
        q.entries[a].failed = Some("nope".into());
        assert_eq!(q.prev_index(), Some(0));
        q.set_current_back(0);
        assert_eq!(q.prev_index(), None);

        let mut q = queue_of(4);
        q.set_current(Some(0));
        q.set_shuffle(true);
        let a = q.next_index().unwrap();
        q.set_current(Some(a));
        let b = q.next_index().unwrap();
        q.set_current(Some(b));
        let b_id = q.entries[b].id;
        q.remove(a);
        let b = q.entries.iter().position(|e| e.id == b_id).unwrap();
        assert_eq!(q.current_index(), Some(b));
        assert_eq!(q.prev_index(), Some(0));
        q.set_current_back(0);
        assert_eq!(q.prev_index(), None);
    }

    #[test]
    fn has_next_agrees_with_next_index_without_drawing() {
        let mut q = queue_of(3);
        q.set_current(Some(0));
        q.set_shuffle(true);
        assert!(q.has_next());
        let before = q.rng.get();
        assert!(q.has_next());
        assert_eq!(q.rng.get(), before, "no draw");
        for _ in 0..2 {
            let i = q.next_index().unwrap();
            q.set_current(Some(i));
        }
        assert!(!q.has_next(), "round over");
        assert_eq!(q.next_index(), None);
        q.set_repeat(Repeat::All);
        assert!(q.has_next());
    }
}
