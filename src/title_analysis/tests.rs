use super::*;

fn analysis() -> TitleAnalysis {
    match TitleAnalysis::new() {
        Ok(analysis) => analysis,
        Err(err) => panic!("unexpected error: {err}"),
    }
}

#[allow(clippy::unnecessary_wraps)]
fn page(title: &str, markup: &str) -> Result<Page, DumpError> {
    Ok(Page {
        word: title.to_string(),
        markup: markup.to_string(),
    })
}

fn dependents(pages: Vec<Result<Page, DumpError>>) -> TitleDependents {
    analysis().static_dependents(pages.into_iter())
}

fn names(set: &BTreeSet<String>) -> Vec<&str> {
    set.iter().map(String::as_str).collect()
}

#[test]
fn a_module_calling_get_current_title_reads_the_title() {
    let found = dependents(vec![
        page("Module:uses", "local t = mw.title.getCurrentTitle()"),
        page("Module:plain", "return {}"),
    ]);

    assert_eq!(names(&found.modules), vec!["uses"]);
}

#[test]
fn a_module_using_a_title_magic_word_reads_the_title() {
    let found = dependents(vec![page("Module:magic", "local x = '{{PAGENAME}}'")]);

    assert_eq!(names(&found.modules), vec!["magic"]);
}

#[test]
fn a_template_using_a_title_magic_word_is_flagged() {
    let found = dependents(vec![
        page("Template:direct", "text {{PAGENAME}} text"),
        page("Template:plain", "text"),
    ]);

    assert_eq!(names(&found.templates), vec!["direct"]);
}

#[test]
fn magic_words_with_a_prefix_and_spacing_are_found() {
    let found = dependents(vec![page(
        "Template:spaced",
        "{{ safesubst:FULLPAGENAME }}",
    )]);

    assert_eq!(names(&found.templates), vec!["spaced"]);
}

#[test]
fn a_template_invoking_a_title_reading_module_is_flagged() {
    let found = dependents(vec![
        page("Module:uses", "mw.title.getCurrentTitle()"),
        page("Template:calls", "{{#invoke:uses|main}}"),
        page("Template:other", "{{#invoke:links|main}}"),
    ]);

    assert_eq!(names(&found.templates), vec!["calls"]);
}

#[test]
fn an_invoked_module_name_ignores_its_prefix_and_underscores() {
    let found = dependents(vec![
        page("Module:two words", "mw.title.getCurrentTitle()"),
        page("Template:calls", "{{#invoke:Module:two_words|main}}"),
    ]);

    assert_eq!(names(&found.templates), vec!["calls"]);
}

#[test]
fn a_template_calling_a_flagged_template_is_flagged() {
    let found = dependents(vec![
        page("Template:base", "{{PAGENAME}}"),
        page("Template:middle", "{{base|x}}"),
        page("Template:top", "{{Template:middle}}"),
        page("Template:unrelated", "{{elsewhere}}"),
    ]);

    assert_eq!(names(&found.templates), vec!["base", "middle", "top"]);
}

#[test]
fn a_template_name_with_underscores_matches_the_known_name() {
    let found = dependents(vec![
        page("Template:base name", "{{PAGENAME}}"),
        page("Template:caller", "{{base_name|x}}"),
    ]);

    assert_eq!(names(&found.templates), vec!["base name", "caller"]);
}

#[test]
fn a_template_called_through_a_dynamic_name_is_flagged() {
    let found = dependents(vec![
        page("Template:base", "{{PAGENAME}}"),
        page("Template:dynamic", "{{ {{base|en}}|x}}"),
    ]);

    assert_eq!(names(&found.templates), vec!["base", "dynamic"]);
}

#[test]
fn magic_words_inside_comments_are_ignored() {
    let found = dependents(vec![page(
        "Template:commented",
        "<!-- {{PAGENAME}} -->text",
    )]);

    assert!(found.templates.is_empty());
}

