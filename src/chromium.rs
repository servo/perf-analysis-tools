use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    time::Duration,
};

use jane_eyre::eyre::{self, bail, OptionExt};
use tracing::{debug, error_span, info, trace, warn};

use crate::{
    json::{JsonTrace, TraceEvent},
    summary::{Analysis, Event, Sample, SYNTHETIC_NAMES},
};

static RENDERER_NAMES: &'static str = "ParseHTML EvaluateScript FunctionCall TimerFire UpdateLayoutTree Layout PrePaint Paint Layerize"; // TODO: does not include rasterisation and compositing
static PARSE_NAMES: &'static str = "ParseHTML";
static SCRIPT_NAMES: &'static str = "EvaluateScript FunctionCall TimerFire";
static LAYOUT_NAMES: &'static str = "UpdateLayoutTree Layout PrePaint Paint";
static RASTERISE_NAMES: &'static str = "Layerize"; // TODO: does not include rasterisation and compositing
static METRICS: &'static [(&'static str, &'static str)] =
    &[("FP", "firstPaint"), ("FCP", "firstContentfulPaint")];

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
            let Ok(events) = s.synthetic_events() else {
                warn!("Failed to get synthetic events");
                return None;
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

#[tracing::instrument(level = "error", skip(url))]
fn analyse_sample(url: &str, path: &str) -> eyre::Result<SampleAnalysis> {
    info!("Analysing sample");

    let mut json = String::default();
    File::open(path)?.read_to_string(&mut json)?;
    let mut all_events = serde_json::from_str::<JsonTrace>(&json)?.traceEvents;
    all_events.sort_by(|p, q| p.ts.cmp(&q.ts).then(p.dur.cmp(&q.dur)));

    let (navigation_id, frame) = all_events
        .iter()
        .find(|e| e.document_loader_url() == Some(&url))
        .ok_or_eyre("Failed to find event with the given documentLoaderURL")
        .map(|e| e.navigation_id().zip(e.frame()))?
        .ok_or_eyre("Event with the given documentLoaderURL has no navigationId and/or frame")?;
    trace!("navigation_id = {navigation_id}");
    trace!("frame = {frame}");

    let relevant_events = all_events
        .iter()
        .filter(|e| e.navigation_id() == Some(navigation_id) || e.frame() == Some(frame))
        .collect::<Vec<_>>();

    let indices_by_event_name = relevant_events
        .iter()
        .map(|e| {
            (
                &*e.name,
                relevant_events
                    .iter()
                    .enumerate()
                    .filter(|(_, e2)| e2.name == e.name)
                    .map(|(i, _)| i)
                    .collect(),
            )
        })
        .collect::<BTreeMap<&str, Vec<usize>>>();

    // Remove first occurrences of events with certain names.
    let is_duplicated_event_name = |name: &str| {
        "navigationStart responseEnd domLoading domInteractive domContentLoadedEventStart domContentLoadedEventEnd domComplete"
            .split(" ")
            .find(|&d| d == name)
            .is_some()
    };
    let relevant_events = relevant_events
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            !is_duplicated_event_name(&e.name) || *i != indices_by_event_name[&*e.name][0]
        })
        .map(|(_, e)| e)
        .collect::<Vec<_>>();

    let mut result = vec![];
    let ts_start = relevant_events[0].ts;
    for &event in relevant_events {
        let ts = event.ts - ts_start;
        if let Some(dur) = event.dur {
            debug!("{} +{} {} {:?} {}", ts, dur, event.ph, event.s, event.name);
        } else {
            debug!("{} {} {:?} {}", ts, event.ph, event.s, event.name);
        }
        trace!("{:?}", event);
        result.push(event.to_owned());
    }

    let mut durations = BTreeMap::default();
    let interesting_event_names = format!("{RENDERER_NAMES}");
    for name in interesting_event_names.split(" ") {
        let duration = SampleAnalysis::sum_duration(&result, name)?;
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
    relevant_events: Vec<TraceEvent>,
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
            .map(|e| e.ts)
            .min()
            .ok_or_eyre("No events")?;

        let result = self.relevant_events
            .iter()
            .filter(|e| "PaintTimingVisualizer::LayoutObjectPainted ResourceSendRequest ResourceReceivedData ResourceReceiveResponse".split(" ").find(|&name| name == e.name).is_none())
            .map(|e| -> eyre::Result<_> {
                let start = e.ts - start;
                let duration = match e.dur {
                    Some(dur) => Some(Duration::from_micros(dur.try_into()?)),
                    None => None,
                };
                Ok(Event {
                    name: e.name.clone(),
                    start: Duration::from_micros(start.try_into()?),
                    duration,
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
            .map(|e| e.ts)
            .min()
            .ok_or_eyre("No events")?;
        let start = Duration::from_micros(start.try_into()?);

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
        // “loading” category events like `firstPaint` and `firstContentfulPaint` are timed from `markAsMainFrame`.
        // <https://codereview.chromium.org/2712773002>
        for (result_name, stop_name) in METRICS {
            if let Ok(mut event) = SampleAnalysis::unique_instantaneous_event_from(
                &self.relevant_events,
                result_name,
                "markAsMainFrame",
                stop_name,
            ) {
                event.start -= start;
                result.push(event);
            }
        }

        Ok(result)
    }
}

impl SampleAnalysis {
    fn sum_duration(relevant_events: &[TraceEvent], name: &str) -> eyre::Result<Duration> {
        let result = Self::dur_by_name(relevant_events, name)
            .iter()
            .sum::<usize>();

        Ok(Duration::from_micros(result.try_into()?))
    }

    fn unique_instantaneous_event_from(
        relevant_events: &[TraceEvent],
        result_name: &str,
        start_name: &str,
        stop_name: &str,
    ) -> eyre::Result<Event> {
        let [start_ts] = Self::ts_by_name(relevant_events, start_name)[..] else {
            bail!("Expected exactly one event with name {start_name}");
        };
        let [stop_ts] = Self::ts_by_name(relevant_events, stop_name)[..] else {
            bail!("Expected exactly one event with name {stop_name}");
        };

        let start = Duration::from_micros(start_ts.try_into()?);
        let duration = Duration::from_micros(u64::try_from(stop_ts)? - u64::try_from(start_ts)?);

        Ok(Event {
            name: result_name.to_owned(),
            start,
            duration: Some(duration),
        })
    }

    fn dur_by_name(relevant_events: &[TraceEvent], name: &str) -> Vec<usize> {
        relevant_events
            .iter()
            .filter(|e| e.name == name)
            .filter_map(|e| e.dur)
            .collect()
    }

    fn ts_by_name(relevant_events: &[TraceEvent], name: &str) -> Vec<usize> {
        relevant_events
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.ts)
            .collect()
    }
}

impl TraceEvent {
    fn document_loader_url(&self) -> Option<&str> {
        self.args
            .get("data")
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("documentLoaderURL"))
            .and_then(|v| v.as_str())
    }

    fn navigation_id(&self) -> Option<&str> {
        self.args
            .get("data")
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("navigationId"))
            .and_then(|v| v.as_str())
    }

    fn frame(&self) -> Option<&str> {
        // Many events use .args.frame,
        // but “Paint” events use .args.data.frame,
        // and “Layout” events use .args.beginData.frame.
        self.args
            .get("data")
            .or(self.args.get("beginData"))
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("frame"))
            .or(self.args.get("frame"))
            .and_then(|v| v.as_str())
    }
}
