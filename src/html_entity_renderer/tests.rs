use super::*;

fn render(input: &str) -> String {
    SimpleHtmlEntityRenderer::default().render(input)
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

#[test]
fn named_entities_are_decoded() {
    let cases = [
        ("&nbsp;", " "),
        ("&emsp;", " "),
        ("&thinsp;", " "),
        ("&rarr;", "→"),
        ("&mdash;", "—"),
        ("&amp;", "&"),
        ("&ndash;", "–"),
        ("&quot;", "\""),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&hellip;", "…"),
        ("&deg;", "°"),
        ("&minus;", "-"),
        ("&equals;", "="),
        ("&times;", "×"),
        ("&Delta;", "Δ"),
    ];

    for (input, expected) in cases {
        assert_eq!(render(input), expected, "decoding {input}");
    }
}

#[test]
fn decimal_numeric_entities_are_decoded() {
    let cases = [("&#39;", "'"), ("&#65;", "A"), ("&#95;", "_"), ("&#8212;", "—")];

    for (input, expected) in cases {
        assert_eq!(render(input), expected, "decoding {input}");
    }
}

#[test]
fn hexadecimal_numeric_entities_are_decoded() {
    let cases = [
        ("&#x27;", "'"),
        ("&#X27;", "'"),
        ("&#x2014;", "—"),
        ("&#x1F600;", "😀"),
        ("&#xe9;", "é"),
    ];

    for (input, expected) in cases {
        assert_eq!(render(input), expected, "decoding {input}");
    }
}

#[test]
fn numeric_entity_for_null_is_removed() {
    assert_eq!(render("a&#0;b"), "ab");
}

#[test]
fn numeric_entity_beyond_unicode_range_is_removed() {
    assert_eq!(render("a&#1114112;b"), "ab");
}

#[test]
fn numeric_entity_for_surrogate_is_removed() {
    assert_eq!(render("a&#xD800;b"), "ab");
}

#[test]
fn unknown_named_entities_are_left_as_written() {
    assert_eq!(render("&foo;"), "&foo;");
    assert_eq!(render("caf&Eacute;"), "caf&Eacute;");
    assert_eq!(render("&Aacute;"), "&Aacute;");
}

#[test]
fn ampersand_that_is_not_an_entity_is_kept() {
    let cases = [
        "AT&T",
        "a & b",
        "&",
        "&;",
        "&#;",
        "&amp",
        "&#xZZ;",
        "100% & more",
        "&1;",
    ];

    for input in cases {
        assert_eq!(render(input), input, "leaving {input} alone");
    }
}

#[test]
fn adjacent_entities_are_all_decoded() {
    assert_eq!(render("&lt;b&gt;bold&lt;/b&gt;"), "<b>bold</b>");
}

#[test]
fn entities_are_not_decoded_twice() {
    assert_eq!(render("&amp;lt;"), "&lt;");
}

#[test]
fn text_around_entities_is_preserved() {
    assert_eq!(render("one&nbsp;two &amp; three"), "one two & three");
}

#[test]
fn plain_and_non_ascii_text_is_unchanged() {
    assert_eq!(render("héllo → wörld"), "héllo → wörld");
}

#[test]
fn empty_text_renders_empty() {
    assert_eq!(render(""), "");
}

#[test]
fn renderer_is_usable_as_a_trait_object() {
    let renderer: Box<dyn HtmlEntityRenderer> = Box::new(SimpleHtmlEntityRenderer::default());

    assert_eq!(renderer.render("a&amp;b"), "a&b");
}

#[test]
fn is_html_entity_requires_ampersand_followed_by_letter_or_hash() {
    assert!(is_html_entity(&chars("&a"), 0));
    assert!(is_html_entity(&chars("&#"), 0));
    assert!(is_html_entity(&chars("x&b"), 1));
    assert!(!is_html_entity(&chars("&1"), 0));
    assert!(!is_html_entity(&chars("&"), 0));
    assert!(!is_html_entity(&chars("a&b"), 0));
}

#[test]
fn is_html_entity_is_false_past_the_end() {
    assert!(!is_html_entity(&chars("&a"), 5));
    assert!(!is_html_entity(&chars(""), 0));
}

#[test]
fn parse_u32_reads_decimal_and_hexadecimal() {
    assert_eq!(parse_u32("65", false), Some(65));
    assert_eq!(parse_u32("41", true), Some(65));
    assert_eq!(parse_u32("ff", true), Some(255));
}

#[test]
fn parse_u32_rejects_invalid_input() {
    assert_eq!(parse_u32("zz", false), None);
    assert_eq!(parse_u32("", false), None);
    assert_eq!(parse_u32("1g", true), None);
    assert_eq!(parse_u32("99999999999", false), None);
}

#[test]
fn extract_html_entity_reports_where_the_entity_ends() {
    let text = chars("&amp;rest");
    let (new_index, entity) = extract_html_entity(&text, 0);

    assert_eq!(new_index, 5);
    assert!(matches!(entity, Some(HtmlEntity::Named(name)) if name == "&amp;"));
}

#[test]
fn extract_html_entity_skips_one_character_when_invalid() {
    let text = chars("&amp rest");
    let (new_index, entity) = extract_html_entity(&text, 0);

    assert_eq!(new_index, 1);
    assert!(entity.is_none());
}
