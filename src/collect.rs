use std::{fs::create_dir_all, path::Path, process::Command};

use jane_eyre::eyre::{self, bail, eyre, OptionExt};
use tracing::info;

use crate::{
    shell::SHELL,
    study::{KeyedCpuConfig, KeyedEngine, KeyedSite, Study},
};

pub fn main(args: Vec<String>) -> eyre::Result<()> {
    let study_dir = Path::new(
        args.iter()
            .nth(0)
            .expect("Usage: collect <studies/example>"),
    );
    let study = Study::load(study_dir.join("study.toml"))?;

    // Change working directory to the study directory.
    // We need this for `traceconv_command` and `isolate_cpu_command`.
    std::env::set_current_dir(study_dir)?;

    for cpu_config in study.cpu_configs() {
        info!("Setting up CPU isolation");
        let (program, args) = study
            .isolate_cpu_command
            .split_first()
            .ok_or_eyre("Bad isolate_cpu_command")?;
        let mut args = args.to_owned();
        args.push(std::process::id().to_string());
        args.extend(cpu_config.cpus.iter().map(|cpu| cpu.to_string()));
        info!(?program, ?args, "Running program");
        let exit_status = Command::new(program).args(args).spawn()?.wait()?;
        if !exit_status.success() {
            bail!("Process failed: {exit_status}");
        }

        for site in study.sites() {
            for engine in study.engines() {
                create_sample(&study, cpu_config, site, engine)?;
            }
        }
    }

    Ok(())
}

#[tracing::instrument(level = "error", skip(study, cpu_config, site, engine), fields(cpu_config = cpu_config.key, site = site.key, engine = engine.key))]
fn create_sample(
    study: &Study,
    cpu_config: KeyedCpuConfig<'_>,
    site: KeyedSite<'_>,
    engine: KeyedEngine<'_>,
) -> eyre::Result<()> {
    let sample_dir = Path::new(cpu_config.key).join(site.key).join(engine.key);
    create_dir_all(&sample_dir)?;

    if std::fs::exists(sample_dir.join("done"))? {
        info!("Sample is already done; skipping");
        return Ok(());
    }

    let sample_dir = sample_dir.to_str().ok_or_eyre("Bad sample path")?;
    info!("Creating sample");
    let mut args = vec![
        engine.browser_path().to_owned(),
        site.url.to_owned(),
        study.sample_size.to_string(),
        sample_dir.to_owned(),
    ];
    args.extend(site.extra_engine_arguments(engine.key).to_owned());
    let exit_status = SHELL
        .lock()
        .map_err(|e| eyre!("Mutex poisoned: {e:?}"))?
        .run(engine.benchmark_runner_code(), args)?
        .spawn()?
        .wait()?;
    if !exit_status.success() {
        bail!("Process failed: {exit_status}");
    }

    Ok(())
}
