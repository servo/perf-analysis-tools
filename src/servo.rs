use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    str,
    time::Duration,
};

use html5ever::{
    local_name, namespace_url, ns,
    tendril::{StrTendril, TendrilSink},
    tree_builder::TreeBuilderOpts,
    LocalName, ParseOpts, QualName,
};
use jane_eyre::eyre::{self, bail, OptionExt};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error_span, info, warn};

use crate::summary::{Analysis, Event, Sample};

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let samples = analyse_samples(&args)?;
    let analysis = Analysis { samples };
    let durations_keys = analysis
        .samples
        .iter()
        .flat_map(|s| s.durations.keys())
        .collect::<BTreeSet<_>>();
    for name in durations_keys {
        if let Ok(summary) = analysis.summary(|s| s.durations.get(name).map(|d| d.as_secs_f64())) {
            info!("{name}: {}", summary);
        };
    }

    Ok(())
}

pub fn analyse_samples(args: &[String]) -> eyre::Result<Vec<SampleAnalysis>> {
    let paths = args;

    let mut samples = vec![];
    for (path, result) in paths
        .iter()
        .map(|path| (path.to_owned(), analyse_sample(path)))
        .collect::<Vec<_>>()
    {
        let span = error_span!("analyse", path = path);
        let _enter = span.enter();
        match result {
            Ok(result) => samples.push(result),
            Err(error) => warn!("Failed to analyse file: {error}"),
        }
    }

    Ok(samples)
}

#[tracing::instrument(level = "error")]
fn analyse_sample(path: &str) -> eyre::Result<SampleAnalysis> {
    info!("Analysing sample");

    let mut input = vec![];
    File::open(path)?.read_to_end(&mut input)?;
    let dom = parse(&input)?;

    let mut script = None;
    for node in Traverse::new(dom.document.clone()) {
        match &node.data {
            NodeData::Element { name, .. } => {
                if name == &make_html_tag_name("script") {
                    let kids = node.children.borrow();
                    let Some(kid) = kids.get(0) else {
                        bail!("First <script> has no children")
                    };
                    let NodeData::Text { contents } = &kid.data else {
                        bail!("First <script> has non-#text child")
                    };
                    script = Some(tendril_to_str(&contents.borrow())?.to_owned());
                    break;
                }
            }
            _ => {}
        }
    }

    let Some(json) = script else {
        bail!("Document has no <script>")
    };
    let Some(json) = json.trim().strip_prefix("window.TRACES = [") else {
        bail!("Failed to strip prefix");
    };
    let Some(json) = json.trim().strip_suffix("];") else {
        bail!("Failed to strip suffix");
    };
    let Some(json) = json.trim().strip_suffix(",") else {
        bail!("Failed to strip trailing comma");
    };

    // Discard entries after TimeToInteractive, because Servo requires us to
    // keep the window open for ten whole seconds after TimeToInteractive.
    let all_entries: Vec<TraceEntry> = serde_json::from_str(&format!("[{json}]"))?;
    let mut relevant_entries = vec![];
    for entry in all_entries.iter() {
        relevant_entries.push(entry.clone());
        if entry.category == "TimeToInteractive" {
            break;
        }
    }

    let mut categories = relevant_entries
        .iter()
        .map(|e| e.category.clone())
        .collect::<BTreeSet<_>>();
    let mut durations = BTreeMap::default();

    if !categories.contains("TimeToInteractive") {
        warn!(
            "No entry with category TimeToInteractive! Did you let the page idle for ten seconds?"
        );
    }
    for category in "Compositing LayoutPerform ScriptEvaluate ScriptParseHTML".split(" ") {
        let duration = SampleAnalysis::sum_duration(&relevant_entries, category)?;
        durations.insert(category.to_owned(), duration);
        categories.remove(category);
    }
    for category in "TimeToFirstContentfulPaint TimeToFirstPaint TimeToInteractive".split(" ") {
        if let Some(duration) = SampleAnalysis::unique_offset_duration(&relevant_entries, category)?
        {
            durations.insert(category.to_owned(), duration);
            categories.remove(category);
        }
    }
    for category in categories {
        warn!("Entry has unknown category: {category}");
    }

    Ok(SampleAnalysis {
        dom,
        all_entries,
        relevant_entries,
        durations,
    })
}

fn parse(mut input: &[u8]) -> eyre::Result<RcDom> {
    let options = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let context = QualName::new(None, ns!(html), local_name!("section"));
    let dom = html5ever::parse_fragment(RcDom::default(), options, context, vec![])
        .from_utf8()
        .read_from(&mut input)?;

    Ok(dom)
}

fn make_html_tag_name(name: &str) -> QualName {
    QualName::new(None, ns!(html), LocalName::from(name))
}

fn tendril_to_str(tendril: &StrTendril) -> eyre::Result<&str> {
    Ok(str::from_utf8(tendril.borrow())?)
}

pub struct SampleAnalysis {
    dom: RcDom,
    all_entries: Vec<TraceEntry>,
    relevant_entries: Vec<TraceEntry>,
    durations: BTreeMap<String, Duration>,
}

struct Traverse(Vec<Handle>);

#[derive(Clone, Debug, Deserialize)]
#[allow(non_snake_case)]
struct TraceEntry {
    category: String,
    startTime: usize,
    endTime: usize,
    #[serde(flatten)]
    _rest: BTreeMap<String, Value>,
}

impl Sample for SampleAnalysis {
    fn durations(&self) -> &BTreeMap<String, Duration> {
        &self.durations
    }

    fn events(&self) -> eyre::Result<Vec<Event>> {
        let start = self
            .relevant_entries
            .iter()
            .map(|e| e.startTime)
            .min()
            .ok_or_eyre("No events")?;

        self.relevant_entries
            .iter()
            .map(|e| -> eyre::Result<_> {
                let start = e.startTime - start;
                let duration = e.endTime - e.startTime;
                let duration =
                    (duration != 0).then_some(Duration::from_nanos(duration.try_into()?));
                Ok(Event {
                    name: e.category.clone(),
                    start: Duration::from_nanos(start.try_into()?),
                    duration,
                })
            })
            .collect()
    }
}

impl SampleAnalysis {
    fn sum_duration(entries: &[TraceEntry], category: &str) -> eyre::Result<Duration> {
        let result = entries
            .iter()
            .filter(|e| e.category == category)
            .map(|e| e.duration())
            .collect::<eyre::Result<Vec<_>>>()?;

        Ok(result.iter().sum())
    }

    fn unique_offset_duration(
        entries: &[TraceEntry],
        category: &str,
    ) -> eyre::Result<Option<Duration>> {
        let Some(first_entry) = entries.get(0) else {
            bail!("No entries")
        };
        let matching_entries = entries
            .iter()
            .filter(|e| e.category == category)
            .collect::<Vec<_>>();
        let entry = match matching_entries[..] {
            [] => return Ok(None),
            [entry] => entry,
            _ => bail!("Expected exactly one entry with category {category}"),
        };

        Ok(Some(Duration::from_nanos(
            (entry.startTime - first_entry.startTime).try_into()?,
        )))
    }
}

impl Traverse {
    pub fn new(node: Handle) -> Self {
        Self(vec![node])
    }
}

impl Iterator for Traverse {
    type Item = Handle;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }

        let node = self.0.remove(0);
        for kid in node.children.borrow().iter() {
            self.0.push(kid.clone());
        }

        Some(node)
    }
}

impl TraceEntry {
    fn duration(&self) -> eyre::Result<Duration> {
        Ok(Duration::from_nanos(
            (self.endTime - self.startTime).try_into()?,
        ))
    }
}
