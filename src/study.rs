use std::{collections::BTreeMap, fs::File, io::Read, path::Path, time::Duration};

use jane_eyre::eyre::{self, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Study {
    pub sample_size: usize,
    pub traceconv_command: Vec<String>,
    pub isolate_cpu_command: Vec<String>,

    cpu_configs: BTreeMap<String, CpuConfig>,
    sites: BTreeMap<String, Site>,
    engines: BTreeMap<String, Engine>,
}

#[derive(Debug, Deserialize)]
struct CpuConfig(Vec<usize>);
#[derive(Clone, Copy, Debug)]
pub struct KeyedCpuConfig<'study> {
    pub key: &'study str,
    pub cpus: &'study [usize],
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Site {
    UrlOnly(String),
    Full {
        url: String,
        browser_open_time: Option<u64>,
        user_agent: Option<String>,
        screen_size: Option<Vec<usize>>,
        wait_for_selectors: Option<BTreeMap<String, usize>>,
        extra_engine_arguments: Option<BTreeMap<String, Vec<String>>>,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct KeyedSite<'study> {
    pub key: &'study str,
    pub url: &'study str,
    pub browser_open_time: Duration,
    pub user_agent: Option<&'study str>,
    screen_size: Option<&'study [usize]>,
    wait_for_selectors: Option<&'study BTreeMap<String, usize>>,
    extra_engine_arguments: Option<&'study BTreeMap<String, Vec<String>>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Engine {
    Servo { path: String },
    Chromium { path: String },
    ChromeDriver { path: String },
}
#[derive(Clone, Copy, Debug)]
pub struct KeyedEngine<'study> {
    pub key: &'study str,
    pub engine: &'study Engine,
}

impl Study {
    pub fn load(path: impl AsRef<Path>) -> eyre::Result<Self> {
        let mut result = String::default();
        File::open(path)?.read_to_string(&mut result)?;
        let result: Study = toml::from_str(&result)?;

        Ok(result)
    }

    pub fn cpu_configs(&self) -> impl Iterator<Item = KeyedCpuConfig> {
        self.cpu_configs
            .iter()
            .map(|(key, cpu_config)| KeyedCpuConfig {
                key,
                cpus: &cpu_config.0,
            })
    }

    pub fn sites(&self) -> impl Iterator<Item = KeyedSite> {
        self.sites.iter().map(|(key, site)| (&**key, site).into())
    }

    pub fn engines(&self) -> impl Iterator<Item = KeyedEngine> {
        self.engines
            .iter()
            .map(|(key, engine)| KeyedEngine { key, engine })
    }
}

impl<'study> From<(&'study str, &'study Site)> for KeyedSite<'study> {
    fn from((key, site): (&'study str, &'study Site)) -> Self {
        let default_browser_open_time = Duration::from_secs(10);

        match site {
            Site::UrlOnly(url) => Self {
                key,
                url,
                browser_open_time: default_browser_open_time,
                user_agent: None,
                screen_size: None,
                wait_for_selectors: None,
                extra_engine_arguments: None,
            },
            Site::Full {
                url,
                browser_open_time,
                user_agent,
                screen_size,
                wait_for_selectors,
                extra_engine_arguments,
            } => Self {
                key,
                url,
                browser_open_time: browser_open_time
                    .map_or(default_browser_open_time, Duration::from_secs),
                user_agent: user_agent.as_deref(),
                screen_size: screen_size.as_deref(),
                wait_for_selectors: wait_for_selectors.as_ref(),
                extra_engine_arguments: extra_engine_arguments.as_ref(),
            },
        }
    }
}

impl KeyedSite<'_> {
    pub fn screen_size(&self) -> eyre::Result<Option<(usize, usize)>> {
        self.screen_size
            .map(|size| {
                Ok(match size {
                    [width, height] => (*width, *height),
                    other => bail!("Bad screen_size: {other:?}"),
                })
            })
            .transpose()
    }

    pub fn wait_for_selectors(&self) -> Box<dyn Iterator<Item = (&String, &usize)> + '_> {
        if let Some(wait_for_selectors) = self.wait_for_selectors {
            Box::new(wait_for_selectors.iter())
        } else {
            Box::new([].into_iter())
        }
    }

    pub fn extra_engine_arguments(&self, engine_key: &str) -> &[String] {
        self.extra_engine_arguments
            .and_then(|map| map.get(engine_key))
            .map_or(&[], |result| &result)
    }
}

impl KeyedEngine<'_> {
    pub fn benchmark_runner_code(&self) -> &str {
        match self.engine {
            Engine::Servo { .. } => include_str!("../benchmark-servo.sh"),
            Engine::Chromium { .. } => include_str!("../benchmark-chromium.sh"),
            Engine::ChromeDriver { .. } => {
                panic!("BUG: Engine::ChromeDriver has no benchmark runner script")
            }
        }
    }

    pub fn browser_path(&self) -> &str {
        match self.engine {
            Engine::Servo { path } => path,
            Engine::Chromium { path } => path,
            Engine::ChromeDriver { path } => path,
        }
    }
}
