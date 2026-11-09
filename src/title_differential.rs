use crate::{
    markup_renderer::{DefinitionOutcome, DefinitionRenderer},
    pipeline::BATCH_TITLE,
    template_examples::TemplateExamples,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::task::JoinSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Same,
    Differs,
    Failed,
}

#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    differs: usize,
    failed: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verdicts {
    pub cleared: BTreeSet<String>,
    pub differs: BTreeSet<String>,
    pub failed: BTreeSet<String>,
    pub examples: usize,
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn rendered_alone(
    renderer: &dyn DefinitionRenderer,
    title: &str,
    markup: &str,
) -> Option<String> {
    match renderer
        .render_definitions(title, &[markup.to_string()])
        .await
    {
        Ok(outcomes) => match outcomes.into_iter().next() {
            Some(DefinitionOutcome::Rendered(text)) => Some(collapse_whitespace(&text)),
            _ => None,
        },
        Err(_) => None,
    }
}

async fn compare(renderer: &dyn DefinitionRenderer, word: &str, markup: &str) -> Comparison {
    let own = rendered_alone(renderer, word, markup).await;
    let shared = rendered_alone(renderer, BATCH_TITLE, markup).await;

    match (own, shared) {
        (Some(own), Some(shared)) if own == shared => Comparison::Same,
        (Some(_), Some(_)) => Comparison::Differs,
        _ => Comparison::Failed,
    }
}

fn record(tallies: &mut BTreeMap<String, Tally>, template: String, comparison: Comparison) {
    let tally = tallies.entry(template).or_default();

    match comparison {
        Comparison::Same => {}
        Comparison::Differs => tally.differs += 1,
        Comparison::Failed => tally.failed += 1,
    }
}

fn verdicts_from(tallies: BTreeMap<String, Tally>, examples: usize) -> Verdicts {
    let mut verdicts = Verdicts {
        examples,
        ..Verdicts::default()
    };

    for (template, tally) in tallies {
        if tally.differs > 0 {
            verdicts.differs.insert(template);
        } else if tally.failed > 0 {
            verdicts.failed.insert(template);
        } else {
            verdicts.cleared.insert(template);
        }
    }

    verdicts
}

pub async fn compare_titles(
    renderer: Arc<dyn DefinitionRenderer>,
    examples: &TemplateExamples,
    concurrency: usize,
) -> Result<Verdicts, String> {
    let jobs = examples.jobs();
    let total = jobs.len();
    let mut tallies = BTreeMap::new();
    let mut tasks = JoinSet::new();

    for (template, word, markup) in jobs {
        while tasks.len() >= concurrency.max(1) {
            if let Some(done) = tasks.join_next().await {
                let (template, comparison) = done.map_err(|err| err.to_string())?;
                record(&mut tallies, template, comparison);
            }
        }

        let renderer = renderer.clone();
        tasks.spawn(async move {
            let comparison = compare(renderer.as_ref(), &word, &markup).await;
            (template, comparison)
        });
    }

    while let Some(done) = tasks.join_next().await {
        let (template, comparison) = done.map_err(|err| err.to_string())?;
        record(&mut tallies, template, comparison);
    }

    Ok(verdicts_from(tallies, total))
}

#[cfg(test)]
mod tests;
