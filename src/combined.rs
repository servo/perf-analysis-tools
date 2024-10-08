use std::path::Path;

use jane_eyre::eyre::{self, bail, OptionExt};
use serde_json::json;

use crate::{
    json::{JsonTrace, TraceEvent},
    summary::{Analysis, Event, Sample},
};

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let mut names = vec![];
    let mut analyses = vec![];
    let mut longest_path_prefix: Option<String> = None;

    for args in args.split(|arg| arg == "--") {
        let mode = &args[0];
        let args = &args[1..];
        names.push(format!("{mode} (command {})", analyses.len()));

        let samples = match &**mode {
            // Usage: analyse servo <trace.html ...>
            "servo" => crate::servo::analyse_samples(&args)?
                .into_iter()
                .map(|s| Box::new(s) as Box<dyn Sample>)
                .collect::<Vec<_>>(),
            // Usage: analyse chromium <page url> <chrome.json ...>
            "chromium" => crate::chromium::analyse_samples(&args)?
                .into_iter()
                .map(|s| Box::new(s) as Box<dyn Sample>)
                .collect::<Vec<_>>(),
            other => bail!("Unknown command: {other}"),
        };

        for sample in samples.iter() {
            let path = Path::new(sample.path()).canonicalize()?;
            let path = path
                .to_str()
                .ok_or_eyre("Failed to convert PathBuf to str")?;
            longest_path_prefix = if let Some(prefix) = longest_path_prefix {
                let mut new_prefix = &*prefix;
                while path.strip_prefix(new_prefix).is_none() {
                    let mut index = new_prefix.len() - 1;
                    while !new_prefix.is_char_boundary(index) {
                        index -= 1;
                    }
                    new_prefix = &new_prefix[..index];
                }
                Some(new_prefix.to_owned())
            } else {
                Some(path.to_owned())
            };
        }

        let analysis = Analysis { samples };
        analyses.push(analysis);
    }

    let longest_path_prefix = longest_path_prefix.ok_or_eyre("No longest path prefix")?;
    let mut events = vec![];
    // For each analysis given, create a “process”.
    for (i, (analysis, name)) in analyses.into_iter().zip(names).enumerate() {
        events.push(TraceEvent {
            ph: "M".to_owned(),
            name: "process_name".to_owned(),
            cat: "__metadata".to_owned(),
            pid: i,
            args: [("name".to_owned(), json!(name))].into_iter().collect(),
            ..Default::default()
        });
        // For each of its samples, create two “threads”, one for synthetic events and one for real events.
        for (j, sample) in analysis.samples.into_iter().enumerate() {
            // Strip the longest path prefix across all samples and all commands, for brevity in Perfetto UI.
            let path = Path::new(sample.path()).canonicalize()?;
            let path = path
                .to_str()
                .ok_or_eyre("Failed to convert PathBuf to str")?;
            let Some(path) = path.strip_prefix(&longest_path_prefix) else {
                bail!("Failed to strip longest path prefix")
            };

            struct TraceRow {
                id: usize,
                name: String,
                events: Vec<Event>,
            }
            for row in [
                TraceRow {
                    id: j * 2 + 0,
                    name: format!("{path} (real)"),
                    events: sample.real_events()?,
                },
                TraceRow {
                    id: j * 2 + 1,
                    name: format!("{path} (synthetic)"),
                    events: sample.synthetic_events()?,
                },
            ] {
                events.push(TraceEvent {
                    ph: "M".to_owned(),
                    name: "thread_name".to_owned(),
                    cat: "__metadata".to_owned(),
                    pid: i,
                    tid: row.id,
                    args: [("name".to_owned(), json!(row.name))].into_iter().collect(),
                    ..Default::default()
                });
                for event in row.events {
                    events.push(TraceEvent {
                        ts: event.start.as_micros().try_into()?,
                        dur: match event.duration {
                            Some(dur) => Some(dur.as_micros().try_into()?),
                            None => None,
                        },
                        ph: if event.duration.is_some() {
                            "X".to_owned()
                        } else {
                            "I".to_owned()
                        },
                        s: Some("t".to_owned()),
                        name: event.name,
                        cat: "content".to_owned(),
                        pid: i,
                        tid: row.id,
                        ..Default::default()
                    });
                }
            }
        }
    }

    let trace = JsonTrace {
        traceEvents: events,
    };
    println!("{}", serde_json::to_string(&trace)?);

    Ok(())
}
