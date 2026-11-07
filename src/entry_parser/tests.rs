use super::*;

fn plain(text: &str) -> Chunk {
    Chunk::PlainText(text.to_string())
}

fn link(text: &str) -> Chunk {
    Chunk::InternalLink(text.to_string())
}

fn template_of(name: &str, contents: &str) -> Template {
    Template {
        name: name.to_string(),
        contents: contents.to_string(),
    }
}

fn template(name: &str, contents: &str) -> Chunk {
    Chunk::Template(template_of(name, contents))
}

fn definition_line(chunks: Vec<Chunk>) -> Entry {
    Entry {
        kind: ListType::Ordered,
        labels: Vec::new(),
        chunks,
    }
}

fn labelled_definition_line(labels: &[&str], chunks: Vec<Chunk>) -> Entry {
    Entry {
        kind: ListType::Ordered,
        labels: labels.iter().map(|label| label.to_string()).collect(),
        chunks,
    }
}

fn colon_line(chunks: Vec<Chunk>) -> Entry {
    let mut all = vec![Chunk::Indent];
    all.extend(chunks);

    Entry {
        kind: ListType::Ordered,
        labels: Vec::new(),
        chunks: all,
    }
}

fn star_line(chunks: Vec<Chunk>) -> Entry {
    Entry {
        kind: ListType::Unordered,
        labels: Vec::new(),
        chunks,
    }
}

fn parse(entries: &[Entry]) -> Vec<Definition> {
    SimpleEntryParser::default().parse(PartOfSpeech::Noun, entries)
}

fn markups(entries: &[Entry]) -> Vec<String> {
    parse(entries)
        .into_iter()
        .map(|definition| definition.markup)
        .collect()
}

#[test]
fn unknown_part_of_speech_yields_no_definitions() {
    let entries = [definition_line(vec![plain(" A thing.")])];

    assert!(
        SimpleEntryParser::default()
            .parse(PartOfSpeech::Unknown, &entries)
            .is_empty()
    );
}

#[test]
fn no_entries_yield_no_definitions() {
    assert!(parse(&[]).is_empty());
}

#[test]
fn a_definition_carries_its_part_of_speech() {
    let entries = [definition_line(vec![plain(" A thing.")])];
    let definitions = SimpleEntryParser::default().parse(PartOfSpeech::Verb, &entries);

    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].part_of_speech, PartOfSpeech::Verb);
}

#[test]
fn a_single_definition_becomes_one_definition() {
    let entries = [definition_line(vec![plain(" A thing.")])];

    assert_eq!(markups(&entries), vec![" A thing."]);
}

#[test]
fn each_top_level_line_becomes_its_own_definition() {
    let entries = [
        definition_line(vec![plain(" First.")]),
        definition_line(vec![plain(" Second.")]),
        definition_line(vec![plain(" Third.")]),
    ];

    assert_eq!(markups(&entries), vec![" First.", " Second.", " Third."]);
}

#[test]
fn definition_markup_joins_plain_text_links_and_templates() {
    let entries = [definition_line(vec![
        template("lb", "{{lb|en|informal}}"),
        plain(" see "),
        link("[[word]]"),
    ])];

    assert_eq!(markups(&entries), vec!["{{lb|en|informal}} see [[word]]"]);
}

#[test]
fn definition_labels_are_kept_with_that_definition() {
    let entries = [
        labelled_definition_line(&["informal"], vec![plain(" First.")]),
        labelled_definition_line(&["slang", "rare"], vec![plain(" Second.")]),
    ];
    let definitions = parse(&entries);

    assert_eq!(definitions[0].labels, vec!["informal"]);
    assert_eq!(definitions[1].labels, vec!["slang", "rare"]);
}

#[test]
fn top_level_line_with_a_qualifier_is_still_a_definition() {
    let entries = [
        definition_line(vec![plain(" To consume.")]),
        definition_line(vec![template("q", "{{q|rare}}"), plain(" To drink.")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" To consume.", "{{q|rare}} To drink."]
    );
}

#[test]
fn an_example_template_line_is_attached_to_the_definition() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![template("ux", "{{ux|en|It is a thing.}}")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example: {{ux|en|It is a thing.}}"]
    );
}

#[test]
fn every_example_template_name_marks_an_example() {
    let names = [
        "ux",
        "uxi",
        "usex",
        "uxa",
        "suffixusex",
        "prefixusex",
        "afex",
        "rfex",
        "affixusex",
        "sufex",
        "prefex",
    ];

    for name in names {
        let contents = format!("{{{{{name}|en|x}}}}");
        let entries = [
            definition_line(vec![plain(" D.")]),
            colon_line(vec![template(name, &contents)]),
        ];

        assert_eq!(
            markups(&entries),
            vec![format!(" D. Example: {contents}")],
            "template {name}"
        );
    }
}

