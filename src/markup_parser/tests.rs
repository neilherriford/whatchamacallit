use super::*;

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn lines(text: &str) -> Vec<String> {
    logical_lines(&chars(text))
        .map(|line| line.into_iter().collect())
        .collect()
}

fn describe(chunks: &[Chunk]) -> Vec<String> {
    chunks
        .iter()
        .map(|chunk| match chunk {
            Chunk::PlainText(text) => format!("text:{text}"),
            Chunk::InternalLink(text) => format!("link:{text}"),
            Chunk::Template(template) => format!("template:{}", template.name),
            Chunk::Indent => "indent".to_string(),
        })
        .collect()
}

fn template(name: &str, contents: &str) -> Template {
    Template {
        name: name.to_string(),
        contents: contents.to_string(),
    }
}

fn arguments(name: &str, contents: &str) -> TemplateArguments {
    extract_arguments_from_template(&template(name, contents))
}

fn parse_page(text: &str) -> HashMap<PartOfSpeech, Vec<Entry>> {
    SimpleMarkupParser::default().parse(text)
}

fn entries_of(page: &HashMap<PartOfSpeech, Vec<Entry>>, part_of_speech: PartOfSpeech) -> Vec<Vec<String>> {
    page.get(&part_of_speech)
        .map(|entries| entries.iter().map(|entry| describe(&entry.chunks)).collect())
        .unwrap_or_default()
}

#[test]
fn logical_lines_of_empty_text_is_empty() {
    assert!(lines("").is_empty());
}

#[test]
fn logical_lines_splits_on_newlines() {
    assert_eq!(lines("a\nb"), vec!["a", "b"]);
}

#[test]
fn logical_lines_does_not_invent_a_line_after_a_trailing_newline() {
    assert_eq!(lines("a\nb\n"), vec!["a", "b"]);
}

#[test]
fn logical_lines_strips_a_carriage_return_before_the_newline() {
    assert_eq!(lines("a\r\nb\r\n"), vec!["a", "b"]);
}

#[test]
fn logical_lines_keeps_a_carriage_return_inside_a_line() {
    assert_eq!(lines("a\rb"), vec!["a\rb"]);
}

#[test]
fn logical_lines_keeps_blank_lines() {
    assert_eq!(lines("a\n\nb"), vec!["a", "", "b"]);
}

#[test]
fn logical_lines_of_a_lone_newline_is_one_blank_line() {
    assert_eq!(lines("\n"), vec![""]);
}

#[test]
fn logical_lines_joins_a_template_that_spans_lines() {
    assert_eq!(lines("{{a|\nb}}\nc"), vec!["{{a|\nb}}", "c"]);
}

#[test]
fn logical_lines_joins_nested_templates_that_span_lines() {
    assert_eq!(lines("{{a|{{b}}\n}}\nc"), vec!["{{a|{{b}}\n}}", "c"]);
}

#[test]
fn logical_lines_ends_an_unclosed_template_at_a_header() {
    assert_eq!(lines("{{a\n=H=\nx"), vec!["{{a", "=H=", "x"]);
}

#[test]
fn logical_lines_treats_a_stray_closing_brace_pair_as_text() {
    assert_eq!(lines("a}}\nb"), vec!["a}}", "b"]);
}

#[test]
fn logical_lines_removes_inline_comments() {
    assert_eq!(lines("a<!-- c -->b"), vec!["ab"]);
}

#[test]
fn logical_lines_removes_multi_line_comments_and_the_headers_inside() {
    assert_eq!(lines("a<!--\n=H=\n-->b\nc"), vec!["ab", "c"]);
}

#[test]
fn logical_lines_ignores_braces_inside_comments() {
    assert_eq!(lines("a<!-- {{ -->\nb"), vec!["a", "b"]);
}

#[test]
fn logical_lines_of_only_a_comment_is_one_blank_line() {
    assert_eq!(lines("<!-- x -->"), vec![""]);
}

#[test]
fn logical_lines_lets_an_unclosed_comment_swallow_the_rest() {
    assert_eq!(lines("a<!-- x\nb\nc"), vec!["a"]);
}

#[test]
fn logical_lines_preserves_non_ascii_text() {
    assert_eq!(lines("é\nü"), vec!["é", "ü"]);
}

