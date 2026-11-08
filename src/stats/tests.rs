use super::*;

#[test]
fn first_template_name_reads_up_to_the_first_pipe() {
    assert_eq!(
        first_template_name("{{plural of|en|dog}}"),
        Some("plural of")
    );
}

#[test]
fn first_template_name_reads_up_to_the_closing_braces() {
    assert_eq!(first_template_name("{{...}} rest"), Some("..."));
}

#[test]
fn first_template_name_skips_leading_whitespace() {
    assert_eq!(first_template_name("  {{inflection of|en}}"), Some("inflection of"));
}

#[test]
fn first_template_name_trims_the_name() {
    assert_eq!(first_template_name("{{ plural of |en}}"), Some("plural of"));
}

#[test]
fn first_template_name_is_none_for_plain_text() {
    assert_eq!(first_template_name("a plain definition"), None);
}

#[test]
fn first_template_name_is_none_when_a_template_is_not_first() {
    assert_eq!(first_template_name("text {{plural of|en|dog}}"), None);
}

#[test]
fn first_template_name_handles_an_unterminated_template() {
    assert_eq!(first_template_name("{{plural of"), Some("plural of"));
}

#[test]
fn first_template_name_is_none_for_empty_input() {
    assert_eq!(first_template_name(""), None);
}

#[test]
fn is_form_of_accepts_plural_of() {
    assert!(is_form_of("{{plural of|en|dog}}"));
}

#[test]
fn is_form_of_accepts_inflection_of() {
    assert!(is_form_of("{{inflection of|en|run||3|s|pres}}"));
}

#[test]
fn is_form_of_accepts_infl_of() {
    assert!(is_form_of("{{infl of|en|run||ing}}"));
}

#[test]
fn is_form_of_is_case_insensitive() {
    assert!(is_form_of("{{Plural Of|en|dog}}"));
}

#[test]
fn is_form_of_rejects_a_label_template() {
    assert!(!is_form_of("{{lb|en|informal}} meaning"));
}

#[test]
fn is_form_of_rejects_plain_text() {
    assert!(!is_form_of("a domesticated canine"));
}

#[test]
fn is_form_of_rejects_a_name_that_only_contains_of() {
    assert!(!is_form_of("{{offer|en}}"));
}

#[test]
fn is_form_of_rejects_a_form_of_template_that_is_not_first() {
    assert!(!is_form_of("meaning {{plural of|en|dog}}"));
}

#[test]
fn is_form_of_rejects_empty_input() {
    assert!(!is_form_of(""));
}

#[test]
fn csv_field_wraps_in_quotes() {
    assert_eq!(csv_field("dog"), "\"dog\"");
}

#[test]
fn csv_field_doubles_embedded_quotes() {
    assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
}

#[test]
fn csv_field_keeps_commas_inside_the_quotes() {
    assert_eq!(csv_field("a,b"), "\"a,b\"");
}

#[test]
fn csv_field_quotes_an_empty_value() {
    assert_eq!(csv_field(""), "\"\"");
}

#[test]
fn now_millis_is_after_the_year_2020() {
    assert!(now_millis() > 1_577_836_800_000);
}

#[test]
fn stopwatch_reports_the_same_elapsed_time_after_the_first_stop() {
    let mut watch = Stopwatch::start();
    let first = watch.stop();

    assert_eq!(watch.stop(), first);
}
