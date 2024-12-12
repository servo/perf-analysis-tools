use core::str;
use std::{collections::BTreeMap, fs::File, io::Write, path::Path};

use dataurl::DataUrl;
use jane_eyre::eyre::{self, bail, eyre, OptionExt};
use poloto::{
    num::float::{FloatFmt, FloatTickFmt},
    ticks::{
        tick_fmt::{TickFmt, WithTickFmt},
        IndexRequester, RenderFrameBound, TickDistGen, TickDistribution, TickRes,
    },
};
use rand::Rng;
use tracing::info;

use crate::{
    shell::SHELL,
    study::{Engine, KeyedCpuConfig, KeyedEngine, KeyedSite, Study},
    summary::{fmt_seconds_short, EventKind, JsonRawSeries, JsonSummaries, JsonSummary, Summary},
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
    let mut raw_series_map = BTreeMap::default();
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
                raw_series_map.insert((cpu_config.key, site.key, engine.key), summaries.raw_series);
            }
        }
    }

    // Print the tooling version, engine keys, and engine descriptions.
    // FIXME: Use askama to avoid having to escape HTML manually.
    println!("<ul>");
    let version = SHELL
        .lock()
        .map_err(|e| eyre!("Mutex poisoned: {e:?}"))?
        .run(
            include_str!("../get-tooling-version.sh"),
            Vec::<&str>::default(),
        )?
        .output()?;
    if !version.status.success() {
        bail!("Process failed: {}", version.status);
    }
    let version = str::from_utf8(&version.stdout)?
        .strip_suffix("\n")
        .ok_or_eyre("Output has no trailing newline")?;
    println!(
        r#"<li><a href="https://github.com/servo/perf-analysis-tools">perf-analysis-tools</a> version:"#,
    );
    println!(
        r#"<a href="https://github.com/servo/perf-analysis-tools/commit/{}">{}</a>"#,
        escape_html_for_attribute(version),
        escape_html_for_inner_html(version),
    );
    for engine in study.engines() {
        print!(
            "<li><strong>{}</strong> = ",
            escape_html_for_inner_html(engine.key),
        );
        if let Some(description) = engine.description() {
            // HTML is allowed here.
            println!("{}", description);
        } else {
            println!(
                "<code>{}</code> at <code>{}</code>",
                escape_html_for_inner_html(engine.type_name()),
                escape_html_for_inner_html(engine.browser_path()),
            );
        }
    }
    println!("</ul>");
    println!();

    // Print the study config file.
    println!("<details><summary>study.toml</summary>\n");
    println!(
        "<pre><code>{}</code></pre>",
        escape_html_for_inner_html(&study.source_toml),
    );
    println!("</details>");
    println!();

    // Print sections for user-facing paint metrics.
    for summary_key in USER_FACING_PAINT_METRICS.split(" ") {
        println!("<h3>{summary_key} (synthetic)</h3>\n");
        print_section(
            &study,
            &raw_series_map,
            &synthetic_and_interpreted_events_map,
            EventKind::SyntheticOrInterpreted,
            summary_key,
        )?;
    }

    // If there were any Servo results, print sections for real Servo events.
    if study
        .engines()
        .find(|engine| matches!(engine.engine, Engine::Servo { .. }))
        .is_some()
    {
        for summary_key in REAL_SERVO_EVENTS.split(" ") {
            println!("<h3>{summary_key} (real)</h3>\n");
            print_section(
                &study,
                &raw_series_map,
                &real_events_map,
                EventKind::Servo,
                summary_key,
            )?;
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
            println!("<h3>{summary_key} (real)</h3>\n");
            print_section(
                &study,
                &raw_series_map,
                &real_events_map,
                EventKind::Chromium,
                summary_key,
            )?;
        }
    }

    // Print sections for rendering phases model.
    for summary_key in RENDERING_PHASES_MODEL_EVENTS.split(" ") {
        println!("<h3>{summary_key} (synthetic)</h3>\n");
        print_section(
            &study,
            &raw_series_map,
            &synthetic_and_interpreted_events_map,
            EventKind::SyntheticOrInterpreted,
            summary_key,
        )?;
    }

    // Print sections for overall rendering time model.
    for summary_key in OVERALL_RENDERING_TIME_MODEL_EVENTS.split(" ") {
        println!("<h3>{summary_key} (synthetic)</h3>\n");
        print_section(
            &study,
            &raw_series_map,
            &synthetic_and_interpreted_events_map,
            EventKind::SyntheticOrInterpreted,
            summary_key,
        )?;
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
    raw_series_map: &BTreeMap<(&str, &str, &str), Vec<JsonRawSeries>>,
    summaries_map: &BTreeMap<(&str, &str, &str), Vec<JsonSummary>>,
    event_kind: EventKind,
    summary_key: &str,
) -> eyre::Result<()> {
    for site in study.sites() {
        println!("<h4>{}</h4>\n", site.key);

        // Plot all of the data for this metric and site, organised by CPU config and engine.
        // First we define a tick distribution factory for the x axis, based on the default for f64
        // (`FloatTickFmt: TickDistGen`) but tweaked with our own stringifier. `FloatTickFmt` is
        // not to be confused with `FloatFmt: TickFmt`, the default stringifier for f64.
        struct TicksX;
        impl TickDistGen<f64> for TicksX {
            type Res = TickDistribution<Vec<f64>, WithTickFmt<FloatFmt, fn(&f64) -> String>>;
            fn generate(
                self,
                data: &poloto::ticks::DataBound<f64>,
                canvas: &poloto::ticks::RenderFrameBound,
                req: poloto::ticks::IndexRequester,
            ) -> Self::Res {
                FloatTickFmt
                    .generate(data, canvas, req)
                    .with_tick_fmt(|&x| fmt_seconds_short(x))
            }
        }
        // Then we define one for the y axis that gives us exactly one tick every 1.0f64,
        // stringifying the labels without any decimal places.
        pub struct SeriesFmt;
        impl TickFmt<f64> for SeriesFmt {
            fn write_tick(&self, writer: &mut dyn std::fmt::Write, x: &f64) -> std::fmt::Result {
                write!(writer, "{x:.0}")
            }
        }
        pub struct SeriesTickFmt;
        impl TickDistGen<f64> for SeriesTickFmt {
            type Res = TickDistribution<Vec<f64>, SeriesFmt>;
            fn generate(
                self,
                data: &poloto::ticks::DataBound<f64>,
                _: &RenderFrameBound,
                _: IndexRequester,
            ) -> Self::Res {
                let min = data.min as i128;
                let max = data.max as i128;
                TickDistribution {
                    res: TickRes { dash_size: None },
                    iter: (min..=max).map(|x| x as f64).collect(),
                    fmt: SeriesFmt,
                }
            }
        }
        // Next we look up all of the raw data series (`JsonRawSeries`) for this metric and site.
        // There is one raw data series for each CPU config and engine. Create a plot builder for
        // each series, pair them up, and collect them into a vec.
        let mut plots = vec![];
        for (cpu_config, site, engine) in study.cpu_configs().flat_map(|cpu_config| {
            study
                .engines()
                .map(move |engine| (cpu_config, site, engine))
        }) {
            if let Some(series) = raw_series_map.get(&(cpu_config.key, site.key, engine.key)) {
                if let Some(series) = series
                    .iter()
                    .find(|s| s.kind == event_kind && s.name == summary_key)
                {
                    let plot = poloto::build::plot(format!("{} {}", cpu_config.key, engine.key));
                    plots.push((series, plot));
                }
            }
        }
        // Plot each series on the respective plot as (time value ms: f64, index: i128), where
        // `index` is in reverse order of series. Since the y axis increases upwards but the legend
        // is read from top to bottom, this makes the plots appear in the same order as the legend.
        let series_count = plots.len() as f64;
        let plots = plots.into_iter().enumerate().map(|(i, (series, plot))| {
            plot.scatter(series.xs.iter().map(|&x| {
                (
                    x,
                    series_count - i as f64 + (rand::thread_rng().gen::<f64>() - 0.5f64) * 0.25f64,
                )
            }))
        });
        // Render the plot as both an SVG file and a data URL.
        let plot_svg = poloto::frame_build()
            .data(poloto::plots!(
                // Make sure x = 0ms is in view, plus space around each y series.
                poloto::build::markers([0f64], [0f64, series_count + 1.0f64]),
                plots
            ))
            .map_xticks(|_| TicksX)
            .map_yticks(|_| SeriesTickFmt)
            .build_and_label((format!("{} {}", summary_key, site.key), "time", "sample"))
            .append_to(poloto::header().light_theme())
            .render_string()?;
        let plot_path = format!("{}.{}.{}.svg", event_kind, summary_key, site.key);
        File::create(&plot_path)?.write_all(plot_svg.as_bytes())?;
        let mut plot_data_url = DataUrl::new();
        plot_data_url.set_media_type(Some("image/svg+xml".to_owned()));
        plot_data_url.set_data(plot_svg.as_bytes());
        println!("<img src='{}'>\n", plot_data_url.to_string());

        println!("<table border=1 cellpadding=3>");
        println!("<tr>");
        println!("<th colspan=2>");
        for cpu_config in study.cpu_configs() {
            println!("<th>{}", cpu_config.key);
        }
        let list: &[(&str, Box<dyn Fn(&Summary<_>) -> String>)] = &[
            // ("n", Box::new(|s| s.fmt_n())),
            // ("μ", Box::new(|s| s.fmt_mean())),
            // ("s", Box::new(|s| s.fmt_stdev())),
            ("min", Box::new(|s| s.fmt_min())),
            // ("max", Box::new(|s| s.fmt_max())),
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

fn escape_html_for_inner_html(text: &str) -> String {
    text.replace("&", "&amp;").replace("<", "&lt;")
}

fn escape_html_for_attribute(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("'", "&apos;")
        .replace(r#"""#, "&quot;")
}