#[test]
fn matches_sequence_at_index_finds_the_sequence_at_the_index() {
    let text = chars("abc");

    assert!(matches_sequence_at_index(&text, 0, "ab"));
    assert!(matches_sequence_at_index(&text, 1, "bc"));
    assert!(matches_sequence_at_index(&text, 0, "abc"));
}

#[test]
fn matches_sequence_at_index_rejects_a_different_sequence() {
    let text = chars("abc");

    assert!(!matches_sequence_at_index(&text, 1, "ab"));
    assert!(!matches_sequence_at_index(&text, 0, "b"));
}

#[test]
fn matches_sequence_at_index_rejects_a_sequence_running_past_the_end() {
    let text = chars("abc");

    assert!(!matches_sequence_at_index(&text, 2, "cd"));
    assert!(!matches_sequence_at_index(&text, 3, "a"));
    assert!(!matches_sequence_at_index(&chars(""), 0, "a"));
}

#[test]
fn matches_sequence_at_index_accepts_an_empty_sequence() {
    assert!(matches_sequence_at_index(&chars("abc"), 0, ""));
}

#[test]
fn entering_header_needs_a_leading_equals_sign() {
    assert!(entering_header(&chars("==English==")));
    assert!(!entering_header(&chars(" ==English==")));
    assert!(!entering_header(&chars("# list")));
    assert!(!entering_header(&chars("")));
}

#[test]
fn parse_header_reads_the_level_and_title() {
    assert_eq!(parse_header(&chars("==English==")), Some((2, "English".to_string())));
    assert_eq!(parse_header(&chars("===Noun===")), Some((3, "Noun".to_string())));
    assert_eq!(
        parse_header(&chars("====Etymology 1====")),
        Some((4, "Etymology 1".to_string()))
    );
    assert_eq!(parse_header(&chars("=Title=")), Some((1, "Title".to_string())));
}

#[test]
fn parse_header_trims_whitespace_around_the_title() {
    assert_eq!(parse_header(&chars("== English ==")), Some((2, "English".to_string())));
    assert_eq!(parse_header(&chars("==English==  ")), Some((2, "English".to_string())));
}

#[test]
fn parse_header_rejects_lines_without_a_closing_run_of_equals() {
    assert_eq!(parse_header(&chars("==English")), None);
    assert_eq!(parse_header(&chars("=")), None);
    assert_eq!(parse_header(&chars("")), None);
}

#[test]
fn parse_header_rejects_headers_with_no_title() {
    assert_eq!(parse_header(&chars("====")), None);
    assert_eq!(parse_header(&chars("==")), None);
    assert_eq!(parse_header(&chars("== ==")), None);
}

#[test]
fn is_entering_list_reads_the_last_marker_character() {
    assert!(matches!(is_entering_list(&chars("# x")), Some(ListType::Ordered)));
    assert!(matches!(is_entering_list(&chars("## x")), Some(ListType::Ordered)));
    assert!(matches!(is_entering_list(&chars("* x")), Some(ListType::Unordered)));
    assert!(matches!(is_entering_list(&chars("#* x")), Some(ListType::Unordered)));
}

#[test]
fn is_entering_list_treats_an_indented_marker_as_ordered() {
    assert!(matches!(is_entering_list(&chars("#: x")), Some(ListType::Ordered)));
}

#[test]
fn is_entering_list_is_none_for_other_lines() {
    assert!(is_entering_list(&chars("plain")).is_none());
    assert!(is_entering_list(&chars("")).is_none());
    assert!(is_entering_list(&chars(": x")).is_none());
    assert!(is_entering_list(&chars("==H==")).is_none());
}

#[test]
fn parse_list_reads_the_text_after_the_marker() {
    let parsed = parse_list(&chars("# foo"));

    assert!(matches!(
        parsed,
        Some(parsed) if describe(&parsed.chunks) == vec!["text: foo"] && parsed.labels.is_empty()
    ));
}

