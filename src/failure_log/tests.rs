use super::*;

fn failure(word: &str, number: usize, error: &str, markup: &str) -> FailedDefinition {
    FailedDefinition {
        word: word.to_string(),
        definition_number: number,
        kind: FailureKind::Error,
        error: error.to_string(),
        markup: markup.to_string(),
    }
}

fn written(log: &FailureLog, tag: &str) -> String {
    let mut buffer = Vec::new();
    log.write_tsv(&mut buffer, tag).unwrap_or_default();
    String::from_utf8_lossy(&buffer).to_string()
}

fn markdown(log: &FailureLog, tag: &str) -> String {
    let mut buffer = Vec::new();
    log.write_markdown(&mut buffer, tag).unwrap_or_default();
    String::from_utf8_lossy(&buffer).to_string()
}

fn logged(word: &str, kind: FailureKind, tag: &str, markup: &str) -> LoggedFailure {
    LoggedFailure {
        failure: FailedDefinition {
            word: word.to_string(),
            definition_number: 0,
            kind,
            error: "boom".to_string(),
            markup: markup.to_string(),
        },
        hash: markup_hash(markup),
        setup_tag: tag.to_string(),
    }
}

#[test]
fn markup_hash_of_empty_text_is_the_fnv_offset_basis() {
    assert_eq!(markup_hash(""), "cbf29ce484222325");
}

#[test]
fn markup_hash_matches_a_known_fnv_1a_value() {
    assert_eq!(markup_hash("a"), "af63dc4c8601ec8c");
}

#[test]
fn markup_hash_is_sixteen_hex_digits() {
    assert_eq!(markup_hash("{{lb|en|x}} text").len(), 16);
}

#[test]
fn markup_hash_differs_for_different_markup() {
    assert_ne!(markup_hash("one"), markup_hash("two"));
}

#[test]
fn markup_hash_is_the_same_for_the_same_markup() {
    assert_eq!(markup_hash("same"), markup_hash("same"));
}

#[test]
fn signature_collapses_double_quoted_text() {
    assert_eq!(
        signature(r#"Parameter "The derivative of x" is not used"#),
        r#"Parameter "..." is not used"#
    );
}

#[test]
fn signature_collapses_single_quoted_text() {
    assert_eq!(
        signature("attempt to index upvalue 'wb' (a nil value)"),
        "attempt to index upvalue '...' (a nil value)"
    );
}

#[test]
fn signature_keeps_text_without_quotes() {
    assert_eq!(signature("Invalid ISBN"), "Invalid ISBN");
}

#[test]
fn signature_is_limited_to_one_hundred_ten_characters() {
    assert_eq!(signature(&"x".repeat(300)).chars().count(), 110);
}

#[test]
fn signature_of_an_unclosed_quote_drops_the_rest() {
    assert_eq!(signature("a \"b c"), "a \"...");
}

#[test]
fn escape_and_unescape_round_trip_special_characters() {
    let text = "a\tb\nc\rd\\e";

    assert_eq!(unescape_field(&escape_field(text)), text);
}

#[test]
fn escape_field_leaves_no_raw_tab_or_newline() {
    let escaped = escape_field("a\tb\nc");

    assert!(!escaped.contains('\t'));
    assert!(!escaped.contains('\n'));
}

#[test]
fn unescape_field_keeps_a_trailing_backslash() {
    assert_eq!(unescape_field("abc\\"), "abc\\");
}

#[test]
fn write_tsv_starts_with_the_header() {
    let log = FailureLog::default();

    assert_eq!(
        written(&log, "setup"),
        "word\tdefinition_number\tkind\thash\tsetup_tag\terror\tmarkup\n"
    );
}

#[test]
fn write_tsv_writes_one_row_per_failure() {
    let mut log = FailureLog::default();
    log.record(failure("cat", 1, "bad", "{{x"));

    assert_eq!(
        written(&log, "setup"),
        format!(
            "word\tdefinition_number\tkind\thash\tsetup_tag\terror\tmarkup\ncat\t1\terror\t{}\tsetup\tbad\t{{{{x\n",
            markup_hash("{{x")
        )
    );
}

#[test]
fn write_tsv_orders_rows_by_word_then_definition_number() {
    let mut log = FailureLog::default();
    log.record(failure("b", 0, "e", "m"));
    log.record(failure("a", 2, "e", "m"));
    log.record(failure("a", 1, "e", "m"));

    let rows = written(&log, "t")
        .lines()
        .skip(1)
        .map(|line| line.split('\t').take(2).collect::<Vec<_>>().join(":"))
        .collect::<Vec<_>>();

    assert_eq!(rows, vec!["a:1", "a:2", "b:0"]);
}

#[test]
fn write_tsv_marks_a_transient_failure() {
    let mut log = FailureLog::default();
    log.record(FailedDefinition {
        kind: FailureKind::Transient,
        ..failure("cat", 0, "timeout", "m")
    });

    assert!(written(&log, "t").contains("\ttransient\t"));
}

#[test]
fn read_tsv_round_trips_what_write_tsv_wrote() {
    let mut log = FailureLog::default();
    log.record(failure(
        "say\thi",
        3,
        "line one\nline two",
        "{{a|b}}\ttext\\",
    ));
    log.record(FailedDefinition {
        kind: FailureKind::Transient,
        ..failure("dog", 0, "timeout", "plain")
    });

    let entries = read_tsv(&written(&log, "tag one"));

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].failure.word, "dog");
    assert_eq!(entries[0].failure.kind, FailureKind::Transient);
    assert_eq!(entries[1].failure.word, "say\thi");
    assert_eq!(entries[1].failure.definition_number, 3);
    assert_eq!(entries[1].failure.error, "line one\nline two");
    assert_eq!(entries[1].failure.markup, "{{a|b}}\ttext\\");
    assert_eq!(entries[1].setup_tag, "tag one");
    assert_eq!(entries[1].hash, markup_hash("{{a|b}}\ttext\\"));
}

