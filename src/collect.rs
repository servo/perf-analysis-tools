use core::str;
use std::{
    collections::BTreeMap,
    fs::{copy, create_dir_all, read_dir, rename, File},
    net::{Shutdown, TcpStream},
    path::Path,
    process::Command,
    thread::sleep,
    time::Duration,
};

use jane_eyre::eyre::{self, bail, eyre, Context, OptionExt};
use serde_json::json;
use tracing::{debug, error, info, warn};
use webdriver_client::{
    chrome::ChromeDriver, messages::NewSessionCmd, Driver, HttpDriverBuilder, LocationStrategy,
};

use crate::{
    shell::SHELL,
    study::{Engine, KeyedCpuConfig, KeyedEngine, KeyedSite, Study},
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

    if engine.uses_webdriver() {
        // Resolve path against PATH if needed. ChromeDriver or WebDriver seems to need this.
        let query = SHELL
            .lock()
            .map_err(|e| eyre!("Mutex poisoned: {e:?}"))?
            .run(include_str!("../query-path.sh"), [engine.browser_path()])?
            .output()?;
        if !query.status.success() {
            bail!("Process failed: {}", query.status);
        }
        let path = str::from_utf8(&query.stdout)?
            .strip_suffix("\n")
            .ok_or_eyre("Output has no trailing newline")?;

        for i in 1..=study.sample_size {
            let (mut session, cleanup): (_, Box<dyn FnOnce(bool) -> eyre::Result<()>>) =
                match engine.engine {
                    Engine::Servo { .. } | Engine::Chromium { .. } => {
                        unreachable!("Guaranteed by Engine::uses_webdriver()")
                    }
                    Engine::ServoDriver { .. } => {
                        info!("Building HttpDriver client");
                        let driver = HttpDriverBuilder::default()
                            .url("http://127.0.0.1:7000")
                            .build()
                            .map_err(|e| eyre!("Failed to build HttpDriver client: {e}"))?;

                        // Configure the browser with WebDriver capabilities.
                        let mut params = NewSessionCmd::default();
                        // Reset to remove the unsupported `goog:chromeOptions` capability, which Servo rejects.
                        params.reset_always_match();
                        // Do not wait for page load to complete.
                        params.always_match("pageLoadStrategy", json!("none"));
                        // TODO: Servo does not support this yet, but unlike with ChromeDriver, we run servoshell ourselves
                        // and capabilities are just constraints, so we can just pass `--ignore-certificate-errors`.
                        // params.always_match("acceptInsecureCerts", json!(true));

                        info!("Starting servoshell");
                        let mut command = Command::new(path);
                        command
                            .env("SERVO_TRACING", "info")
                            .arg("--webdriver")
                            // Allow the use of mitmproxy replay (see ../start-mitmproxy.sh).
                            .arg("--ignore-certificate-errors");

                        if let Some(user_agent) = site.user_agent {
                            command.args(["--user-agent", user_agent]);
                        }
                        if let Some((width, height)) = site.screen_size()? {
                            command.args(["--screen-size", &format!("{width}x{height}")]);
                        }

                        // Write a manifest that pairs the HTML and Perfetto traces of each run,
                        // both as paths relative to the directory containing the manifest file.
                        let index_width = study.sample_size.to_string().len();
                        let trace_html_filename =
                            format!("trace{:0width$}.html", i, width = index_width);
                        let trace_html_path = sample_dir.join(&trace_html_filename);
                        let trace_html_path =
                            trace_html_path.to_str().ok_or_eyre("Unsupported path")?;
                        let servo_pftrace_filename =
                            format!("servo{:0width$}.pftrace", i, width = index_width);
                        let servo_pftrace_path = sample_dir.join(&servo_pftrace_filename);
                        let manifest_path = sample_dir.join(format!(
                            "manifest{:0width$}.json",
                            i,
                            width = index_width
                        ));
                        let manifest_file = File::create(manifest_path)?;
                        serde_json::to_writer(
                            manifest_file,
                            &json!({
                                "perfetto": servo_pftrace_filename,
                                "html": trace_html_filename,
                            }),
                        )?;

                        let mut servoshell = command
                            .arg(format!("--profiler-trace-path={trace_html_path}"))
                            .arg("--print-pwm")
                            .args(site.extra_engine_arguments(engine.key))
                            .arg("about:blank")
                            .spawn()
                            .wrap_err("Failed to start servoshell")?;

                        // Try to connect to the WebDriver server for up to ten seconds, using a temporary TcpStream to
                        // avoid session() consuming the HttpDriver.
                        info!("Connecting to WebDriver server");
                        let mut ok = false;
                        for _ in 0..40 {
                            sleep(Duration::from_millis(250));
                            if let Ok(stream) = TcpStream::connect("127.0.0.1:7000") {
                                stream.shutdown(Shutdown::Both)?;
                                ok = true;
                                break;
                            }
                        }
                        if !ok {
                            if let Err(error) = servoshell.kill() {
                                error!(?error, "Failed to kill servoshell");
                            }
                            bail!("WebDriver server did not start");
                        }

                        match driver.session(&params) {
                            Ok(session) => (
                                session,
                                Box::new(move |closing_failed| {
                                    // Kill servoshell, if closing the browser failed.
                                    // TODO: consider always killing if process is still running after a few seconds?
                                    if closing_failed {
                                        servoshell.kill().wrap_err("Failed to kill servoshell")?;
                                    }

                                    // Rename servo.pftrace to its final path.
                                    rename("servo.pftrace", servo_pftrace_path)?;

                                    Ok(())
                                }),
                            ),
                            Err(error) => {
                                error!(?error);
                                if let Err(error) = servoshell.kill() {
                                    error!(?error, "Failed to kill servoshell");
                                }
                                bail!("Failed to connect to WebDriver server");
                            }
                        }
                    }
                    Engine::ChromeDriver { .. } => {
                        info!("Starting ChromeDriver");
                        let driver = ChromeDriver::spawn()
                            .map_err(|e| eyre!("Failed to spawn ChromeDriver: {e}"))?;

                        // Configure the browser with WebDriver capabilities. Note that ChromeDriver takes care
                        // of running Chromium with a clean profile (much like `--user-data-dir=$(mktemp -d)`)
                        // and in a way amenable to automation (e.g. `--no-first-run`).
                        // <https://www.w3.org/TR/webdriver/#capabilities>
                        // <https://developer.chrome.com/docs/chromedriver/capabilities>
                        let mut params = NewSessionCmd::default();
                        // Do not wait for page load to complete.
                        params.always_match("pageLoadStrategy", json!("none"));
                        // Allow the use of mitmproxy replay (see ../start-mitmproxy.sh).
                        params.always_match("acceptInsecureCerts", json!(true));

                        let mut mobile_emulation = BTreeMap::default();
                        if let Some(user_agent) = site.user_agent {
                            // ChromeDriver does not support the standard `userAgent` capability, which goes in
                            // the top level. Use `.goog:chromeOptions.mobileEmulation.userAgent` instead.
                            mobile_emulation.insert("userAgent", json!(user_agent));
                        }
                        if let Some((width, height)) = site.screen_size()? {
                            mobile_emulation.insert(
                                "deviceMetrics",
                                json!({ "width": width, "height": height }),
                            );
                        }

                        let pftrace_temp_dir = mktemp::Temp::new_dir()?;
                        let attempted_pftrace_temp_path = pftrace_temp_dir.join("chrome.pftrace");
                        let attempted_pftrace_temp_path = attempted_pftrace_temp_path
                            .to_str()
                            .ok_or_eyre("Unsupported path")?;
                        let mut args = vec![
                            "--trace-startup".to_owned(),
                            format!("--trace-startup-file={attempted_pftrace_temp_path}"),
                        ];
                        args.extend(site.extra_engine_arguments(engine.key).to_owned());
                        params.always_match(
                            "goog:chromeOptions",
                            json!({
                                // <https://developer.chrome.com/docs/chromedriver/capabilities>
                                "mobileEmulation": mobile_emulation,
                                "binary": path,
                                "args": args,
                            }),
                        );
                        let cleanup = |_closing_failed| {
                            // When using ChromeDriver, for some reason, Chromium fails to rename the Perfetto trace
                            // to `--trace-startup-file`. Always kill ChromeDriver and rename it ourselves.
                            let pftrace_path = sample_dir.join(format!(
                                "chrome{:0width$}.pftrace",
                                i,
                                width = study.sample_size.to_string().len()
                            ));
                            let pftrace_path =
                                pftrace_path.to_str().ok_or_eyre("Unsupported path")?;
                            for entry in read_dir(&pftrace_temp_dir)? {
                                let pftrace_temp_path = entry?.path();
                                info!(
                                    ?pftrace_temp_path,
                                    ?pftrace_path,
                                    "Copying Perfetto trace to sample directory"
                                );
                                copy(pftrace_temp_path, pftrace_path)?;
                            }

                            // Extend the lifetime of `pftrace_temp_dir` to avoid premature deletion.
                            drop(pftrace_temp_dir);

                            Ok(())
                        };

                        info!("Starting Chromium");
                        (driver.session(&params)?, Box::new(cleanup))
                    }
                };

            info!(site.url, "Navigating to site");
            session.go(site.url)?;

            info!(?site.browser_open_time, "Waiting for fixed amount of time");
            sleep(site.browser_open_time);

            info!(wait_for_selectors = ?site.wait_for_selectors().collect::<Vec<_>>(), "Checking for elements");
            #[derive(Debug)]
            struct ElementCounts {
                expected: usize,
                actual: usize,
            }
            let element_counts = site
                .wait_for_selectors()
                .map(
                    |(selector, expected)| -> eyre::Result<(&String, ElementCounts)> {
                        Ok((
                            selector,
                            ElementCounts {
                                expected: *expected,
                                actual: session
                                    .find_elements(selector, LocationStrategy::Css)?
                                    .len(),
                            },
                        ))
                    },
                )
                .collect::<eyre::Result<BTreeMap<&String, ElementCounts>>>()?;
            debug!(?element_counts, "Found elements");
            for (selector, ElementCounts { expected, actual }) in element_counts {
                assert_eq!(expected, actual, "Condition failed: wait_for_selectors.{selector:?}: expected {expected}, actual {actual}");
            }

            // Close the window gracefully.
            // For Servo, this ensures that it can finish writing `trace.html` properly.
            info!("Closing browser window");
            if let Err(error) = session.close_window() {
                error!(?error, "Failed to close browser window");
                drop(session);
                cleanup(true)?;
            } else {
                cleanup(false)?;
            }
        }

        info!("Marking sample as done");
        File::create_new(sample_dir.join("done"))?;

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
        .env(
            "SERVO_PERF_BROWSER_OPEN_TIME",
            site.browser_open_time.as_secs().to_string(),
        )
        .spawn()?
        .wait()?;
    if !exit_status.success() {
        bail!("Process failed: {exit_status}");
    }

    Ok(())
}