#[test]
fn parse_list_strips_every_marker_character() {
    let parsed = parse_list(&chars("##   deep"));
    assert!(matches!(
        parsed,
        Some(parsed) if describe(&parsed.chunks) == vec!["text:   deep"]
    ));

    let parsed = parse_list(&chars("#* quote"));
    assert!(matches!(
        parsed,
        Some(parsed) if describe(&parsed.chunks) == vec!["text: quote"]
    ));
}

#[test]
fn parse_list_turns_a_leading_colon_into_an_indent() {
    let parsed = parse_list(&chars("#: example"));

    assert!(matches!(
        parsed,
        Some(parsed) if describe(&parsed.chunks) == vec!["indent", "text: example"]
    ));
}

#[test]
fn parse_list_rejects_markers_with_no_text() {
    assert!(parse_list(&chars("#")).is_none());
    assert!(parse_list(&chars("*")).is_none());
    assert!(parse_list(&chars("##")).is_none());
    assert!(parse_list(&chars("# ")).is_none());
    assert!(parse_list(&chars("#   \t")).is_none());
}

#[test]
fn parse_list_rejects_lines_without_a_marker() {
    assert!(parse_list(&chars("foo")).is_none());
    assert!(parse_list(&chars("")).is_none());
}

#[test]
fn parse_text_of_plain_text_is_one_chunk() {
    assert_eq!(describe(&parse_text(&chars("plain")).chunks), vec!["text:plain"]);
}

#[test]
fn parse_text_of_empty_text_has_no_chunks() {
    let parsed = parse_text(&chars(""));

    assert!(parsed.chunks.is_empty());
    assert!(parsed.labels.is_empty());
}

#[test]
fn parse_text_splits_around_a_template() {
    assert_eq!(
        describe(&parse_text(&chars("head {{ux|en|hi}} tail")).chunks),
        vec!["text:head ", "template:ux", "text: tail"]
    );
}

#[test]
fn parse_text_splits_around_an_internal_link() {
    assert_eq!(
        describe(&parse_text(&chars("see [[word]] now")).chunks),
        vec!["text:see ", "link:[[word]]", "text: now"]
    );
}

#[test]
fn parse_text_keeps_the_template_text_as_written() {
    let parsed = parse_text(&chars("{{ux|en|hi}}"));

    assert!(matches!(
        parsed.chunks.first(),
        Some(Chunk::Template(found)) if found.contents == "{{ux|en|hi}}"
    ));
}

#[test]
fn parse_text_turns_only_leading_colons_into_indents() {
    assert_eq!(
        describe(&parse_text(&chars(": indented")).chunks),
        vec!["indent", "text: indented"]
    );
    assert_eq!(
        describe(&parse_text(&chars(":: deeper")).chunks),
        vec!["indent", "indent", "text: deeper"]
    );
}

#[test]
fn parse_text_keeps_a_colon_after_the_start_of_the_text() {
    assert_eq!(describe(&parse_text(&chars("a: b")).chunks), vec!["text:a: b"]);
    assert_eq!(
        describe(&parse_text(&chars("[[a]]: b")).chunks),
        vec!["link:[[a]]", "text:: b"]
    );
    assert_eq!(
        describe(&parse_text(&chars("{{t}}: b")).chunks),
        vec!["template:t", "text:: b"]
    );
}

#[test]
fn parse_text_keeps_an_unclosed_template_as_text() {
    assert_eq!(describe(&parse_text(&chars("a {{b")).chunks), vec!["text:a {{b"]);
}

#[test]
fn parse_text_keeps_an_unclosed_link_as_text() {
    assert_eq!(describe(&parse_text(&chars("a [[b")).chunks), vec!["text:a [[b"]);
}

#[test]
fn parse_text_collects_english_labels() {
    let parsed = parse_text(&chars("{{lb|en|informal|slang}} meaning"));

    assert_eq!(parsed.labels, vec!["informal", "slang"]);
    assert_eq!(describe(&parsed.chunks), vec!["template:lb", "text: meaning"]);
}

#[test]
fn parse_text_collects_labels_from_every_label_template_name() {
    for name in ["label", "lbl", "lb"] {
        let parsed = parse_text(&chars(&format!("{{{{{name}|en|rare}}}}")));

        assert_eq!(parsed.labels, vec!["rare"], "template {name}");
    }
}