#[test]
fn a_colon_line_without_a_template_falls_back_to_an_example() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![plain(" It is a thing.")]),
    ];

    assert_eq!(markups(&entries), vec![" A thing. Example:  It is a thing."]);
}

#[test]
fn a_star_line_without_a_template_falls_back_to_a_quotation() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        star_line(vec![plain(" 1900, Someone, A Book")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example quotation:  1900, Someone, A Book"]
    );
}

#[test]
fn a_quotation_template_line_is_a_quotation() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        star_line(vec![template("quote-book", "{{quote-book|en|year=1900}}")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example quotation: {{quote-book|en|year=1900}}"]
    );
}

#[test]
fn every_quotation_template_name_marks_a_quotation() {
    let names = [
        "quote-book",
        "quote-journal",
        "quote-web",
        "quote",
        "cite-book",
        "cite-web",
        "rq:nyt",
        "circa",
        "c.",
        "ante",
        "rfdate",
        "rfdatek",
        "see citations",
        "see more citations",
        "seemorecites",
    ];

    for name in names {
        assert!(
            is_quotation_template(&template_of(name, "{{x}}")),
            "template {name}"
        );
    }
}

#[test]
fn an_example_template_beats_the_star_fallback() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        star_line(vec![template("ux", "{{ux|en|Explicit example.}}")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example: {{ux|en|Explicit example.}}"]
    );
}

#[test]
fn a_quotation_template_beats_the_colon_fallback() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![template("quote-book", "{{quote-book|en}}")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example quotation: {{quote-book|en}}"]
    );
}

#[test]
fn a_qualifier_does_not_hide_an_example_template() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![
            template("q", "{{q|UK}}"),
            template("ux", "{{ux|en|text}}"),
        ]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example: {{q|UK}}{{ux|en|text}}"]
    );
}

#[test]
fn several_examples_and_quotations_are_attached_in_order() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![template("ux", "{{ux|en|One.}}")]),
        star_line(vec![template("quote-book", "{{quote-book|en|two}}")]),
        colon_line(vec![template("ux", "{{ux|en|Three.}}")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![
            " A thing. Example: {{ux|en|One.}} Example quotation: {{quote-book|en|two}} Example: {{ux|en|Three.}}"
        ]
    );
}

#[test]
fn examples_belong_to_the_definition_they_follow() {
    let entries = [
        definition_line(vec![plain(" First.")]),
        colon_line(vec![template("ux", "{{ux|en|One.}}")]),
        definition_line(vec![plain(" Second.")]),
        colon_line(vec![template("ux", "{{ux|en|Two.}}")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![
            " First. Example: {{ux|en|One.}}",
            " Second. Example: {{ux|en|Two.}}"
        ]
    );
}

#[test]
fn the_leading_indent_never_reaches_the_markup() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![plain("text")]),
    ];

    assert_eq!(markups(&entries), vec![" A thing. Example: text"]);
}

#[test]
fn relation_lines_are_skipped() {
    let names = [
        "syn",
        "synonyms",
        "nearsyn",
        "near-syn",
        "near-synonyms",
        "nsyn",
        "cot",
        "co",
        "coi",
        "coord",
        "coordinate terms",
        "coa",
        "comeronyms",
        "hyper",
        "hypernyms",
        "hypo",
        "hyponyms",
        "hol",
        "holo",
        "holonyms",
        "mer",
        "mero",
        "meronyms",
        "alti",
        "collocation",
        "collocations",
        "color panel",
        "rfquote-sense",
        "rfquotek",
        "list:places in the philippines/en",
    ];

    for name in names {
        let contents = format!("{{{{{name}|en|x}}}}");
        let entries = [
            definition_line(vec![plain(" A thing.")]),
            colon_line(vec![template(name, &contents)]),
        ];

        assert_eq!(markups(&entries), vec![" A thing."], "template {name}");
    }
}

#[test]
fn a_relation_line_with_a_leading_qualifier_is_skipped() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![
            template("q", "{{q|UK}}"),
            template("syn", "{{syn|en|reservation}}"),
        ]),
    ];

    assert_eq!(markups(&entries), vec![" A thing."]);
}

#[test]
fn a_relation_line_with_a_sense_label_is_skipped() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![
            template("sense", "{{sense|medicine}}"),
            template("synonyms", "{{synonyms|en|herb}}"),
        ]),
    ];

    assert_eq!(markups(&entries), vec![" A thing."]);
}

