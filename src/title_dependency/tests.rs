use super::*;

fn dependency() -> SimpleTitleDependency {
    SimpleTitleDependency::from_lines(
        "Template:see more citations\nTemplate:&lit\nTemplate:RQ:Plato Tredennick Socrates\nTemplate:with space\nModule:senseno\nModule:definition/templates\n",
    )
}

#[test]
fn from_lines_reads_templates_and_modules() {
    let dependency = SimpleTitleDependency::from_lines("Template:a\nModule:b\n");

    assert!(dependency.templates.contains("a"));
    assert!(dependency.modules.contains("b"));
}

#[test]
fn from_lines_ignores_other_lines() {
    let dependency = SimpleTitleDependency::from_lines("junk\n\nTemplate:a\n");

    assert_eq!(dependency.templates.len(), 1);
    assert!(dependency.modules.is_empty());
}

#[test]
fn from_lines_keeps_colons_inside_template_names() {
    let dependency = SimpleTitleDependency::from_lines("Template:RQ:Plato\n");

    assert!(dependency.templates.contains("RQ:Plato"));
}

#[test]
fn plain_text_does_not_require_title() {
    assert!(!dependency().requires_title("a plain definition"));
}

#[test]
fn empty_markup_does_not_require_title() {
    assert!(!dependency().requires_title(""));
}

#[test]
fn flagged_template_with_arguments_requires_title() {
    assert!(dependency().requires_title("{{see more citations|en}}"));
}

#[test]
fn flagged_template_without_arguments_requires_title() {
    assert!(dependency().requires_title("{{&lit}}"));
}

#[test]
fn unflagged_template_does_not_require_title() {
    assert!(!dependency().requires_title("{{lb|en|informal}} a thing"));
}

#[test]
fn flagged_template_after_other_text_requires_title() {
    assert!(dependency().requires_title("{{lb|en|informal}} a thing. {{see more citations|en}}"));
}

#[test]
fn flagged_template_nested_in_arguments_requires_title() {
    assert!(dependency().requires_title("{{lb|en|{{&lit|en|x}}}}"));
}

#[test]
fn template_names_are_case_sensitive() {
    assert!(!dependency().requires_title("{{See more citations|en}}"));
}

#[test]
fn underscores_in_names_match_spaces() {
    assert!(dependency().requires_title("{{see_more_citations|en}}"));
}

#[test]
fn whitespace_around_names_is_ignored() {
    assert!(dependency().requires_title("{{  see more citations  |en}}"));
}

#[test]
fn spaces_inside_names_are_kept() {
    assert!(dependency().requires_title("{{with space|x}}"));
}

#[test]
fn template_namespace_prefix_is_ignored() {
    assert!(dependency().requires_title("{{Template:see more citations|en}}"));
}

#[test]
fn template_namespace_prefix_is_case_insensitive() {
    assert!(dependency().requires_title("{{template:see more citations|en}}"));
}

#[test]
fn leading_colon_is_ignored() {
    assert!(dependency().requires_title("{{:see more citations|en}}"));
}

#[test]
fn names_containing_colons_match() {
    assert!(dependency().requires_title("{{RQ:Plato Tredennick Socrates|passage=x}}"));
}

#[test]
fn magic_word_requires_title() {
    assert!(dependency().requires_title("see {{PAGENAME}}"));
}

#[test]
fn magic_word_with_spaces_requires_title() {
    assert!(dependency().requires_title("see {{ FULLPAGENAME }}"));
}

#[test]
fn magic_word_with_subst_prefix_requires_title() {
    assert!(dependency().requires_title("{{safesubst:PAGENAME}}"));
    assert!(dependency().requires_title("{{subst:NAMESPACE}}"));
}

#[test]
fn magic_word_with_an_argument_does_not_require_title() {
    assert!(!dependency().requires_title("{{PAGENAME:Some page}}"));
}

#[test]
fn lowercase_magic_word_does_not_require_title() {
    assert!(!dependency().requires_title("{{pagename}}"));
}