#[test]
fn magic_words_inside_noinclude_are_ignored() {
    let found = dependents(vec![page(
        "Template:documented",
        "text<noinclude>{{PAGENAME}}</noinclude>",
    )]);

    assert!(found.templates.is_empty());
}

#[test]
fn a_redirect_to_a_flagged_template_is_flagged() {
    let found = dependents(vec![
        page("Template:base", "{{PAGENAME}}"),
        page("Template:alias", "#REDIRECT [[Template:base]]"),
    ]);

    assert_eq!(names(&found.templates), vec!["alias", "base"]);
}

#[test]
fn a_redirect_outside_the_template_namespace_is_not_followed() {
    let found = dependents(vec![
        page("Template:base", "{{PAGENAME}}"),
        page("Template:alias", "#REDIRECT [[Module:base]]"),
    ]);

    assert_eq!(names(&found.templates), vec!["base"]);
}

#[test]
fn a_redirect_page_is_not_flagged_by_its_own_text() {
    let found = dependents(vec![page(
        "Template:alias",
        "#REDIRECT [[Template:missing]] {{PAGENAME}}",
    )]);

    assert!(found.templates.is_empty());
}

#[test]
fn pages_outside_the_template_and_module_namespaces_are_ignored() {
    let found = dependents(vec![
        page("word", "{{PAGENAME}}"),
        page("Category:x", "{{PAGENAME}}"),
    ]);

    assert_eq!(found, TitleDependents::default());
}

#[test]
fn the_pages_analysed_are_counted() {
    let found = dependents(vec![
        page("Template:a", "x"),
        page("Template:b", "x"),
        page("Module:c", "x"),
    ]);

    assert_eq!(found.templates_analysed, 2);
    assert_eq!(found.modules_analysed, 1);
}

#[test]
fn reading_stops_at_the_first_page_error() {
    let found = dependents(vec![
        page("Template:before", "{{PAGENAME}}"),
        Err(DumpError::IoError(std::io::Error::other("read failed"))),
        page("Template:after", "{{PAGENAME}}"),
    ]);

    assert_eq!(names(&found.templates), vec!["before"]);
}

#[test]
fn lines_list_sorted_templates_then_sorted_modules() {
    let found = TitleDependents {
        templates: ["b", "a"].iter().map(ToString::to_string).collect(),
        modules: ["z", "y"].iter().map(ToString::to_string).collect(),
        ..TitleDependents::default()
    };

    assert_eq!(
        found.lines(),
        "Template:a\nTemplate:b\nModule:y\nModule:z\n"
    );
}

#[test]
fn lines_of_nothing_are_empty() {
    assert_eq!(TitleDependents::default().lines(), "");
}

#[test]
fn without_templates_removes_only_the_listed_templates() {
    let found = TitleDependents {
        templates: ["a", "b", "c"].iter().map(ToString::to_string).collect(),
        modules: ["m"].iter().map(ToString::to_string).collect(),
        templates_analysed: 7,
        modules_analysed: 3,
    };
    let removed = ["b", "other"]
        .iter()
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();

    let kept = found.without_templates(&removed);

    assert_eq!(names(&kept.templates), vec!["a", "c"]);
    assert_eq!(names(&kept.modules), vec!["m"]);
    assert_eq!(kept.templates_analysed, 7);
    assert_eq!(kept.modules_analysed, 3);
}

#[test]
fn the_flagged_names_round_trip_through_the_list_reader() {
    let found = dependents(vec![
        page("Template:base", "{{PAGENAME}}"),
        page("Module:uses", "mw.title.getCurrentTitle()"),
    ]);
    let reader = crate::title_dependency::SimpleTitleDependency::from_lines(&found.lines());

    assert!(crate::title_dependency::TitleDependency::requires_title(
        &reader,
        "{{base|x}}"
    ));
    assert!(crate::title_dependency::TitleDependency::requires_title(
        &reader,
        "{{#invoke:uses|main}}"
    ));
}