#[test]
fn a_star_relation_line_is_skipped() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        star_line(vec![template("syn", "{{syn|en|x}}")]),
    ];

    assert_eq!(markups(&entries), vec![" A thing."]);
}

#[test]
fn a_relation_template_inside_a_definition_line_is_kept() {
    let entries = [definition_line(vec![
        plain(" Big. "),
        template("syn", "{{syn|en|large}}"),
    ])];

    assert_eq!(markups(&entries), vec![" Big. {{syn|en|large}}"]);
}

#[test]
fn an_example_template_beats_a_relation_template_on_the_same_line() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![
            template("ux", "{{ux|en|text}}"),
            template("syn", "{{syn|en|x}}"),
        ]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example: {{ux|en|text}}{{syn|en|x}}"]
    );
}

#[test]
fn housekeeping_only_lines_are_dropped() {
    let names = [
        "see citations",
        "see more citations",
        "seecites",
        "seemorecites",
        "rfdate",
        "rfdatek",
    ];

    for name in names {
        let contents = format!("{{{{{name}|en}}}}");
        let entries = [
            definition_line(vec![plain(" A thing.")]),
            star_line(vec![template(name, &contents)]),
        ];

        assert_eq!(markups(&entries), vec![" A thing."], "template {name}");
    }
}

#[test]
fn several_housekeeping_templates_on_one_line_are_dropped() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        star_line(vec![
            template("rfdate", "{{rfdate|en}}"),
            plain(" "),
            template("see citations", "{{see citations|en}}"),
        ]),
    ];

    assert_eq!(markups(&entries), vec![" A thing."]);
}

#[test]
fn a_housekeeping_template_with_real_text_is_kept_as_a_quotation() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        star_line(vec![template("rfdate", "{{rfdate|en}}"), plain(" Real quote text")]),
    ];

    assert_eq!(
        markups(&entries),
        vec![" A thing. Example quotation: {{rfdate|en}} Real quote text"]
    );
}

#[test]
fn blank_sub_lines_are_dropped() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![]),
        colon_line(vec![plain("   ")]),
        star_line(vec![]),
        star_line(vec![plain(" ")]),
    ];

    assert_eq!(markups(&entries), vec![" A thing."]);
}

#[test]
fn a_sub_line_with_only_a_link_is_kept() {
    let entries = [
        definition_line(vec![plain(" A thing.")]),
        colon_line(vec![link("[[word]]")]),
    ];

    assert_eq!(markups(&entries), vec![" A thing. Example: [[word]]"]);
}

#[test]
fn antonyms_are_collected_for_the_definition_they_follow() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en|small|tiny}}")]),
    ];
    let definitions = parse(&entries);

    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].antonyms, vec!["small", "tiny"]);
    assert_eq!(definitions[0].markup, " Big.");
}

#[test]
fn antonym_template_aliases_are_recognised() {
    for name in ["ant", "antonym", "antonyms"] {
        let contents = format!("{{{{{name}|en|small}}}}");
        let entries = [
            definition_line(vec![plain(" Big.")]),
            colon_line(vec![template(name, &contents)]),
        ];

        assert_eq!(parse(&entries)[0].antonyms, vec!["small"], "template {name}");
    }
}

#[test]
fn antonym_inline_modifiers_are_removed() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en|tiny<q:rare>|small}}")]),
    ];

    assert_eq!(parse(&entries)[0].antonyms, vec!["tiny", "small"]);
}

#[test]
fn antonym_named_arguments_are_not_terms() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en|small|q1=archaic}}")]),
    ];

    assert_eq!(parse(&entries)[0].antonyms, vec!["small"]);
}

#[test]
fn antonym_empty_terms_are_dropped() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en||small|<q:x>}}")]),
    ];

    assert_eq!(parse(&entries)[0].antonyms, vec!["small"]);
}

#[test]
fn non_english_antonyms_are_discarded_without_becoming_an_example() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|fr|petit}}")]),
    ];
    let definitions = parse(&entries);

    assert!(definitions[0].antonyms.is_empty());
    assert_eq!(definitions[0].markup, " Big.");
}

#[test]
fn english_variant_antonyms_are_accepted() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en-GB|colour}}")]),
    ];

    assert_eq!(parse(&entries)[0].antonyms, vec!["colour"]);
}

#[test]
fn repeated_antonyms_are_listed_once() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en|small|tiny}}")]),
        colon_line(vec![template("antonyms", "{{antonyms|en|tiny|little}}")]),
    ];

    assert_eq!(
        parse(&entries)[0].antonyms,
        vec!["small", "tiny", "little"]
    );
}