#[test]
fn parse_text_collects_labels_regardless_of_template_name_case() {
    assert_eq!(parse_text(&chars("{{LB|en|rare}}")).labels, vec!["rare"]);
}

#[test]
fn parse_text_collects_labels_for_english_variants() {
    assert_eq!(parse_text(&chars("{{lb|en-GB|british}}")).labels, vec!["british"]);
    assert_eq!(parse_text(&chars("{{lb|EN-us|american}}")).labels, vec!["american"]);
}

#[test]
fn parse_text_ignores_labels_in_other_languages() {
    assert!(parse_text(&chars("{{lb|fr|familier}}")).labels.is_empty());
    assert!(parse_text(&chars("{{lb|enm|archaic}}")).labels.is_empty());
}

#[test]
fn parse_text_skips_empty_label_arguments() {
    assert_eq!(parse_text(&chars("{{lb|en||x}}")).labels, vec!["x"]);
}

#[test]
fn parse_text_collects_labels_across_several_label_templates() {
    assert_eq!(
        parse_text(&chars("{{lb|en|a}} {{lb|en|b|c}}")).labels,
        vec!["a", "b", "c"]
    );
}

#[test]
fn parse_text_drops_denied_templates() {
    let denied = [
        "attention",
        "attn",
        "c",
        "cln",
        "defdate",
        "rfc-sense",
        "rfd-sense",
        "rfdef",
        "rfquotek",
        "rfv-sense",
        "senseid",
        "sid",
        "topics",
    ];

    for name in denied {
        let parsed = parse_text(&chars(&format!("{{{{{name}|en|x}}}}")));

        assert!(parsed.chunks.is_empty(), "template {name}");
    }
}

#[test]
fn parse_text_keeps_templates_that_are_not_denied() {
    for name in ["lb", "ux", "syn", "plural of", "quote-book"] {
        let parsed = parse_text(&chars(&format!("{{{{{name}|en|x}}}}")));

        assert_eq!(describe(&parsed.chunks), vec![format!("template:{name}")]);
    }
}

#[test]
fn parse_text_drops_denied_templates_but_keeps_the_text_around_them() {
    assert_eq!(
        describe(&parse_text(&chars("a {{sid|en|Q1}}b")).chunks),
        vec!["text:a ", "text:b"]
    );
}

#[test]
fn extract_template_reads_the_name_and_contents() {
    let text = "{{lb|en|x}}";
    let (new_index, found) = extract_template(&chars(text), 0);

    assert_eq!(new_index, text.chars().count());
    assert!(matches!(
        found,
        Some(found) if found.name == "lb" && found.contents == "{{lb|en|x}}"
    ));
}

#[test]
fn extract_template_starts_at_the_given_index() {
    let (new_index, found) = extract_template(&chars("ab{{x}}cd"), 2);

    assert_eq!(new_index, 7);
    assert!(matches!(found, Some(found) if found.name == "x"));
}

#[test]
fn extract_template_lowercases_the_name_but_not_the_contents() {
    let (_, found) = extract_template(&chars("{{LB|en}}"), 0);

    assert!(matches!(
        found,
        Some(found) if found.name == "lb" && found.contents == "{{LB|en}}"
    ));
}

#[test]
fn extract_template_trims_whitespace_around_the_name() {
    let (_, found) = extract_template(&chars("{{ lb |en}}"), 0);

    assert!(matches!(
        found,
        Some(found) if found.name == "lb" && found.contents == "{{lb|en}}"
    ));
}

#[test]
fn extract_template_reads_nested_templates() {
    let text = "{{a|{{b|c}}|d}}";
    let (new_index, found) = extract_template(&chars(text), 0);

    assert_eq!(new_index, text.chars().count());
    assert!(matches!(
        found,
        Some(found) if found.name == "a" && found.contents == text
    ));
}

#[test]
fn extract_template_reads_a_template_with_no_arguments() {
    let (new_index, found) = extract_template(&chars("{{foo}}"), 0);

    assert_eq!(new_index, 7);
    assert!(matches!(
        found,
        Some(found) if found.name == "foo" && found.contents == "{{foo}}"
    ));
}

