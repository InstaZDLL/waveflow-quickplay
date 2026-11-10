// Windows document icons for the types QuickPlay opens. build.rs embeds
// each `assets/file-icons/{stem}.ico` into the executable under its
// resource id; `--register` (src/integration.rs) points each type's ProgID
// at `"exe",-{id}`, so a file whose default player is QuickPlay shows its
// format instead of the application icon.
//
// Shared with build.rs through `include!`: plain constants only, no
// reference to the rest of the crate.

/// `(icon file stem, resource id)`. Id 1 is the application icon, the
/// lowest id, so it stays the one Explorer shows for the executable.
/// Ids are written into the registry: never renumber one, only add.
pub const FILE_ICONS: &[(&str, u16)] = &[
    ("audio", 101),
    ("flac", 102),
    ("mp3", 103),
    ("wav", 104),
    ("aiff", 105),
    ("ogg", 106),
    ("opus", 107),
    ("m4a", 108),
    ("mp4", 109),
    ("aac", 110),
    ("dsf", 111),
    ("dff", 112),
    ("m3u", 113),
    ("m3u8", 114),
    ("wv", 115),
    ("ape", 116),
];

/// The icon of an extension with no file of its own.
#[allow(dead_code)]
pub const GENERIC_FILE_ICON: u16 = 101;

/// Extensions that share another one's icon: `(extension, icon stem)`.
#[allow(dead_code)]
pub const FILE_ICON_ALIASES: &[(&str, &str)] = &[("aif", "aiff"), ("oga", "ogg")];