#[test]
fn magic_word_followed_by_a_pipe_does_not_require_title() {
    assert!(!dependency().requires_title("{{PAGENAME|x}}"));
}

#[test]
fn flagged_invoked_module_requires_title() {
    assert!(dependency().requires_title("{{#invoke:senseno|link}}"));
}

#[test]
fn invoked_module_with_a_slash_requires_title() {
    assert!(dependency().requires_title("{{#invoke:definition/templates|and_lit_t}}"));
}

#[test]
fn invoke_is_case_insensitive() {
    assert!(dependency().requires_title("{{#INVOKE: senseno |link}}"));
}

#[test]
fn invoke_accepts_the_module_prefix() {
    assert!(dependency().requires_title("{{#invoke:Module:senseno|link}}"));
}

#[test]
fn unflagged_invoked_module_does_not_require_title() {
    assert!(!dependency().requires_title("{{#invoke:labels|show}}"));
}

#[test]
fn unterminated_invoke_does_not_require_title() {
    assert!(!dependency().requires_title("{{#invoke:senseno"));
}

#[test]
fn parser_function_is_skipped_but_its_contents_are_checked() {
    assert!(dependency().requires_title("{{#if:x|{{see more citations|en}}}}"));
    assert!(!dependency().requires_title("{{#if:x|{{lb|en|y}}}}"));
}

#[test]
fn dynamic_template_name_is_treated_as_requiring_title() {
    assert!(dependency().requires_title("{{ {{#if:x|a|b}} |lang=en}}"));
}

#[test]
fn template_parameter_is_treated_as_requiring_title() {
    assert!(dependency().requires_title("{{{1}}}"));
}

#[test]
fn unterminated_template_does_not_require_title() {
    assert!(!dependency().requires_title("{{see more citations"));
}

#[test]
fn name_followed_by_a_link_does_not_match() {
    assert!(!dependency().requires_title("{{see more citations[[x]]}}"));
}

#[test]
fn empty_dependency_requires_nothing() {
    assert!(!SimpleTitleDependency::default().requires_title("{{see more citations|en}}"));
}

#[test]
fn flagged_invocations_returns_the_name_and_full_text() {
    let invocations = dependency().flagged_invocations("a {{see more citations|en}} b");

    assert_eq!(
        invocations,
        vec![Invocation {
            template: "see more citations".to_string(),
            text: "{{see more citations|en}}".to_string()
        }]
    );
}

#[test]
fn flagged_invocations_includes_nested_templates_in_the_outer_text() {
    let invocations = dependency().flagged_invocations("{{&lit|en|{{see more citations|en}}}}");

    assert_eq!(invocations.len(), 2);
    assert_eq!(invocations[0].template, "&lit");
    assert_eq!(invocations[0].text, "{{&lit|en|{{see more citations|en}}}}");
    assert_eq!(invocations[1].text, "{{see more citations|en}}");
}

#[test]
fn flagged_invocations_skips_unflagged_templates() {
    assert!(dependency().flagged_invocations("{{lb|en|x}}").is_empty());
}

#[test]
fn flagged_invocations_skips_unbalanced_templates() {
    assert!(
        dependency()
            .flagged_invocations("{{&lit|en|{{x}}")
            .is_empty()
    );
}

#[test]
fn flagged_invocations_normalizes_the_name() {
    let invocations = dependency().flagged_invocations("{{Template:see_more_citations|en}}");

    assert_eq!(invocations[0].template, "see more citations");
}

#[test]
fn flagged_invocations_of_plain_text_is_empty() {
    assert!(dependency().flagged_invocations("plain").is_empty());
}

#[test]
fn balanced_end_finds_the_matching_close() {
    let text = "{{a|{{b}}}} tail";

    assert_eq!(balanced_end(text, 0), Some(11));
}

#[test]
fn balanced_end_of_an_unclosed_template_is_none() {
    assert_eq!(balanced_end("{{a|{{b}}", 0), None);
}