#[test]
fn antonyms_reset_for_the_next_definition() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant|en|small}}")]),
        definition_line(vec![plain(" Large.")]),
    ];
    let definitions = parse(&entries);

    assert_eq!(definitions[0].antonyms, vec!["small"]);
    assert!(definitions[1].antonyms.is_empty());
}

#[test]
fn an_antonym_line_without_arguments_adds_nothing() {
    let entries = [
        definition_line(vec![plain(" Big.")]),
        colon_line(vec![template("ant", "{{ant}}")]),
    ];
    let definitions = parse(&entries);

    assert!(definitions[0].antonyms.is_empty());
    assert_eq!(definitions[0].markup, " Big.");
}

#[test]
fn is_sub_line_is_true_for_star_and_indented_lines() {
    assert!(is_sub_line(&star_line(vec![plain("x")])));
    assert!(is_sub_line(&colon_line(vec![plain("x")])));
}

#[test]
fn is_sub_line_is_false_for_top_level_lines() {
    assert!(!is_sub_line(&definition_line(vec![plain("x")])));
    assert!(!is_sub_line(&definition_line(vec![])));
}

#[test]
fn extract_example_type_is_none_for_top_level_lines() {
    let entry = definition_line(vec![template("ux", "{{ux|en|x}}")]);

    assert!(extract_example_type(&entry).is_none());
}

#[test]
fn extract_example_type_reads_templates_before_the_fallbacks() {
    let entry = star_line(vec![template("ux", "{{ux|en|x}}")]);

    assert!(matches!(
        extract_example_type(&entry),
        Some(ExampleType::Example)
    ));
}

#[test]
fn extract_example_type_falls_back_by_marker() {
    assert!(matches!(
        extract_example_type(&star_line(vec![plain("x")])),
        Some(ExampleType::Quotation)
    ));
    assert!(matches!(
        extract_example_type(&colon_line(vec![plain("x")])),
        Some(ExampleType::Example)
    ));
}

#[test]
fn template_name_matching_ignores_case_and_surrounding_whitespace() {
    assert!(is_example_template(&template_of(" UX ", "{{ux}}")));
    assert!(is_quotation_template(&template_of("Quote-Book", "{{x}}")));
}

#[test]
fn qualifiers_and_labels_are_not_example_or_quotation_templates() {
    for name in ["q", "qualifier", "lb", "label", "syn", "ant", "w", "m"] {
        assert!(!is_example_template(&template_of(name, "{{x}}")), "{name}");
        assert!(!is_quotation_template(&template_of(name, "{{x}}")), "{name}");
    }
}

#[test]
fn skipped_template_matching_requires_an_exact_name_or_the_list_prefix() {
    assert!(is_skipped_template(&template_of("syn", "{{syn}}")));
    assert!(is_skipped_template(&template_of("list:anything", "{{x}}")));
    assert!(!is_skipped_template(&template_of("synonym of", "{{x}}")));
    assert!(!is_skipped_template(&template_of("ux", "{{x}}")));
}

#[test]
fn housekeeping_template_matching_is_exact() {
    assert!(is_housekeeping_template(&template_of("rfdate", "{{x}}")));
    assert!(is_housekeeping_template(&template_of("seecites", "{{x}}")));
    assert!(!is_housekeeping_template(&template_of("rfquotek", "{{x}}")));
    assert!(!is_housekeeping_template(&template_of("quote-book", "{{x}}")));
}

#[test]
fn strip_term_modifiers_cuts_at_the_first_angle_bracket() {
    assert_eq!(strip_term_modifiers("tiny<q:rare>"), "tiny");
    assert_eq!(strip_term_modifiers("tiny<t:small><id:x>"), "tiny");
}

#[test]
fn strip_term_modifiers_trims_whitespace() {
    assert_eq!(strip_term_modifiers("  spaced  "), "spaced");
    assert_eq!(strip_term_modifiers("word <q:x>"), "word");
}

#[test]
fn strip_term_modifiers_of_a_bare_modifier_is_empty() {
    assert_eq!(strip_term_modifiers("<q:x>"), "");
    assert_eq!(strip_term_modifiers(""), "");
}

#[test]
fn append_chunks_skips_indents_and_joins_the_rest() {
    let mut buffer = String::from("start:");
    append_chunks(
        &mut buffer,
        &[
            Chunk::Indent,
            plain(" a "),
            link("[[b]]"),
            template("t", "{{t}}"),
        ],
    );

    assert_eq!(buffer, "start: a [[b]]{{t}}");
}
