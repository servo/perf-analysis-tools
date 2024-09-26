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
    summary::{Analysis, Event, Sample},
};

static PARSE_NAMES: &'static str = "ParseHTML";
static SCRIPT_NAMES: &'static str = "EvaluateScript FunctionCall TimerFire";
static LAYOUT_NAMES: &'static str = "UpdateLayoutTree Layout PrePaint Paint Layerize";
static METRICS: &'static [(&'static str, &'static str)] =
    &[("FP", "firstPaint"), ("FCP", "firstContentfulPaint")];

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let samples = analyse_samples(&args)?;
    let analysis = Analysis { samples };
    info!(
        "Total: {}",
        analysis.summary(|s| s.total_duration.as_secs_f64())?
    );
    info!(
        "Fetching: {}",
        analysis.summary(|s| s.fetching_duration.as_secs_f64())?
    );
    info!(
        "Navigation: {}",
        analysis.summary(|s| s.navigation_duration.as_secs_f64())?
    );

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

    let result = SampleAnalysisRequest {
        relevant_events: result,
        ts_start,
    };

    let total = SampleAnalysis::unique_instantaneous_event_from(
        &result.relevant_events,
        "*Total",
        "fetchStart",
        "loadEventEnd",
    )?;
    let fetching = SampleAnalysis::unique_instantaneous_event_from(
        &result.relevant_events,
        "*Fetching",
        "fetchStart",
        "responseEnd",
    )?;
    let navigation = SampleAnalysis::unique_instantaneous_event_from(
        &result.relevant_events,
        "*Navigation",
        "navigationStart",
        "commitNavigationEnd",
    )?;
    let total_duration = total.duration.ok_or_eyre("Event has no duration")?;
    let fetching_duration = fetching.duration.ok_or_eyre("Event has no duration")?;
    let navigation_duration = navigation.duration.ok_or_eyre("Event has no duration")?;
    debug!("Total: {:?}", total_duration);
    debug!("Fetching: {:?}", fetching_duration);
    debug!("Navigation: {:?}", navigation_duration);

    // “loading” category events are timed from markAsMainFrame.
    let mut durations = BTreeMap::default();
    let loading_event_names = SampleAnalysis::loading_events(&result.relevant_events)
        .map(|e| &*e.name)
        .collect::<BTreeSet<_>>();
    for name in loading_event_names {
        if name == "markAsMainFrame" {
            continue;
        }
        if let Ok(event) = SampleAnalysis::unique_instantaneous_event_from(
            &result.relevant_events,
            name,
            "markAsMainFrame",
            name,
        ) {
            let duration = event.duration.ok_or_eyre("Event has no duration")?;
            debug!("{name}: {:?}", duration);
            durations.insert(name.to_owned(), duration);
        }
    }

    let other_event_names =
        "ParseHTML EvaluateScript UpdateLayoutTree Layout PrePaint Paint Layerize".split(" ");
    for name in other_event_names {
        let duration = SampleAnalysis::sum_duration(&result.relevant_events, name)?;
        debug!("{name}: {:?}", duration);
        durations.insert(name.to_owned(), duration);
    }

    let result = SampleAnalysis {
        path: path.to_owned(),
        navigation_id: navigation_id.to_owned(),
        frame: frame.to_owned(),
        all_events,
        relevant_events: result.relevant_events,
        ts_start: result.ts_start,
        total_duration,
        fetching_duration,
        navigation_duration,
        durations,
    };

    Ok(result)
}

pub struct SampleAnalysis {
    path: String,
    navigation_id: String,
    frame: String,
    all_events: Vec<TraceEvent>,
    relevant_events: Vec<TraceEvent>,
    ts_start: usize,
    total_duration: Duration,
    fetching_duration: Duration,
    navigation_duration: Duration,
    durations: BTreeMap<String, Duration>,
}

struct SampleAnalysisRequest {
    relevant_events: Vec<TraceEvent>,
    ts_start: usize,
}

impl Sample for SampleAnalysis {
    fn path(&self) -> &str {
        &self.path
    }

    fn durations(&self) -> &BTreeMap<String, Duration> {
        &self.durations
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
        let mut result = [
            Event::generate_merged_events(parse_events, "Parse")?,
            Event::generate_merged_events(script_events, "Script")?,
            Event::generate_merged_events(layout_events, "Layout")?,
            // TODO: paint events?
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
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
    fn loading_events(relevant_events: &[TraceEvent]) -> impl Iterator<Item = &TraceEvent> {
        relevant_events.iter().filter(|e| e.has_category("loading"))
    }

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

    fn has_category(&self, category: &str) -> bool {
        self.cat.split(",").find(|&cat| cat == category).is_some()
    }
}
