use super::*;
use crate::{markup_renderer::Error, title_dependency::SimpleTitleDependency};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Script = Box<dyn Fn(&str, &str) -> Result<Vec<DefinitionOutcome>, Error> + Send + Sync>;

struct ScriptedRenderer {
    script: Script,
    calls: Mutex<Vec<(String, String)>>,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
}

fn renderer(
    script: impl Fn(&str, &str) -> Result<Vec<DefinitionOutcome>, Error> + Send + Sync + 'static,
) -> Arc<ScriptedRenderer> {
    Arc::new(ScriptedRenderer {
        script: Box::new(script),
        calls: Mutex::new(Vec::new()),
        in_flight: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
    })
}

#[async_trait::async_trait]
impl DefinitionRenderer for ScriptedRenderer {
    async fn render_definitions(
        &self,
        page_title: &str,
        definitions: &[String],
    ) -> Result<Vec<DefinitionOutcome>, Error> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::task::yield_now().await;

        match self.calls.lock() {
            Ok(mut calls) => calls.push((page_title.to_string(), definitions.join("|"))),
            Err(poisoned) => poisoned
                .into_inner()
                .push((page_title.to_string(), definitions.join("|"))),
        }
        let result = (self.script)(page_title, &definitions.join("|"));
        self.in_flight.fetch_sub(1, Ordering::SeqCst);

        result
    }
}

fn rendered(text: &str) -> Result<Vec<DefinitionOutcome>, Error> {
    Ok(vec![DefinitionOutcome::Rendered(text.to_string())])
}

fn examples(entries: &[(&str, &str)]) -> TemplateExamples {
    let dependency = SimpleTitleDependency::from_lines("Template:a\nTemplate:b\n");
    let mut examples = TemplateExamples::new(10, 10);

    for (word, markup) in entries {
        examples.add_markup(&dependency, word, markup);
    }

    examples
}

fn names(set: &BTreeSet<String>) -> Vec<&str> {
    set.iter().map(String::as_str).collect()
}

async fn verdicts(
    renderer: Arc<ScriptedRenderer>,
    examples: &TemplateExamples,
    concurrency: usize,
) -> Verdicts {
    match compare_titles(renderer, examples, concurrency).await {
        Ok(verdicts) => verdicts,
        Err(err) => panic!("unexpected error: {err}"),
    }
}

#[tokio::test]
async fn a_template_with_the_same_output_under_both_titles_is_cleared() {
    let renderer = renderer(|_, _| rendered("same"));
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 4).await;

    assert_eq!(names(&found.cleared), vec!["a"]);
    assert!(found.differs.is_empty());
    assert!(found.failed.is_empty());
}

#[tokio::test]
async fn a_template_whose_output_changes_with_the_title_differs() {
    let renderer = renderer(|title, _| rendered(&format!("title {title}")));
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 4).await;

    assert_eq!(names(&found.differs), vec!["a"]);
    assert!(found.cleared.is_empty());
}

#[tokio::test]
async fn whitespace_differences_do_not_count() {
    let renderer = renderer(|title, _| {
        if title == BATCH_TITLE {
            rendered("a  b\n c")
        } else {
            rendered("a b c")
        }
    });
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 4).await;

    assert_eq!(names(&found.cleared), vec!["a"]);
}

#[tokio::test]
async fn a_failed_render_keeps_the_template_flagged_as_failed() {
    let renderer = renderer(|_, _| Ok(vec![DefinitionOutcome::Failed("bad".to_string())]));
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 4).await;

    assert_eq!(names(&found.failed), vec!["a"]);
    assert!(found.cleared.is_empty());
}

#[tokio::test]
async fn a_request_error_counts_as_a_failed_render() {
    let renderer = renderer(|_, _| Err(Error::RenderingError("down".to_string())));
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 4).await;

    assert_eq!(names(&found.failed), vec!["a"]);
}

#[tokio::test]
async fn an_empty_outcome_list_counts_as_a_failed_render() {
    let renderer = renderer(|_, _| Ok(Vec::new()));
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 4).await;

    assert_eq!(names(&found.failed), vec!["a"]);
}

#[tokio::test]
async fn one_differing_example_outweighs_cleared_and_failed_ones() {
    let renderer = renderer(|title, markup| {
        if markup.contains("same") {
            rendered("same")
        } else if markup.contains("fail") {
            Err(Error::RenderingError("down".to_string()))
        } else {
            rendered(title)
        }
    });
    let found = verdicts(
        renderer,
        &examples(&[
            ("w1", "{{a|same}}"),
            ("w2", "{{a|fail}}"),
            ("w3", "{{a|other}}"),
        ]),
        4,
    )
    .await;

    assert_eq!(names(&found.differs), vec!["a"]);
    assert!(found.failed.is_empty());
    assert!(found.cleared.is_empty());
}

#[tokio::test]
async fn a_failed_example_prevents_clearing_the_template() {
    let renderer = renderer(|_, markup| {
        if markup.contains("fail") {
            Err(Error::RenderingError("down".to_string()))
        } else {
            rendered("same")
        }
    });
    let found = verdicts(
        renderer,
        &examples(&[("w1", "{{a|ok}}"), ("w2", "{{a|fail}}")]),
        4,
    )
    .await;

    assert_eq!(names(&found.failed), vec!["a"]);
}

#[tokio::test]
async fn each_template_gets_its_own_verdict() {
    let renderer = renderer(|title, markup| {
        if markup.contains("{{b") {
            rendered(title)
        } else {
            rendered("same")
        }
    });
    let found = verdicts(
        renderer,
        &examples(&[("w1", "{{a|x}}"), ("w2", "{{b|x}}")]),
        4,
    )
    .await;

    assert_eq!(names(&found.cleared), vec!["a"]);
    assert_eq!(names(&found.differs), vec!["b"]);
}

#[tokio::test]
async fn every_example_is_rendered_under_its_word_and_the_shared_title() {
    let renderer = renderer(|_, _| rendered("same"));
    let found = verdicts(
        renderer.clone(),
        &examples(&[("w1", "{{a|x}}"), ("w2", "{{a|y}}")]),
        1,
    )
    .await;
    let calls = match renderer.calls.lock() {
        Ok(calls) => calls.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    assert_eq!(found.examples, 2);
    assert_eq!(calls.len(), 4);
    for title in ["w1", "w2", BATCH_TITLE] {
        assert!(calls.iter().any(|(called, _)| called == title));
    }
}

#[tokio::test]
async fn no_examples_produce_no_verdicts() {
    let renderer = renderer(|_, _| rendered("same"));
    let found = verdicts(renderer.clone(), &examples(&[]), 4).await;

    assert_eq!(found, Verdicts::default());
    assert_eq!(renderer.peak.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_concurrency_of_one_renders_one_example_at_a_time() {
    let renderer = renderer(|_, _| rendered("same"));
    let entries = (0..6)
        .map(|n| (format!("w{n}"), format!("{{{{a|{n}}}}}")))
        .collect::<Vec<_>>();
    let dependency = SimpleTitleDependency::from_lines("Template:a\n");
    let mut many = TemplateExamples::new(10, 10);
    for (word, markup) in &entries {
        many.add_markup(&dependency, word, markup);
    }

    verdicts(renderer.clone(), &many, 1).await;

    assert_eq!(renderer.peak.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_concurrency_of_zero_still_renders_every_example() {
    let renderer = renderer(|_, _| rendered("same"));
    let found = verdicts(renderer, &examples(&[("w1", "{{a|x}}")]), 0).await;

    assert_eq!(names(&found.cleared), vec!["a"]);
}
