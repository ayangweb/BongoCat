//! What may reach the parser, and where the cut lands.

use super::*;

// --- The input bounds ---

#[test]
fn an_ordinary_changelog_is_not_cut() {
    let notes = "# BongoCat 2.0.0\n\n## Added\n\n- the overlay stays up\n- `just build` works\n\n> thanks\n";
    assert_eq!(prepare(notes), notes);
    assert_eq!(cut_offset(notes), None);
}

#[test]
fn an_oversized_changelog_is_cut_with_a_visible_marker() {
    let notes = "a".repeat(MAXIMUM_MARKDOWN_BYTES + 1);
    let prepared = prepare(&notes);
    assert!(prepared.ends_with(TRUNCATION_MARKER), "{prepared:?}");
    assert_eq!(
        prepared.len(),
        MAXIMUM_MARKDOWN_BYTES + TRUNCATION_MARKER.len(),
        "the cut has to land exactly on the byte budget"
    );
}

#[test]
fn a_cut_lands_on_a_character_boundary() {
    // Each `é` is two bytes, so a budget that lands mid-character would panic on a
    // slice if the boundary were not walked back.
    let notes = "é".repeat(MAXIMUM_MARKDOWN_BYTES);
    let prepared = prepare(&notes);
    assert!(prepared.ends_with(TRUNCATION_MARKER), "{prepared:?}");
    assert!(prepared.starts_with('é'));
}

/// Deeply nested quotes are the cheapest way to make a renderer recurse.
///
/// Two bytes buy one level, so a document that nests thousands deep fits in a
/// kilobyte. The bound is on the number of containers rather than on the apparent
/// depth, which is what makes it hold for a document written to look shallow.
#[test]
fn nesting_deeper_than_the_stack_allows_is_cut() {
    let notes = format!("{}deep\n", "> ".repeat(MAXIMUM_BLOCK_MARKERS + 1));
    let prepared = prepare(&notes);
    assert!(prepared.ends_with(TRUNCATION_MARKER), "{prepared:?}");
    assert!(
        prepared.len() < notes.len(),
        "a document this nested must be cut, not rendered"
    );
}

/// A real changelog has a long list in it, and none of that is nesting.
#[test]
fn a_long_list_is_not_mistaken_for_nesting() {
    let notes = (0..MAXIMUM_BLOCK_MARKERS)
        .map(|index| format!("- entry {index}\n"))
        .collect::<String>();
    assert_eq!(cut_offset(&notes), None);
}

#[test]
fn a_bullet_is_recognised_wherever_one_could_start() {
    let bytes = b"- a * b 1. c 2) d";
    let found: Vec<usize> = (0..bytes.len())
        .filter(|index| is_list_marker_start(bytes, *index))
        .collect();
    // `-`, `*`, `1.`, `2)`. A `-` or `*` mid-word is not a marker.
    assert_eq!(found, vec![0, 4, 8, 13]);
}
