//! What may reach the parser at all.
//!
//! Both bounds are applied to the raw text, before a tree exists. That ordering
//! is the point: a bound the parser has already built a tree from is a bound the
//! parser paid for first, and a changelog is the one input here that arrives over
//! the network. The cut lands on a character boundary and on a point where no list
//! marker is half-spelled, because a truncation that lands mid-marker shows the
//! user a bullet that was never in the notes.

use super::*;

/// Upper bound on the changelog this module will hand to the parser.
///
/// `bongocat-packaging` already bounds the announced notes, but that is the publisher's
/// promise, not this process's guarantee: the manifest arrives over the network, and an
/// oversized notes field would otherwise become an unbounded number of elements to lay
/// out. Truncation happens at a character boundary, so the worst case is a changelog that
/// stops mid-sentence with a visible marker.
pub(crate) const MAXIMUM_MARKDOWN_BYTES: usize = 32 * 1024;

/// Upper bound on the block containers a changelog may open.
///
/// Every level of CommonMark block nesting is opened by at least one container marker in
/// the source, so a document with at most this many markers cannot nest deeper than this
/// many levels. That is what makes the bound sound rather than a guess about how deeply
/// the source *looks* nested. A real changelog spends a handful; a document written to
/// exhaust the stack spends two bytes per level and runs into this within a kilobyte.
pub(crate) const MAXIMUM_BLOCK_MARKERS: usize = 512;

/// What a cut changelog ends with, so a truncated document never reads as complete.
pub(crate) const TRUNCATION_MARKER: &str = "\n\n…";

/// The Markdown extensions that keep a release manifest inert.
///
/// Built once. `MarkdownExtensions` stamps every registration with a process-wide
/// revision, so rebuilding this per frame would hand `TextView` a new configuration on
/// every frame and leave it one change of that comparison away from re-parsing the whole
/// changelog sixty times a second. A single registry makes the configuration provably
/// stable instead.
pub(crate) fn extensions() -> MarkdownExtensions {
    static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
    EXTENSIONS
        .get_or_init(|| {
            MarkdownExtensions::default()
                // A fixed parser revision as well, so the registry reads as the same
                // parser even if a future `gpui-kit` starts comparing it.
                .parser_revision(PARSER_REVISION)
                .plugin(NoRemoteImage)
                .plugin(RefusedLink)
                .plugin(LiteralHtml::<false>)
                .plugin(LiteralHtml::<true>)
        })
        .clone()
}

// --- The input bounds, applied before anything parses it ---

/// The changelog, bounded, ready for the parser.
///
/// Both bounds are applied to the raw text. The parser builds its tree with an explicit
/// stack, so it is not the parsing that has to be bounded —it is the shape of the tree
/// that comes out, because the renderer walks that by recursion.
pub(crate) fn prepare(notes: &str) -> String {
    let Some(cut) = cut_offset(notes) else {
        return notes.to_owned();
    };
    let mut boundary = cut;
    while boundary > 0 && !notes.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}{TRUNCATION_MARKER}", &notes[..boundary])
}

/// Where the notes have to stop being rendered, if anywhere.
pub(crate) fn cut_offset(notes: &str) -> Option<usize> {
    let bytes = notes.as_bytes();
    let mut markers = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if index >= MAXIMUM_MARKDOWN_BYTES {
            return Some(index);
        }
        if *byte == b'>' || is_list_marker_start(bytes, index) {
            markers += 1;
            if markers > MAXIMUM_BLOCK_MARKERS {
                return Some(index);
            }
        }
    }
    None
}

/// Whether a list marker starts at `index`.
///
/// Deliberately over-eager: a bullet counts wherever it follows whitespace or starts the
/// text, and so does a digit run closed by `.` or `)`. Over-counting only makes the bound
/// trip earlier, which is the direction a guard has to fail in —the alternative is a
/// document that nests deeper than its marker count suggests.
pub(crate) fn is_list_marker_start(bytes: &[u8], index: usize) -> bool {
    if index > 0 && !bytes[index - 1].is_ascii_whitespace() {
        return false;
    }
    if matches!(bytes[index], b'-' | b'*' | b'+') {
        return true;
    }
    let digits = bytes[index..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    digits > 0
        && bytes
            .get(index + digits)
            .is_some_and(|byte| matches!(byte, b'.' | b')'))
}
