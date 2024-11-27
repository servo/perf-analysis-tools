use std::{ffi::OsStr, fs::File, io::Write, path::Path, process::Command};

use jane_eyre::eyre::{self, bail, OptionExt};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use tracing::info;

use crate::study::{Engine, KeyedCpuConfig, KeyedEngine, KeyedSite, Study};

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

    for cpu_config in study.cpu_configs() {
        for site in study.sites() {
            for engine in study.engines() {
                analyse_sample(&study, cpu_config, site, engine)?;
            }
        }
    }

    Ok(())
}

#[tracing::instrument(level = "error", skip(study, cpu_config, site, engine), fields(cpu_config = cpu_config.key, site = site.key, engine = engine.key))]
fn analyse_sample(
    study: &Study,
    cpu_config: KeyedCpuConfig<'_>,
    site: KeyedSite<'_>,
    engine: KeyedEngine<'_>,
) -> eyre::Result<()> {
    let sample_dir = Path::new(cpu_config.key).join(site.key).join(engine.key);
    let mut args = vec![site.url.to_owned()];

    info!(?sample_dir, "Analysing sample");
    match engine.engine {
        Engine::Servo { .. } => {
            for entry in std::fs::read_dir(&sample_dir)? {
                let path = entry?.path();
                // Skip our own output files `summaries.*`.
                if path.file_stem() == Some(OsStr::new("summaries")) {
                    continue;
                }
                // Filter to `manifest*.json`.
                if path.extension() == Some(OsStr::new("json")) {
                    args.push(path.to_str().ok_or_eyre("Unsupported path")?.to_owned());
                }
            }
        }
        Engine::Chromium { .. } => {
            let mut json_paths = vec![];
            let mut convert_jobs = vec![];
            for entry in std::fs::read_dir(&sample_dir)? {
                let path = entry?.path();
                // Filter to `chrome*.pftrace`.
                if path.extension() == Some(OsStr::new("pftrace")) {
                    let pftrace_path = path.to_str().ok_or_eyre("Unsupported path")?;
                    let json_path = format!(
                        "{}.json",
                        pftrace_path
                            .strip_suffix(".pftrace")
                            .expect("Guaranteed by extension check")
                    );
                    if !std::fs::exists(&json_path)? {
                        convert_jobs.push((pftrace_path.to_owned(), json_path.clone()));
                    }
                    json_paths.push(json_path);
                }
            }
            let traceconv_results = convert_jobs
                .par_iter()
                .map(|(pftrace_path, json_path)| -> eyre::Result<()> {
                    convert_pftrace_to_json(study, pftrace_path, json_path)
                })
                .collect::<Vec<_>>();
            for result in traceconv_results {
                result?;
            }
            for entry in std::fs::read_dir(&sample_dir)? {
                let path = entry?.path();
                // Skip our own output files `summaries.*`.
                if path.file_stem() == Some(OsStr::new("summaries")) {
                    continue;
                }
                // Filter to `chrome*.json`.
                if path.extension() == Some(OsStr::new("json")) {
                    args.push(path.to_str().ok_or_eyre("Unsupported path")?.to_owned());
                }
            }
        }
    }

    let summaries = match engine.engine {
        Engine::Servo { .. } => crate::servo::compute_summaries(args)?,
        Engine::Chromium { .. } => crate::chromium::compute_summaries(args)?,
    };

    File::create(sample_dir.join("summaries.json"))?.write_all(summaries.json().as_bytes())?;
    File::create(sample_dir.join("summaries.txt"))?.write_all(summaries.text()?.as_bytes())?;

    Ok(())
}

#[tracing::instrument(level = "error", err, skip(study))]
fn convert_pftrace_to_json(study: &Study, pftrace_path: &str, json_path: &str) -> eyre::Result<()> {
    let (program, args) = study
        .traceconv_command
        .split_first()
        .ok_or_eyre("Bad traceconv_command")?;
    let mut args = args.to_owned();
    args.extend([
        "json".to_owned(),
        pftrace_path.to_owned(),
        json_path.to_owned(),
    ]);
    info!(?program, ?args, "Running traceconv");
    let exit_status = Command::new(program).args(args).spawn()?.wait()?;
    if !exit_status.success() {
        bail!("Process failed: {exit_status}");
    }

    Ok(())
}
