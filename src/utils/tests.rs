use super::*;

#[test]
fn trim_to_string_removes_leading_and_trailing_whitespace() {
    assert_eq!(trim_to_string("  hello \n"), "hello");
}

#[test]
fn trim_to_string_keeps_inner_whitespace() {
    assert_eq!(trim_to_string(" a  b "), "a  b");
}

#[test]
fn trim_to_string_of_blank_text_is_empty() {
    assert_eq!(trim_to_string(" \t\n "), "");
}

#[test]
fn trim_to_string_of_empty_text_is_empty() {
    assert_eq!(trim_to_string(""), "");
}

#[test]
fn trim_to_string_handles_non_ascii_whitespace() {
    assert_eq!(trim_to_string("\u{00a0}héllo\u{2003}"), "héllo");
}
