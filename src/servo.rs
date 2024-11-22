use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    time::Duration,
};

use jane_eyre::eyre::{self, bail, OptionExt};
use perfetto_protos::{
    trace::Trace,
    trace_packet::trace_packet::Data,
    track_event::{track_event, TrackEvent},
};
use protobuf::Message;
use serde_json::json;
use tracing::{debug, error_span, info, trace, warn};

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

    let mut real_events = vec![];
    let mut synthetic_and_interpreted_events = vec![];

    for name in durations_keys {
        if let Ok(summary) = analysis.summary(|s| s.durations.get(name).map(|d| d.as_secs_f64())) {
            real_events.push(summary.to_json(name));
        };
    }

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
            synthetic_and_interpreted_events.push(summary.to_json(synthetic_name));
        }
    }

    println!(
        "{}",
        json! ({
            "real_events": real_events,
            "synthetic_and_interpreted_events": synthetic_and_interpreted_events,
        })
        .to_string()
    );
    println!();
    println!(">>> Real events");
    for summary in real_events {
        println!(
            "{}: {} ({})",
            summary.name, summary.representative, summary.full
        );
    }
    println!();
    println!(">>> Synthetic and interpreted events");
    for summary in synthetic_and_interpreted_events {
        println!(
            "{}: {} ({})",
            summary.name, summary.representative, summary.full
        );
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

    // Tracks can have slices, instants, and counters. Slices must have stack-like behaviour within
    // a track, so we can use a stack to find pairs and merge them together.
    let mut tracks: HashMap<u64, Vec<PendingSlice>> = HashMap::default();
    struct PendingSlice {
        start: u64,
        event: TrackEvent,
    }

    let mut all_events = vec![];
    for mut packet in Trace::parse_from_reader(&mut File::open(path)?)?.packet {
        // Assume the default clock (1ns, absolute).
        assert!(packet.timestamp_clock_id.is_none());

        match packet.data.take().ok_or_eyre("TracePacket has no data")? {
            Data::TrackEvent(event) => {
                let slice_stack = tracks.entry(event.track_uuid()).or_default();
                match event.type_() {
                    track_event::Type::TYPE_SLICE_BEGIN => {
                        slice_stack.push(PendingSlice {
                            start: packet.timestamp(),
                            event,
                        });
                    }
                    track_event::Type::TYPE_SLICE_END => {
                        let slice = slice_stack
                            .pop()
                            .ok_or_eyre("Slice stack for track is empty")?;
                        if event.has_name() {
                            assert_eq!(event.name(), slice.event.name());
                        }
                        let event = Event {
                            name: slice.event.name().to_owned(),
                            start: Duration::from_nanos(slice.start),
                            duration: Some(Duration::from_nanos(packet.timestamp() - slice.start)),
                            metadata: slice
                                .event
                                .debug_annotations
                                .into_iter()
                                .map(|a| (a.name().to_owned(), a))
                                .collect(),
                        };
                        all_events.push(event);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    all_events.sort_by(|p, q| p.start.cmp(&q.start).then(p.duration.cmp(&q.duration)));

    let relevant_events = all_events
        .iter()
        .filter(|e| {
            // Ignore any entries with the wrong .metadata.url, since they are for other iframes.
            // Categories in NO_URL_NAMES have no .metadata.url.
            e.metadata
                .get("url")
                .is_some_and(|v| v.string_value() == url)
                || NO_URL_NAMES.split(" ").find(|&n| n == e.name).is_some()
        })
        .collect::<Vec<_>>();

    let mut result = vec![];
    let start_timestamp = relevant_events[0].start;
    for event in relevant_events {
        let new_timestamp = event.start - start_timestamp;
        if let Some(dur) = event.duration {
            debug!("{:?} +{:?} {}", new_timestamp, dur, event.name);
        } else {
            debug!("{:?} {}", new_timestamp, event.name);
        }
        trace!("{:?}", event);
        result.push(event.to_owned());
    }

    let mut durations = BTreeMap::default();
    let interesting_event_names = format!("{RENDERER_NAMES}");
    for name in interesting_event_names.split(" ") {
        let duration = SampleAnalysis::sum_duration(&result, name);
        debug!("{name}: {:?}", duration);
        durations.insert(name.to_owned(), duration);
    }

    let result = SampleAnalysis {
        path: path.to_owned(),
        relevant_events: result,
        durations,
    };

    Ok(result)
}

pub struct SampleAnalysis {
    path: String,
    relevant_events: Vec<Event>,
    durations: BTreeMap<String, Duration>,
}

impl Sample for SampleAnalysis {
    fn path(&self) -> &str {
        &self.path
    }

    fn real_events(&self) -> eyre::Result<Vec<Event>> {
        let start = self
            .relevant_events
            .iter()
            .map(|e| e.start)
            .min()
            .ok_or_eyre("No events")?;

        let result = self
            .relevant_events
            .iter()
            .map(|e| -> eyre::Result<_> {
                let start = e.start - start;
                Ok(Event {
                    name: e.name.clone(),
                    start,
                    duration: e.duration,
                    metadata: e.metadata.clone(),
                })
            })
            .collect::<eyre::Result<Vec<_>>>()?;

        Ok(result)
    }

    fn synthetic_events(&self) -> eyre::Result<Vec<Event>> {
        let real_events = self.real_events()?;
        let start = self
            .relevant_events
            .iter()
            .map(|e| e.start)
            .min()
            .ok_or_eyre("No events")?;

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
                &self.relevant_events,
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
    fn sum_duration(relevant_events: &[Event], name: &str) -> Duration {
        Self::dur_by_name(relevant_events, name).iter().sum()
    }

    fn unique_instantaneous_event_from_first_parse(
        relevant_events: &[Event],
        result_name: &str,
        name: &str,
    ) -> eyre::Result<Option<Event>> {
        let Some(first_parse_event) = relevant_events.iter().find(|e| e.name == "ScriptParseHTML")
        else {
            bail!("No events with category ScriptParseHTML")
        };
        let matching_events = relevant_events
            .iter()
            .filter(|e| e.name == name)
            .collect::<Vec<_>>();
        let event = match matching_events[..] {
            [] => return Ok(None),
            [event] => event,
            _ => bail!("Expected exactly one event with name {name}"),
        };
        if event.duration.is_some() {
            bail!("Entry is not instantaneous");
        }
        if event.start < first_parse_event.start {
            bail!("Entry is earlier than first ScriptParseHTML event");
        }

        Ok(Some(Event {
            name: result_name.to_owned(),
            start: first_parse_event.start,
            duration: Some(event.start - first_parse_event.start),
            metadata: event.metadata.clone(),
        }))
    }

    fn dur_by_name(relevant_events: &[Event], name: &str) -> Vec<Duration> {
        relevant_events
            .iter()
            .filter(|e| e.name == name)
            .filter_map(|e| e.duration)
            .collect()
    }
}