#[test]
fn read_tsv_skips_the_header() {
    assert!(read_tsv("word\tdefinition_number\tkind\thash\tsetup_tag\terror\tmarkup\n").is_empty());
}

#[test]
fn read_tsv_skips_malformed_rows() {
    let text = "header\nonly\tthree\tfields\ncat\tnotanumber\terror\th\tt\te\tm\ncat\t1\tweird\th\tt\te\tm\ncat\t2\terror\th\tt\te\tm\n";

    let entries = read_tsv(text);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].failure.definition_number, 2);
}

#[test]
fn read_tsv_of_empty_text_is_empty() {
    assert!(read_tsv("").is_empty());
}

#[test]
fn split_for_retry_retries_entries_from_a_different_setup() {
    let entries = vec![
        logged("a", FailureKind::Error, "old", "m1"),
        logged("b", FailureKind::Error, "new", "m2"),
    ];

    let (retry, keep) = split_for_retry(entries, "new");

    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].failure.word, "a");
    assert_eq!(keep.len(), 1);
    assert_eq!(keep[0].failure.word, "b");
}

#[test]
fn split_for_retry_always_retries_transient_entries() {
    let entries = vec![logged("a", FailureKind::Transient, "new", "m")];

    let (retry, keep) = split_for_retry(entries, "new");

    assert_eq!(retry.len(), 1);
    assert!(keep.is_empty());
}

#[test]
fn split_for_retry_keeps_errors_from_the_current_setup() {
    let entries = vec![logged("a", FailureKind::Error, "new", "m")];

    let (retry, keep) = split_for_retry(entries, "new");

    assert!(retry.is_empty());
    assert_eq!(keep.len(), 1);
}

#[test]
fn split_for_retry_of_nothing_is_empty() {
    let (retry, keep) = split_for_retry(Vec::new(), "new");

    assert!(retry.is_empty());
    assert!(keep.is_empty());
}

#[test]
fn known_failures_maps_hash_to_error_for_the_current_setup() {
    let entries = vec![
        logged("a", FailureKind::Error, "new", "m1"),
        logged("b", FailureKind::Error, "old", "m2"),
        logged("c", FailureKind::Transient, "new", "m3"),
    ];

    let known = known_failures(&entries, "new");

    assert_eq!(known.len(), 1);
    assert_eq!(known.get(&markup_hash("m1")), Some(&"boom".to_string()));
}

#[test]
fn write_markdown_lists_the_total_and_the_setup_tag() {
    let mut log = FailureLog::default();
    log.record(failure("cat", 0, "bad", "m"));

    let text = markdown(&log, "my-setup");

    assert!(text.contains("Server setup tag: `my-setup`."));
    assert!(text.contains("Total: 1"));
}

#[test]
fn write_markdown_counts_failures_by_error_signature() {
    let mut log = FailureLog::default();
    log.record(failure("a", 0, "Parameter \"x\" is not used", "m"));
    log.record(failure("b", 0, "Parameter \"y\" is not used", "m"));
    log.record(failure("c", 0, "Invalid ISBN", "m"));

    let text = markdown(&log, "t");

    assert!(text.contains("| Parameter \"...\" is not used | 2 |"));
    assert!(text.contains("| Invalid ISBN | 1 |"));
}

#[test]
fn write_markdown_lists_each_definition_in_a_row() {
    let mut log = FailureLog::default();
    log.record(failure("cat", 4, "bad", "{{lb|en|x}} a"));

    assert!(markdown(&log, "t").contains("| cat | 4 | error | bad | {{lb&#124;en&#124;x}} a |"));
}

#[test]
fn write_markdown_escapes_html_in_cells() {
    let mut log = FailureLog::default();
    log.record(failure("w", 0, "a <b> & c", "<score>x</score>"));

    let text = markdown(&log, "t");

    assert!(text.contains("a &lt;b&gt; &amp; c"));
    assert!(text.contains("&lt;score&gt;x&lt;/score&gt;"));
}

#[test]
fn write_markdown_shortens_long_markup() {
    let mut log = FailureLog::default();
    log.record(failure("w", 0, "e", &"x".repeat(300)));

    let text = markdown(&log, "t");

    assert!(text.contains(&format!("{}...", "x".repeat(140))));
    assert!(!text.contains(&"x".repeat(141)));
}

#[test]
fn write_markdown_puts_newlines_in_cells_on_one_line() {
    let mut log = FailureLog::default();
    log.record(failure("w", 0, "e", "a\nb"));

    assert!(markdown(&log, "t").contains("| a b |"));
}

#[test]
fn write_markdown_of_an_empty_log_has_a_zero_total() {
    assert!(markdown(&FailureLog::default(), "t").contains("Total: 0"));
}

#[test]
fn len_and_is_empty_follow_the_recorded_failures() {
    let mut log = FailureLog::default();
    assert!(log.is_empty());

    log.record(failure("a", 0, "e", "m"));

    assert_eq!(log.len(), 1);
    assert!(!log.is_empty());
}