#[test]
fn extract_template_reads_names_containing_colons_and_spaces() {
    let (_, found) = extract_template(&chars("{{list:places in the philippines/en}}"), 0);

    assert!(matches!(
        found,
        Some(found) if found.name == "list:places in the philippines/en"
    ));
}

#[test]
fn extract_template_rejects_an_unclosed_template() {
    let (new_index, found) = extract_template(&chars("{{a|b"), 0);

    assert_eq!(new_index, 5);
    assert!(found.is_none());
}

#[test]
fn extract_template_rejects_an_empty_name() {
    let (new_index, found) = extract_template(&chars("{{}}"), 0);

    assert_eq!(new_index, 4);
    assert!(found.is_none());
}

#[test]
fn extract_internal_link_reads_the_link() {
    let (new_index, found) = extract_internal_link(&chars("[[word]] rest"), 0);

    assert_eq!(new_index, 8);
    assert_eq!(found, Some("[[word]]".to_string()));
}

#[test]
fn extract_internal_link_keeps_the_display_text() {
    let (_, found) = extract_internal_link(&chars("[[word|shown]]"), 0);

    assert_eq!(found, Some("[[word|shown]]".to_string()));
}

#[test]
fn extract_internal_link_reads_templates_inside_the_link() {
    let (_, found) = extract_internal_link(&chars("[[a{{b}}c]]"), 0);

    assert_eq!(found, Some("[[a{{b}}c]]".to_string()));
}

#[test]
fn extract_internal_link_rejects_an_unclosed_link() {
    let (new_index, found) = extract_internal_link(&chars("[[abc"), 0);

    assert_eq!(new_index, 5);
    assert_eq!(found, None);
}

#[test]
fn is_label_template_matches_only_label_names() {
    assert!(is_label_template(&template("label", "{{label}}")));
    assert!(is_label_template(&template("lbl", "{{lbl}}")));
    assert!(is_label_template(&template("lb", "{{lb}}")));
    assert!(!is_label_template(&template("labels", "{{labels}}")));
    assert!(!is_label_template(&template("q", "{{q}}")));
}

#[test]
fn template_arguments_number_positional_arguments_from_one() {
    let found = arguments("lb", "{{lb|en|informal|slang}}");

    assert_eq!(found.ordinals(), vec![1, 2, 3]);
    assert_eq!(found.try_get_value_by_ordinal(1), Some("en".to_string()));
    assert_eq!(found.try_get_value_by_ordinal(2), Some("informal".to_string()));
    assert_eq!(found.try_get_value_by_ordinal(3), Some("slang".to_string()));
}

