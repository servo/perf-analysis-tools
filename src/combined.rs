use jane_eyre::eyre::{self, bail};
use serde_json::json;

use crate::{
    json::{JsonTrace, TraceEvent},
    summary::{Analysis, Sample},
};

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let mut names = vec![];
    let mut analyses = vec![];

    for args in args.split(|arg| arg == "--") {
        let mode = &args[0];
        let args = &args[1..];
        names.push(format!("{mode} {}", analyses.len()));

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

        let analysis = Analysis { samples };
        analyses.push(analysis);
    }

    let mut events = vec![];
    for (i, (analysis, name)) in analyses.into_iter().zip(names).enumerate() {
        events.push(TraceEvent {
            ph: "M".to_owned(),
            name: "process_name".to_owned(),
            cat: "__metadata".to_owned(),
            pid: i,
            args: [("name".to_owned(), json!(name))].into_iter().collect(),
            ..Default::default()
        });
        for (j, sample) in analysis.samples.into_iter().enumerate() {
            events.push(TraceEvent {
                ph: "M".to_owned(),
                name: "thread_name".to_owned(),
                cat: "__metadata".to_owned(),
                pid: i,
                tid: j,
                args: [("name".to_owned(), json!(format!("Sample {j}")))]
                    .into_iter()
                    .collect(),
                ..Default::default()
            });
            for event in sample.events()? {
                events.push(TraceEvent {
                    ts: event.start.as_micros().try_into()?,
                    // tts: Some(event.start.as_micros().try_into()?),
                    dur: match event.duration {
                        Some(dur) => Some(dur.as_micros().try_into()?),
                        None => None,
                    },
                    // tdur: match event.duration {
                    //     Some(dur) => Some(dur.as_micros().try_into()?),
                    //     None => None,
                    // },
                    ph: "X".to_owned(),
                    name: event.name,
                    cat: "content".to_owned(),
                    pid: i,
                    tid: j,
                    ..Default::default()
                });
            }
        }
    }

    let trace = JsonTrace {
        traceEvents: events,
    };
    println!("{}", serde_json::to_string(&trace)?);

    Ok(())
}
