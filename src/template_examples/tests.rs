use super::*;

fn dependency() -> SimpleTitleDependency {
    SimpleTitleDependency::from_lines("Template:lb\nTemplate:plural of\nTemplate:with space\n")
}

fn written(examples: &TemplateExamples) -> String {
    examples
        .jobs()
        .iter()
        .map(|(template, word, text)| format!("{template}\t{word}\t{text}\n"))
        .collect()
}

#[test]
fn default_keeps_two_examples_per_argument_shape() {
    let mut examples = TemplateExamples::default();

    for word in ["a", "b", "c", "d", "e"] {
        examples.add_markup(&dependency(), word, &format!("{{{{lb|en|{word}}}}}"));
    }

    assert_eq!(written(&examples).lines().count(), 2);
}

#[test]
fn default_keeps_examples_of_each_argument_shape() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{lb|en|x}}");
    examples.add_markup(&dependency(), "b", "{{lb|en|x|y}}");
    examples.add_markup(&dependency(), "c", "{{lb|en|dot=z}}");

    assert_eq!(written(&examples).lines().count(), 3);
}

#[test]
fn per_template_cap_limits_the_total() {
    let mut examples = TemplateExamples::new(1, 2);

    examples.add_markup(&dependency(), "a", "{{lb|en|x}}");
    examples.add_markup(&dependency(), "b", "{{lb|en|x|y}}");
    examples.add_markup(&dependency(), "c", "{{lb|en|x|y|z}}");

    assert_eq!(written(&examples).lines().count(), 2);
}

#[test]
fn argument_shape_counts_positional_arguments() {
    assert_eq!(argument_shape("{{lb|en|a|b}}"), "3|");
}

#[test]
fn argument_shape_lists_named_arguments_sorted() {
    assert_eq!(argument_shape("{{lb|en|year=1|author=x}}"), "1|author,year");
}

#[test]
fn argument_shape_ignores_the_template_name() {
    assert_eq!(argument_shape("{{x}}"), "0|");
}

#[test]
fn argument_shape_ignores_pipes_inside_nested_templates() {
    assert_eq!(argument_shape("{{lb|en|{{q|a|b}}}}"), "2|");
}

#[test]
fn argument_shape_ignores_pipes_inside_links() {
    assert_eq!(argument_shape("{{lb|en|[[a|b]]}}"), "2|");
}

#[test]
fn argument_shape_treats_an_equals_inside_a_nested_template_as_positional() {
    assert_eq!(argument_shape("{{lb|{{q|a=b}}}}"), "1|");
}

#[test]
fn argument_shape_deduplicates_repeated_names() {
    assert_eq!(argument_shape("{{lb|a=1|a=2}}"), "0|a");
}

#[test]
fn argument_shape_handles_non_ascii_text() {
    assert_eq!(argument_shape("{{lb|en|é|ü=ö}}"), "2|ü");
}

#[test]
fn keeps_the_first_examples_seen() {
    let mut examples = TemplateExamples::new(2, 12);

    for word in ["a", "b", "c"] {
        examples.add_markup(&dependency(), word, &format!("{{{{lb|en|{word}}}}}"));
    }

    assert_eq!(
        written(&examples),
        "lb\ta\t{{lb|en|a}}\nlb\tb\t{{lb|en|b}}\n"
    );
}

#[test]
fn ignores_unflagged_templates() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{q|en|x}} text");

    assert_eq!(examples.template_count(), 0);
    assert_eq!(written(&examples), "");
}

#[test]
fn takes_one_example_per_word_for_a_template() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{lb|en|x}} and {{lb|en|y}}");

    assert_eq!(written(&examples), "lb\ta\t{{lb|en|x}}\n");
}

#[test]
fn skips_duplicate_invocation_text_from_other_words() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{lb|en|x}}");
    examples.add_markup(&dependency(), "b", "{{lb|en|x}}");

    assert_eq!(written(&examples), "lb\ta\t{{lb|en|x}}\n");
}

#[test]
fn records_several_templates_from_one_definition() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{lb|en|x}} {{plural of|en|dog}}");

    assert_eq!(examples.template_count(), 2);
}

#[test]
fn records_nested_flagged_templates_separately() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{plural of|en|{{lb|en|x}}}}");

    assert_eq!(
        written(&examples),
        "lb\ta\t{{lb|en|x}}\nplural of\ta\t{{plural of|en|{{lb|en|x}}}}\n"
    );
}

#[test]
fn names_with_spaces_are_recorded_with_the_stripped_name() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{ with_space |x}}");

    assert_eq!(written(&examples), "with space\ta\t{{ with_space |x}}\n");
}

#[test]
fn jobs_are_ordered_by_template_name() {
    let mut examples = TemplateExamples::default();

    examples.add_markup(&dependency(), "a", "{{plural of|en|x}} {{lb|en|y}}");

    let lines = written(&examples);
    assert!(lines.starts_with("lb\t"));
}