#[test]
fn template_arguments_trim_values() {
    let found = arguments("t", "{{t| x |y}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("x".to_string()));
}

#[test]
fn template_arguments_treat_blank_values_as_missing() {
    let found = arguments("t", "{{t||y}}");

    assert_eq!(found.try_get_value_by_ordinal(1), None);
    assert_eq!(found.try_get_value_by_ordinal(2), Some("y".to_string()));
}

#[test]
fn template_arguments_return_none_for_an_absent_ordinal() {
    let found = arguments("t", "{{t|a}}");

    assert_eq!(found.try_get_value_by_ordinal(2), None);
}

#[test]
fn template_arguments_keep_named_arguments_out_of_the_ordinals() {
    let found = arguments("ux", "{{ux|en|text|t=translation}}");

    assert_eq!(found.ordinals(), vec![1, 2]);
    assert_eq!(
        found.arguments.get("t").map(String::as_str),
        Some("translation")
    );
}

#[test]
fn template_arguments_number_positional_arguments_around_named_ones() {
    let found = arguments("t", "{{t|a|k=v|b}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("a".to_string()));
    assert_eq!(found.try_get_value_by_ordinal(2), Some("b".to_string()));
}

#[test]
fn template_arguments_accept_explicit_numeric_names() {
    let found = arguments("t", "{{t|2=b|a}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("a".to_string()));
    assert_eq!(found.try_get_value_by_ordinal(2), Some("b".to_string()));
}

#[test]
fn template_arguments_do_not_split_on_pipes_inside_links() {
    let found = arguments("t", "{{t|[[a|b]]|c}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("[[a|b]]".to_string()));
    assert_eq!(found.try_get_value_by_ordinal(2), Some("c".to_string()));
}

#[test]
fn template_arguments_do_not_split_on_pipes_inside_nested_templates() {
    let found = arguments("t", "{{t|{{x|y}}|z}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("{{x|y}}".to_string()));
    assert_eq!(found.try_get_value_by_ordinal(2), Some("z".to_string()));
}

#[test]
fn template_arguments_do_not_treat_equals_inside_links_as_names() {
    let found = arguments("t", "{{t|[[a=b]]}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("[[a=b]]".to_string()));
}

#[test]
fn template_arguments_do_not_treat_equals_inside_nested_templates_as_names() {
    let found = arguments("t", "{{t|{{x|k=v}}}}");

    assert_eq!(found.try_get_value_by_ordinal(1), Some("{{x|k=v}}".to_string()));
}

#[test]
fn template_arguments_of_a_template_without_arguments_are_empty() {
    assert!(arguments("foo", "{{foo}}").ordinals().is_empty());
}

#[test]
fn is_english_accepts_en_and_its_variants() {
    for code in ["en", "EN", "en-GB", "EN-us", "en-hk", "en-AU"] {
        let found = arguments("lb", &format!("{{{{lb|{code}|x}}}}"));

        assert!(is_english(&found), "code {code}");
    }
}

#[test]
fn is_english_rejects_other_languages() {
    for code in ["enm", "fr", "de", "english", "e", "ang"] {
        let found = arguments("lb", &format!("{{{{lb|{code}|x}}}}"));

        assert!(!is_english(&found), "code {code}");
    }
}

#[test]
fn is_english_rejects_a_missing_or_blank_language_code() {
    assert!(!is_english(&arguments("lb", "{{lb||x}}")));
    assert!(!is_english(&arguments("foo", "{{foo}}")));
}

#[test]
fn part_of_speech_names_parse_and_display_symmetrically() {
    let names = [
        ("abbreviation", PartOfSpeech::Abbreviation),
        ("adjective", PartOfSpeech::Adjective),
        ("adverb", PartOfSpeech::Adverb),
        ("article", PartOfSpeech::Article),
        ("combining form", PartOfSpeech::CombiningForm),
        ("conjunction", PartOfSpeech::Conjunction),
        ("contraction", PartOfSpeech::Contraction),
        ("determiner", PartOfSpeech::Determiner),
        ("diacritical mark", PartOfSpeech::DiacriticalMark),
        ("idiom", PartOfSpeech::Idiom),
        ("infix", PartOfSpeech::Infix),
        ("interfix", PartOfSpeech::Interfix),
        ("interjection", PartOfSpeech::Interjection),
        ("intransitive verb", PartOfSpeech::IntransitiveVerb),
        ("letter", PartOfSpeech::Letter),
        ("noun", PartOfSpeech::Noun),
        ("number", PartOfSpeech::Number),
        ("numeral", PartOfSpeech::Numeral),
        ("participle", PartOfSpeech::Participle),
        ("particle", PartOfSpeech::Particle),
        ("phrase", PartOfSpeech::Phrase),
        ("postposition", PartOfSpeech::Postposition),
        ("prefix", PartOfSpeech::Prefix),
        ("preposition", PartOfSpeech::Preposition),
        ("prepositional phrase", PartOfSpeech::PrepositionalPhrase),
        ("pronoun", PartOfSpeech::Pronoun),
        ("proper noun", PartOfSpeech::ProperNoun),
        ("proverb", PartOfSpeech::Proverb),
        ("punctuation mark", PartOfSpeech::PunctuationMark),
        ("suffix", PartOfSpeech::Suffix),
        ("symbol", PartOfSpeech::Symbol),
        ("transitive verb", PartOfSpeech::TransitiveVerb),
        ("verb", PartOfSpeech::Verb),
    ];

    for (name, expected) in names {
        assert_eq!(name.parse::<PartOfSpeech>(), Ok(expected), "parsing {name}");
        assert_eq!(expected.to_string(), name, "displaying {name}");
    }
}

#[test]
fn part_of_speech_parsing_ignores_case_and_surrounding_whitespace() {
    assert_eq!(" Proper Noun ".parse::<PartOfSpeech>(), Ok(PartOfSpeech::ProperNoun));
    assert_eq!("VERB".parse::<PartOfSpeech>(), Ok(PartOfSpeech::Verb));
}

#[test]
fn part_of_speech_parsing_rejects_unknown_names() {
    assert_eq!(
        "Etymology 1".parse::<PartOfSpeech>(),
        Err(PartOfSpeechParseError::UnknownPartOfSpeach("etymology 1".to_string()))
    );
    assert!("".parse::<PartOfSpeech>().is_err());
}

#[test]
fn part_of_speech_defaults_to_unknown() {
    assert_eq!(PartOfSpeech::default(), PartOfSpeech::Unknown);
    assert_eq!(PartOfSpeech::Unknown.to_string(), "unknown");
}

#[test]
fn chunks_display_as_the_text_they_were_parsed_from() {
    assert_eq!(Chunk::PlainText("a".to_string()).to_string(), "a");
    assert_eq!(Chunk::InternalLink("[[b]]".to_string()).to_string(), "[[b]]");
    assert_eq!(
        Chunk::Template(template("t", "{{t|x}}")).to_string(),
        "{{t|x}}"
    );
    assert_eq!(Chunk::Indent.to_string(), " ");
}

#[test]
fn parse_collects_definitions_under_the_part_of_speech() {
    let page = parse_page("==English==\n===Noun===\n# A thing.\n# Another thing.\n");

    assert_eq!(page.len(), 1);
    assert_eq!(
        entries_of(&page, PartOfSpeech::Noun),
        vec![vec!["text: A thing."], vec!["text: Another thing."]]
    );
}

#[test]
fn parse_ignores_languages_before_english() {
    let page = parse_page("==French==\n===Noun===\n# fr\n==English==\n===Noun===\n# en\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: en"]]);
}

#[test]
fn parse_stops_at_the_first_language_after_english() {
    let page = parse_page("==English==\n===Noun===\n# en\n==German==\n===Noun===\n# de\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: en"]]);
}

#[test]
fn parse_of_a_page_without_english_is_empty() {
    assert!(parse_page("==French==\n===Noun===\n# fr\n").is_empty());
    assert!(parse_page("").is_empty());
    assert!(parse_page("just some text\n").is_empty());
}

#[test]
fn parse_separates_parts_of_speech() {
    let page = parse_page("==English==\n===Noun===\n# n\n===Verb===\n# v\n");

    assert_eq!(page.len(), 2);
    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: n"]]);
    assert_eq!(entries_of(&page, PartOfSpeech::Verb), vec![vec!["text: v"]]);
}

#[test]
fn parse_reads_multi_word_parts_of_speech() {
    let page = parse_page("==English==\n===Proper noun===\n# p\n");

    assert_eq!(entries_of(&page, PartOfSpeech::ProperNoun), vec![vec!["text: p"]]);
}

#[test]
fn parse_ignores_lines_under_headings_that_are_not_parts_of_speech() {
    let page = parse_page("==English==\n===Noun===\n# a\n====Synonyms====\n* x\n===Etymology===\n# y\n");

    assert_eq!(page.len(), 1);
    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: a"]]);
}

#[test]
fn parse_reads_parts_of_speech_nested_under_etymology_headings() {
    let page = parse_page("==English==\n===Etymology 1===\n====Noun====\n# a\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: a"]]);
}

