//! Synced lyrics from an `.lrc` sidecar: `song.flac` → `song.lrc`.
//!
//! Only what the player needs: timed lines, `[offset:]`, enhanced-LRC word
//! stamps stripped. Lines are kept in time order; an empty line is a
//! deliberate "nothing to show" marker, which the UI turns back into the
//! track title, as PotPlayer does.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// The largest sidecar read. A song's lyrics are a few kilobytes; a file
/// far bigger is not lyrics, and is left alone rather than parsed on the
/// UI thread.
const MAX_SIDECAR_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    at_ms: u64,
    /// Shared by every timestamp of one LRC line (`[00:02][00:07]chorus`).
    text: Arc<str>,
}

#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    lines: Vec<Line>,
}

impl Lyrics {
    /// Reads the `.lrc` next to `audio` (any case of the extension). `None`
    /// when there is no such file or it holds no timed line.
    pub fn load_for(audio: &Path) -> Option<Self> {
        audio.file_stem()?;
        // `with_extension` on the full path: a stem with dots in it
        // (`01. Intro.flac`) must become `01. Intro.lrc`, not `01.lrc`.
        // Every casing of the extension, for case-sensitive file systems;
        // on the others the first one already finds any.
        let candidate = (0..8u8)
            .map(|mask| {
                let ext: String = "lrc"
                    .chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if mask & (1 << i) != 0 {
                            c.to_ascii_uppercase()
                        } else {
                            c
                        }
                    })
                    .collect();
                audio.with_extension(ext)
            })
            .find(|p| p.is_file())?;
        let mut bytes = Vec::new();
        let read = std::fs::File::open(&candidate)
            .and_then(|f| f.take(MAX_SIDECAR_BYTES + 1).read_to_end(&mut bytes));
        if let Err(e) = read {
            tracing::warn!(path = %candidate.display(), error = %e, "lrc unreadable");
            return None;
        }
        if bytes.len() as u64 > MAX_SIDECAR_BYTES {
            tracing::warn!(path = %candidate.display(), max = MAX_SIDECAR_BYTES, "lrc too large, ignored");
            return None;
        }
        let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes));
        let parsed = Self::parse(&text);
        if let Some(l) = &parsed {
            tracing::debug!(path = %candidate.display(), lines = l.lines.len(), "lrc loaded");
        }
        parsed
    }

    /// Parses LRC text. `None` when no line carries a timestamp.
    pub fn parse(text: &str) -> Option<Self> {
        let mut offset_ms: i64 = 0;
        let mut raw: Vec<Line> = Vec::new();
        for line in text.lines() {
            let mut rest = line.trim();
            let mut stamps: Vec<u64> = Vec::new();
            while let Some(after) = rest.strip_prefix('[') {
                let Some(end) = after.find(']') else {
                    break;
                };
                let tag = &after[..end];
                rest = after[end + 1..].trim_start();
                if let Some(ms) = parse_stamp(tag) {
                    stamps.push(ms);
                } else if let Some(v) = tag.strip_prefix("offset:") {
                    // A malformed one leaves the offset as it was.
                    match v.trim().parse() {
                        Ok(ms) => offset_ms = ms,
                        Err(_) => tracing::debug!(tag, "lrc offset unreadable, ignored"),
                    }
                }
                // Any other `[id:value]` tag is metadata; ignored.
            }
            if stamps.is_empty() {
                continue;
            }
            let text: Arc<str> = strip_word_stamps(rest).into();
            for at_ms in stamps {
                raw.push(Line {
                    at_ms,
                    text: Arc::clone(&text),
                });
            }
        }
        if raw.is_empty() {
            return None;
        }
        // In file time first: the offset can bring several lines down to
        // zero, and among those the last one in time must stay last.
        raw.sort_by_key(|l| l.at_ms);
        // A positive offset shifts the lyrics earlier (LRC convention).
        for l in &mut raw {
            let shifted = i128::from(l.at_ms) - i128::from(offset_ms);
            l.at_ms = shifted.clamp(0, i128::from(u64::MAX)) as u64;
        }
        Some(Self { lines: raw })
    }

    /// The line in force at `pos_ms`: the last one that started at or
    /// before it. `None` before the first line or when that line is empty.
    #[cfg(test)]
    pub fn line_at(&self, pos_ms: u64) -> Option<&str> {
        self.current(pos_ms).map(|(_, text)| text)
    }

    /// Same as [`line_at`](Self::line_at), with the line's start time.
    pub fn current(&self, pos_ms: u64) -> Option<(u64, &str)> {
        let idx = self.lines.partition_point(|l| l.at_ms <= pos_ms);
        let line = self.lines.get(idx.checked_sub(1)?)?;
        (!line.text.is_empty()).then_some((line.at_ms, &*line.text))
    }

    /// When the displayed line next changes after `pos_ms`, if ever.
    pub fn next_change_after(&self, pos_ms: u64) -> Option<u64> {
        let idx = self.lines.partition_point(|l| l.at_ms <= pos_ms);
        self.lines.get(idx).map(|l| l.at_ms)
    }
}

