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

use crate::summary::{Analysis, Event, Sample, SYNTHETIC_NAMES};

static RENDERER_NAMES: &'static str = "ScriptParseHTML ScriptEvaluate LayoutPerform Compositing";
static PARSE_NAMES: &'static str = "ScriptParseHTML";
static SCRIPT_NAMES: &'static str = "ScriptEvaluate";
static LAYOUT_NAMES: &'static str = "LayoutPerform";
static RASTERISE_NAMES: &'static str = "Compositing";
static NO_URL_NAMES: &'static str = "Compositing IpcReceiver";
static METRICS: &'static [(&'static str, &'static str)] = &[
    ("FP", "TimeToFirstPaint"),
    ("FCP", "TimeToFirstContentfulPaint"),
    ("TTI", "TimeToInteractive"),
];

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let samples = analyse_samples(&args)?;
    let analysis = Analysis { samples };
    let durations_keys = analysis
        .samples
        .iter()
        .flat_map(|s| s.durations.keys())
        .collect::<BTreeSet<_>>();
    println!(">>> Real events");
    for name in durations_keys {
        if let Ok(summary) = analysis.summary(|s| s.durations.get(name).map(|d| d.as_secs_f64())) {
            println!("{name}: {}", summary);
        };
    }
    println!(">>> Synthetic and interpreted events");
    for synthetic_name in SYNTHETIC_NAMES.split(" ") {
        if let Ok(summary) = analysis.summary(|s| {
            let events = match s.synthetic_events() {
                Ok(events) => events,
                Err(error) => {
                    warn!(?error, "Failed to get synthetic events");
                    return None;
                }
            };
            let result = events
                .iter()
                .filter(|e| e.name == synthetic_name)
                .flat_map(|e| e.duration.map(|d| d.as_secs_f64()))
                .sum::<f64>();
            Some(result)
        }) {
            println!("{synthetic_name}: {}", summary);
        }
    }

    Ok(())
}

pub fn analyse_samples(args: &[String]) -> eyre::Result<Vec<SampleAnalysis>> {
    let url = args.iter().nth(0).unwrap().to_owned();
    let paths = args.into_iter().skip(1).collect::<Vec<_>>();

    let mut samples = vec![];
    for (path, result) in paths
        .iter()
        .map(|path| (path.to_owned(), analyse_sample(&url, path)))
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
fn analyse_sample(url: &str, path: &str) -> eyre::Result<SampleAnalysis> {
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
    let json = json.trim();
    // If we automate Servo with SIGTERM, the HTML file will be truncated after the last trace
    // entry, so if we can’t find the bracket etc, assume it’s just the end of the file.
    let json = json.strip_suffix("];").unwrap_or(json);
    let Some(json) = json.trim().strip_suffix(",") else {
        bail!("Failed to strip trailing comma");
    };

    let mut all_entries: Vec<TraceEntry> = serde_json::from_str(&format!("[{json}]"))?;
    all_entries.sort_by(|p, q| {
        p.startTime
            .cmp(&q.startTime)
            .then(p.endTime.cmp(&q.endTime))
    });
    let relevant_entries = all_entries
        .iter()
        .filter(|e| {
            // Ignore any entries with the wrong .metadata.url, since they are for other iframes.
            // Categories in NO_URL_NAMES have no .metadata.url.
            e.metadata.as_ref().is_some_and(|m| m.url == url)
                || NO_URL_NAMES.split(" ").find(|&n| n == e.category).is_some()
        })
        .cloned()
        .collect::<Vec<_>>();
    if relevant_entries
        .iter()
        .find(|e| e.metadata.as_ref().is_some_and(|m| m.url == url))
        .is_none()
    {
        bail!("No entry with matching .metadata.url! Skipping sample");
    }

    let mut durations = BTreeMap::default();
    let interesting_categories = format!("{RENDERER_NAMES}");
    for category in interesting_categories.split(" ") {
        let duration = SampleAnalysis::sum_duration(&relevant_entries, category)?;
        durations.insert(category.to_owned(), duration);
    }

    Ok(SampleAnalysis {
        path: path.to_owned(),
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
    path: String,
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
    metadata: Option<Metadata>,
    #[serde(flatten)]
    _rest: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[allow(non_snake_case)]
struct Metadata {
    url: String,
    #[serde(flatten)]
    _rest: BTreeMap<String, Value>,
}

impl Sample for SampleAnalysis {
    fn path(&self) -> &str {
        &self.path
    }

    fn real_events(&self) -> eyre::Result<Vec<Event>> {
        let start = self
            .relevant_entries
            .iter()
            .map(|e| e.startTime)
            .min()
            .ok_or_eyre("No events")?;

        let result = self
            .relevant_entries
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
            .collect::<eyre::Result<Vec<_>>>()?;

        Ok(result)
    }

    fn synthetic_events(&self) -> eyre::Result<Vec<Event>> {
        let real_events = self.real_events()?;
        let start = self
            .relevant_entries
            .iter()
            .map(|e| e.startTime)
            .min()
            .ok_or_eyre("No events")?;
        let start = Duration::from_nanos(start.try_into()?);

        // Add some synthetic events with our interpretations.
        let renderer_events = real_events.iter().filter(|e| {
            RENDERER_NAMES
                .split(" ")
                .find(|&name| name == e.name)
                .is_some()
        });
        let parse_events = real_events.iter().filter(|e| {
            PARSE_NAMES
                .split(" ")
                .find(|&name| name == e.name)
                .is_some()
        });
        let script_events = real_events.iter().filter(|e| {
            SCRIPT_NAMES
                .split(" ")
                .find(|&name| name == e.name)
                .is_some()
        });
        let layout_events = real_events.iter().filter(|e| {
            LAYOUT_NAMES
                .split(" ")
                .find(|&name| name == e.name)
                .is_some()
        });
        let rasterise_events = real_events.iter().filter(|e| {
            RASTERISE_NAMES
                .split(" ")
                .find(|&name| name == e.name)
                .is_some()
        });
        let mut result = [
            Event::generate_merged_events(renderer_events, "Renderer")?,
            Event::generate_merged_events(parse_events, "Parse")?,
            Event::generate_merged_events(script_events, "Script")?,
            Event::generate_merged_events(layout_events, "Layout")?,
            Event::generate_merged_events(rasterise_events, "Rasterise")?,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        for (result_name, category) in METRICS {
            if let Some(mut event) = SampleAnalysis::unique_instantaneous_event_from_first_parse(
                &self.relevant_entries,
                result_name,
                category,
            )? {
                event.start -= start;
                result.push(event);
            }
        }

        Ok(result)
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

    fn unique_instantaneous_event_from_first_parse(
        entries: &[TraceEntry],
        result_name: &str,
        category: &str,
    ) -> eyre::Result<Option<Event>> {
        let Some(first_parse_entry) = entries.iter().find(|e| e.category == "ScriptParseHTML")
        else {
            bail!("No entries with category ScriptParseHTML")
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
        if entry.endTime - entry.startTime > 0 {
            bail!("Entry is not instantaneous");
        }
        if entry.startTime < first_parse_entry.startTime {
            bail!("Entry is earlier than first ScriptParseHTML entry");
        }

        let start = Duration::from_nanos(first_parse_entry.startTime.try_into()?);
        let duration =
            Duration::from_nanos((entry.startTime - first_parse_entry.startTime).try_into()?);

        Ok(Some(Event {
            name: result_name.to_owned(),
            start,
            duration: Some(duration),
        }))
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
