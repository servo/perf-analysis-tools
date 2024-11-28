use std::{collections::BTreeMap, fs::File, path::Path};

use jane_eyre::eyre::{self, OptionExt};
use tracing::info;

use crate::{
    study::{Engine, KeyedCpuConfig, KeyedEngine, KeyedSite, Study},
    summary::{JsonSummaries, JsonSummary, Summary},
};

static USER_FACING_PAINT_METRICS: &str = "FP FCP";
static REAL_SERVO_EVENTS: &str = "Compositing LayoutPerform ScriptEvaluate ScriptParseHTML";
static REAL_CHROMIUM_EVENTS: &str = "EvaluateScript FunctionCall Layerize Layout Paint ParseHTML PrePaint TimerFire UpdateLayoutTree";
static RENDERING_PHASES_MODEL_EVENTS: &str = "Parse Script Layout Rasterise";
static OVERALL_RENDERING_TIME_MODEL_EVENTS: &str = "Renderer";

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let study_dir = Path::new(
        args.iter()
            .nth(0)
            .expect("Usage: analyse <studies/example>"),
    );
    let study = Study::load(study_dir.join("study.toml"))?;

    // Change working directory to the study directory.
    // We need this for `traceconv_command` and `isolate_cpu_command`.
    std::env::set_current_dir(study_dir)?;

    let mut real_events_map = BTreeMap::default();
    let mut synthetic_and_interpreted_events_map = BTreeMap::default();
    for cpu_config in study.cpu_configs() {
        for site in study.sites() {
            for engine in study.engines() {
                let summaries = load_summaries(cpu_config, site, engine)?;
                real_events_map.insert(
                    (cpu_config.key, site.key, engine.key),
                    summaries.real_events,
                );
                synthetic_and_interpreted_events_map.insert(
                    (cpu_config.key, site.key, engine.key),
                    summaries.synthetic_and_interpreted_events,
                );
            }
        }
    }

    // Print sections for user-facing paint metrics.
    for summary_key in USER_FACING_PAINT_METRICS.split(" ") {
        println!("### {summary_key} (synthetic)\n");
        print_section(&study, summary_key, &synthetic_and_interpreted_events_map)?;
    }

    // If there were any Servo results, print sections for real Servo events.
    if study
        .engines()
        .find(|engine| matches!(engine.engine, Engine::Servo { .. }))
        .is_some()
    {
        for summary_key in REAL_SERVO_EVENTS.split(" ") {
            println!("### {summary_key} (real)\n");
            print_section(&study, summary_key, &real_events_map)?;
        }
    }

    // If there were any Chromium results, print sections for real Chromium events.
    if study
        .engines()
        .find(|engine| {
            matches!(
                engine.engine,
                Engine::Chromium { .. } | Engine::ChromeDriver { .. },
            )
        })
        .is_some()
    {
        for summary_key in REAL_CHROMIUM_EVENTS.split(" ") {
            println!("### {summary_key} (real)\n");
            print_section(&study, summary_key, &real_events_map)?;
        }
    }

    // Print sections for rendering phases model.
    for summary_key in RENDERING_PHASES_MODEL_EVENTS.split(" ") {
        println!("### {summary_key} (synthetic)\n");
        print_section(&study, summary_key, &synthetic_and_interpreted_events_map)?;
    }

    // Print sections for overall rendering time model.
    for summary_key in OVERALL_RENDERING_TIME_MODEL_EVENTS.split(" ") {
        println!("### {summary_key} (synthetic)\n");
        print_section(&study, summary_key, &synthetic_and_interpreted_events_map)?;
    }

    Ok(())
}

#[tracing::instrument(level = "error", skip(cpu_config, site, engine), fields(cpu_config = cpu_config.key, site = site.key, engine = engine.key))]
fn load_summaries(
    cpu_config: KeyedCpuConfig<'_>,
    site: KeyedSite<'_>,
    engine: KeyedEngine<'_>,
) -> eyre::Result<JsonSummaries> {
    info!("Loading summaries.json");
    let sample_dir = Path::new(cpu_config.key)
        .join(site.key)
        .join(engine.key)
        .join("summaries.json");

    Ok(serde_json::from_reader(File::open(&sample_dir)?)?)
}

fn print_section(
    study: &Study,
    summary_key: &str,
    summaries_map: &BTreeMap<(&str, &str, &str), Vec<JsonSummary>>,
) -> eyre::Result<()> {
    for site in study.sites() {
        println!("#### {}\n", site.key);
        println!("<table>");
        println!("<tr>");
        println!("<th colspan=2>");
        for cpu_config in study.cpu_configs() {
            println!("<th>{}", cpu_config.key);
        }
        let list: &[(&str, Box<dyn Fn(&Summary<_>) -> String>)] = &[
            ("n", Box::new(|s| s.fmt_n())),
            ("μ", Box::new(|s| s.fmt_mean())),
            ("s", Box::new(|s| s.fmt_stdev())),
            ("min", Box::new(|s| s.fmt_min())),
            ("max", Box::new(|s| s.fmt_max())),
        ];
        for (statistic_label, statistic_getter) in list {
            // Count the actual number of rows we will need, for rowspan.
            let mut rowspan = 0;
            for engine in study.engines() {
                for cpu_config in study.cpu_configs() {
                    let summaries = summaries_map
                        .get(&(cpu_config.key, site.key, engine.key))
                        .ok_or_eyre("Vec<JsonSummary> not found")?;
                    if summaries
                        .iter()
                        .find(|summary| summary.name == summary_key)
                        .is_some()
                    {
                        rowspan += 1;
                    }
                }
            }
            let mut need_statistic_label = true;
            for engine in study.engines() {
                // Loop and break to print a `<tr>` and `<th>` only when `summary_key` is applicable to `engine`.
                for cpu_config in study.cpu_configs() {
                    let summaries = summaries_map
                        .get(&(cpu_config.key, site.key, engine.key))
                        .ok_or_eyre("Vec<JsonSummary> not found")?;
                    if summaries
                        .iter()
                        .find(|summary| summary.name == summary_key)
                        .is_some()
                    {
                        println!("<tr>");
                        if need_statistic_label {
                            println!("<th rowspan={rowspan}>{statistic_label}");
                        }
                        println!("<th>{}", engine.key);
                        need_statistic_label = false;
                        break;
                    }
                }
                // Now print the data for that row.
                for cpu_config in study.cpu_configs() {
                    let summaries = summaries_map
                        .get(&(cpu_config.key, site.key, engine.key))
                        .ok_or_eyre("Vec<JsonSummary> not found")?;
                    if let Some(summary) =
                        summaries.iter().find(|summary| summary.name == summary_key)
                    {
                        println!(
                            "<td title='{}'>{}",
                            summary.full,
                            statistic_getter(&summary.raw)
                        );
                    }
                }
            }
        }
        println!("</table>\n");
    }

    Ok(())
}
