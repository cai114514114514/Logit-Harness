use loh::cli::terminal::Input;

#[test]
fn a_wide_character_wrapped_at_the_margin_has_a_row_start_cursor() {
    for prefix in ["abc".to_owned(), "a".repeat(33)] {
        let width = prefix.len() + 1;
        let mut input = Input::default();
        input.insert(&format!("{prefix}中def"));
        input.home();
        input.visual_down(width);
        assert_eq!(input.position(width), (0, 1));
        input.insert("X");
        assert_eq!(input.text(), format!("{prefix}X中def"));
    }
}

#[test]
fn joining_lines_keeps_the_cursor_outside_combining_graphemes() {
    let mut input = Input::default();
    input.insert("a\n\u{301}b");
    input.home();
    input.right();
    input.delete();
    assert_eq!(input.text(), "a\u{301}b");
    assert_eq!(input.position(10), (1, 0));
    input.backspace();
    assert_eq!(input.text(), "b");
}

#[test]
fn wrapped_arrows_edit_the_draft_before_browsing_history() {
    let mut input = Input::default();
    input.remember("older request");
    input.insert("abcdefghijklmnopqrstuvwx中文0123456789");
    assert_eq!(input.position(34), (4, 1));
    input.visual_up(34);
    input.insert("X");
    assert_eq!(input.text(), "abcdXefghijklmnopqrstuvwx中文0123456789");
    input.visual_down(34);
    input.insert("Y");
    assert!(input.text().ends_with("9Y"));
}

#[test]
fn history_restores_the_unsent_cursor_and_word_deletion_keeps_graphemes() {
    let mut input = Input::default();
    input.remember("prior");
    input.insert("abc 👩‍💻 tail");
    input.home();
    input.right();
    input.up();
    input.down();
    input.insert("X");
    assert_eq!(input.text(), "aXbc 👩‍💻 tail");
    input.end();
    input.delete_word();
    input.delete_word();
    assert_eq!(input.text(), "aXbc ");
    input.insert("e\u{301}");
    input.delete_word();
    assert_eq!(input.text(), "aXbc ");
}

#[test]
fn line_editing_keeps_adjacent_lines_and_can_join_at_the_boundary() {
    let mut input = Input::default();
    input.insert("first\nsecond\nthird");
    input.home();
    input.line_end();
    input.kill_line();
    assert_eq!(input.text(), "firstsecond\nthird");
    input.line_end();
    input.insert("X");
    assert_eq!(input.text(), "firstsecondX\nthird");
    input.end();
    input.line_home();
    input.kill_line();
    assert_eq!(input.text(), "firstsecondX\n");
}
