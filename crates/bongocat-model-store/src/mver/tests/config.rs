//! Reading the legacy key table the way the legacy application reads it.

use super::*;

/// A key table a model author annotated, in the shape a real paid model ships.
///
/// The comments sit between array elements, which is where a hand-written key
/// table is annotated: `l2d_motion_lockhand` and `sounds` are lists of pairs and
/// only their first column is meaningless to a reader. The legacy application
/// reads this file with JsonCpp's `CharReaderBuilder`, whose defaults set
/// `allowComments`, so these comments are part of the format rather than damage
/// to it.
///
/// This is the regression the whole file exists for: a strict reader refuses
/// the file, detection is speculative and swallows the refusal, and the folder
/// is then reported as an invalid *package* — the model looks unimportable for
/// a reason that has nothing to do with the model.
#[test]
fn an_annotated_key_table_is_still_a_legacy_key_table() {
    let root = tempdir().expect("root");
    legacy_source(root.path(), &all_modes(), true);
    write(
        root.path(),
        LEGACY_CONFIG_FILE,
        r#"{
	//键鼠模式下的表情快捷键
	"standard" : {
		"hand" : [
			[ 9 ],   //Tab
			[ 65 ]  //A
		],
		"keyboard" : [ [ 17 ], [ 16 ] ]
	},
	"keyboard" : {
		"lefthand" : [ [ 9 ] /* tab */ ],
		"righthand" : [ [ 8 ] ],
		"keyboard" : [ [ 9 ], [ 8 ] ]
	},
	"gamepad" : {
		"lefthand" : [ [ 4 ] ],
		"righthand" : [ [ 0 ] ],
		"keyboard" : [ [ 4 ], [ 0 ] ]
	}
}"#
        .as_bytes(),
    );

    let plan = inspect_directory(root.path()).expect("legacy plan");
    assert_eq!(
        plan.modes().collect::<Vec<_>>(),
        MverInputMode::ALL.to_vec()
    );

    // The comments are gone, not the table: the standard mode still binds Tab
    // to its first hand image and A to its second.
    assert_eq!(
        plan_mode(&plan, MverInputMode::Standard)
            .slots
            .iter()
            .map(|slot| slot.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["left-keys/Tab.png", "left-keys/KeyA.png"]
    );
    // A block comment is as ignorable as a line comment.
    assert_eq!(
        plan_mode(&plan, MverInputMode::Keyboard)
            .slots
            .iter()
            .map(|slot| slot.reference.as_str())
            .collect::<Vec<_>>(),
        vec!["left-keys/Tab.png", "right-keys/Backspace.png"]
    );
}

/// The comment scan is about *where* a `/` is, not about the text around it.
///
/// A `/` inside a string is data — a URL, a Windows path, a model name with a
/// slash in it — and a quoted `"` can be escaped, so a scanner that only tracks
/// "am I inside a string" naively would end the literal early and then mangle
/// everything after it.
#[test]
fn comments_are_removed_outside_strings_and_nowhere_else() {
    assert_eq!(
        without_json_comments(r#"{"send_ip":"192.168.0.1","url":"https://example.test/a//b"}"#),
        r#"{"send_ip":"192.168.0.1","url":"https://example.test/a//b"}"#
    );
    // An escaped quote does not end the literal, so the comment after it is
    // still inside a string and stays.
    assert_eq!(
        without_json_comments(r#"{"note":"a \" // not a comment","b":1}"#),
        r#"{"note":"a \" // not a comment","b":1}"#
    );
    // A comment whose last line has no break in it is still a comment.
    assert_eq!(without_json_comments("{\"a\":1} // trailing"), "{\"a\":1} ");
    // A block comment keeps its line breaks, so a file that still does not parse
    // fails near the line its author wrote rather than at the top.
    assert_eq!(
        without_json_comments("{\n/* one\n   two */\n\"a\":1\n}"),
        "{\n\n\n\"a\":1\n}"
    );
    // A file that ends inside an unterminated comment loses the comment, not
    // the whole document: the truncation is a consequence of the source.
    assert_eq!(without_json_comments("{\"a\":1} /* open"), "{\"a\":1} ");
}

/// Comment removal is not allowed to damage anything that is not a comment.
///
/// Non-ASCII text is the everyday case here: the annotations these files carry
/// are written in the model author's own language, and a byte-wise scan that
/// split a multi-byte character would turn a readable key table into invalid
/// UTF-8 — which detection would again report as an ordinary package.
#[test]
fn comment_removal_leaves_multibyte_text_intact() {
    let text = "{\n  // 默认表情\n  \"standard\": { \"hand\": [[9]] }\n}";
    assert_eq!(
        without_json_comments(text),
        "{\n  \n  \"standard\": { \"hand\": [[9]] }\n}"
    );
    let config = parse_legacy_config(text.as_bytes()).expect("annotated config");
    assert_eq!(
        config.standard.expect("standard section").hand,
        vec![vec![9]]
    );
}

/// The scan is a strict superset of "valid JSON": it must not make a file that
/// is not a config parse into one.
#[test]
fn a_config_that_is_not_json_is_still_not_a_config() {
    assert!(parse_legacy_config(b"not json").is_none());
    // Bytes that are not UTF-8 are not text at all, which is what the strict
    // reader said about them before.
    assert!(parse_legacy_config(&[0xff, 0xfe, 0x00]).is_none());
}