/// `mm:ss`, `mm:ss.xx`, `mm:ss.xxx`, `mm:ss:xx` → milliseconds.
fn parse_stamp(tag: &str) -> Option<u64> {
    let (min, rest) = tag.split_once(':')?;
    let min: u64 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((s, f)) => (s, Some(f)),
        None => (rest, None),
    };
    let sec: u64 = sec.trim().parse().ok()?;
    let frac_ms = match frac {
        None => 0,
        Some(f) => {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let v: u64 = f.parse().ok()?;
            match f.len() {
                1 => v * 100,
                2 => v * 10,
                3 => v,
                _ => return None,
            }
        }
    };
    // Checked: a sidecar is user data, and a huge number must not panic the
    // UI thread in a debug build or wrap in release.
    min.checked_mul(60_000)?
        .checked_add(sec.checked_mul(1000)?)?
        .checked_add(frac_ms)
}

/// Removes enhanced-LRC `<mm:ss.xx>` word stamps and collapses the spaces.
/// One pass: a `<` that opens no stamp is text, and the scan goes on from
/// the next `<` or `>` after it, so stray brackets are never searched past
/// more than once.
fn strip_word_stamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(['<', '>']) {
            Some(end) if after.as_bytes()[end] == b'>' && parse_stamp(&after[..end]).is_some() => {
                rest = &after[end + 1..];
            }
            Some(end) => {
                out.push('<');
                out.push_str(&after[..end]);
                rest = &after[end..];
            }
            None => {
                out.push('<');
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
[ar:Artist]
[ti:Title]
[00:01.00]first line
[00:03.50]
[00:05.25]<00:05.25>third <00:05.80>line
[00:02.00][00:07.00]repeated
";

    #[test]
    fn parses_and_orders() {
        let l = Lyrics::parse(SAMPLE).unwrap();
        assert_eq!(l.lines.len(), 5);
        assert_eq!(l.lines[0].at_ms, 1000);
        assert_eq!(l.lines[1].at_ms, 2000);
        assert_eq!(&*l.lines[1].text, "repeated");
        assert_eq!(&*l.lines[3].text, "third line");
        assert_eq!(l.lines[4].at_ms, 7000);
    }

    #[test]
    fn line_in_force() {
        let l = Lyrics::parse(SAMPLE).unwrap();
        assert_eq!(l.line_at(0), None);
        assert_eq!(l.line_at(1000), Some("first line"));
        assert_eq!(l.line_at(2999), Some("repeated"));
        assert_eq!(l.line_at(4000), None, "an empty line shows the title");
        assert_eq!(l.line_at(6000), Some("third line"));
        assert_eq!(l.line_at(60_000), Some("repeated"));
        assert_eq!(l.next_change_after(0), Some(1000));
        assert_eq!(l.next_change_after(1000), Some(2000));
        assert_eq!(l.next_change_after(7000), None);
    }

    #[test]
    fn stamps_and_offset() {
        assert_eq!(parse_stamp("01:02"), Some(62_000));
        assert_eq!(parse_stamp("01:02.5"), Some(62_500));
        assert_eq!(parse_stamp("01:02.05"), Some(62_050));
        assert_eq!(parse_stamp("01:02.005"), Some(62_005));
        assert_eq!(parse_stamp("01:02:05"), Some(62_050));
        assert_eq!(parse_stamp("ar:Artist"), None);
        assert_eq!(parse_stamp("offset:500"), None);
        assert_eq!(
            parse_stamp("18446744073709551615:00"),
            None,
            "overflow is rejected"
        );
        let l = Lyrics::parse(
            "[offset:-9223372036854775808]
[00:01.00]a
",
        )
        .unwrap();
        assert_eq!(
            l.lines[0].at_ms,
            1000 + 9_223_372_036_854_775_808,
            "a huge negative offset neither panics nor wraps"
        );
        let l = Lyrics::parse("[offset:500]\n[00:01.00]a\n").unwrap();
        assert_eq!(l.lines[0].at_ms, 500);
        let l = Lyrics::parse("[offset:-500]\n[00:01.00]a\n").unwrap();
        assert_eq!(l.lines[0].at_ms, 1500);
    }

    #[test]
    fn sidecar_next_to_a_dotted_name() {
        let dir = std::env::temp_dir().join(format!("quickplay-lrc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let audio = dir.join("01. Intro.flac");
        std::fs::write(&audio, b"").unwrap();
        assert!(Lyrics::load_for(&audio).is_none(), "no sidecar yet");
        std::fs::write(
            dir.join("01. Intro.lrc"),
            "[00:01.00]hello
",
        )
        .unwrap();
        let l = Lyrics::load_for(&audio).expect("sidecar found");
        assert_eq!(l.line_at(1000), Some("hello"));
        // Any casing of the extension, on case-sensitive file systems too.
        let other = dir.join("02. Outro.flac");
        std::fs::write(&other, b"").unwrap();
        std::fs::write(dir.join("02. Outro.lRc"), "[00:02.00]bye\n").unwrap();
        let l = Lyrics::load_for(&other).expect("mixed-case sidecar found");
        assert_eq!(l.line_at(2000), Some("bye"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_offset_keeps_the_good_one() {
        let l = Lyrics::parse("[offset:500]\n[offset:bad]\n[00:01]a\n").unwrap();
        assert_eq!(l.lines[0].at_ms, 500);
    }

    #[test]
    fn lines_brought_to_zero_keep_their_order() {
        // Out of order in the file; the offset brings both to zero, and the
        // later one (b, at 3 s) is the one in force there.
        let l = Lyrics::parse("[offset:5000]\n[00:03]b\n[00:01]a\n").unwrap();
        assert_eq!(l.line_at(0), Some("b"));
    }

    #[test]
    fn stray_brackets_are_text_and_cheap() {
        assert_eq!(strip_word_stamps("a < b > c"), "a < b > c");
        assert_eq!(strip_word_stamps("<<00:01.00>x"), "<x");
        assert_eq!(strip_word_stamps("x <y"), "x <y");
        // Linear: 400k unmatched `<` took seconds with the old scan.
        let started = std::time::Instant::now();
        let many = "<".repeat(400_000);
        assert_eq!(strip_word_stamps(&many).len(), 400_000);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn repeated_stamps_share_their_text() {
        let line = format!("{}{}\n", "[0:1]".repeat(1000), "x".repeat(1000));
        let l = Lyrics::parse(&line).unwrap();
        assert_eq!(l.lines.len(), 1000);
        assert!(Arc::ptr_eq(&l.lines[0].text, &l.lines[999].text));
    }

    #[test]
    fn no_timed_lines_is_none() {
        assert!(Lyrics::parse("[ar:x]\njust text\n").is_none());
        assert!(Lyrics::parse("").is_none());
    }
}