#[test]
fn parse_merges_the_same_part_of_speech_across_etymologies() {
    let page = parse_page(
        "==English==\n===Etymology 1===\n====Noun====\n# a\n===Etymology 2===\n====Noun====\n# b\n",
    );

    assert_eq!(
        entries_of(&page, PartOfSpeech::Noun),
        vec![vec!["text: a"], vec!["text: b"]]
    );
}

#[test]
fn parse_ignores_lines_before_the_first_part_of_speech() {
    let page = parse_page("==English==\n# stray\n===Noun===\n# a\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: a"]]);
}

#[test]
fn parse_ignores_lines_that_are_not_list_items() {
    let page = parse_page("==English==\n===Noun===\nplain\n{{template}}\n# a\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: a"]]);
}

#[test]
fn parse_skips_empty_list_items() {
    let page = parse_page("==English==\n===Noun===\n# \n#\n#   \n# real\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: real"]]);
}

#[test]
fn parse_skips_definitions_that_contain_only_a_denied_template() {
    let page = parse_page("==English==\n===Noun===\n# {{rfdef|en}}\n# real\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: real"]]);
}

#[test]
fn parse_collects_english_labels_for_each_entry() {
    let page = parse_page("==English==\n===Noun===\n# {{lb|en|informal}} meaning\n");
    let labels: Vec<Vec<String>> = page
        .get(&PartOfSpeech::Noun)
        .map(|entries| entries.iter().map(|entry| entry.labels.clone()).collect())
        .unwrap_or_default();

    assert_eq!(labels, vec![vec!["informal".to_string()]]);
}

#[test]
fn parse_lists_each_label_of_an_entry_once() {
    let page = parse_page("==English==\n===Noun===\n# {{lb|en|a}} {{lb|en|a|b}} text\n");
    let labels: Vec<Vec<String>> = page
        .get(&PartOfSpeech::Noun)
        .map(|entries| entries.iter().map(|entry| entry.labels.clone()).collect())
        .unwrap_or_default();

    assert_eq!(labels, vec![vec!["a".to_string(), "b".to_string()]]);
}

#[test]
fn parse_records_the_kind_of_each_list_marker() {
    let page = parse_page("==English==\n===Noun===\n# def\n#: example\n#* quote\n");
    let kinds: Vec<bool> = page
        .get(&PartOfSpeech::Noun)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| matches!(entry.kind, ListType::Unordered))
                .collect()
        })
        .unwrap_or_default();

    assert_eq!(kinds, vec![false, false, true]);
}

#[test]
fn parse_keeps_the_indent_of_colon_lines() {
    let page = parse_page("==English==\n===Noun===\n# def\n#: example\n");

    assert_eq!(
        entries_of(&page, PartOfSpeech::Noun),
        vec![vec!["text: def"], vec!["indent", "text: example"]]
    );
}

#[test]
fn parse_joins_templates_that_span_lines() {
    let page = parse_page("==English==\n===Noun===\n# {{ux|en|a\n|b}} tail\n");

    assert_eq!(
        entries_of(&page, PartOfSpeech::Noun),
        vec![vec!["text: ", "template:ux", "text: tail"]]
    );
}

#[test]
fn parse_drops_comments_inside_definitions() {
    let page = parse_page("==English==\n===Noun===\n# a<!-- hidden -->b\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: ab"]]);
}

#[test]
fn parse_reads_headers_that_carry_a_trailing_comment() {
    let page = parse_page("==English==\n===Verb===<!-- note -->\n# v\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Verb), vec![vec!["text: v"]]);
}

#[test]
fn parse_ignores_headers_that_are_inside_comments() {
    let page = parse_page("==English==\n===Noun===\n# a\n<!--\n===Verb===\n# hidden\n-->\n# b\n");

    assert_eq!(
        entries_of(&page, PartOfSpeech::Noun),
        vec![vec!["text: a"], vec!["text: b"]]
    );
    assert!(!page.contains_key(&PartOfSpeech::Verb));
}

#[test]
fn parse_handles_windows_line_endings() {
    let page = parse_page("==English==\r\n===Noun===\r\n# a\r\n# b\r\n");

    assert_eq!(
        entries_of(&page, PartOfSpeech::Noun),
        vec![vec!["text: a"], vec!["text: b"]]
    );
}

#[test]
fn parse_keeps_colons_inside_definitions() {
    let page = parse_page("==English==\n===Noun===\n# a: b\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Noun), vec![vec!["text: a: b"]]);
}

#[test]
fn parse_ends_an_unclosed_template_at_the_next_header() {
    let page = parse_page("==English==\n===Noun===\n# {{broken\n===Verb===\n# v\n");

    assert_eq!(entries_of(&page, PartOfSpeech::Verb), vec![vec!["text: v"]]);
}

#[test]
fn parse_works_through_the_trait_object() {
    let parser: Box<dyn MarkupParser> = Box::new(SimpleMarkupParser::default());
    let page = parser.parse("==English==\n===Noun===\n# a\n");

    assert_eq!(page.len(), 1);
}
