use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

use jane_eyre::eyre;
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
pub struct CpuConfig(Vec<usize>);
#[derive(Clone, Copy, Debug)]
pub struct KeyedCpuConfig<'study> {
    pub key: &'study str,
    pub cpus: &'study [usize],
}

#[derive(Debug, Deserialize)]
pub struct Site(String);
#[derive(Clone, Copy, Debug)]
pub struct KeyedSite<'study> {
    pub key: &'study str,
    pub url: &'study str,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Engine {
    Servo { path: String },
    Chromium { path: String },
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
        self.sites
            .iter()
            .map(|(key, site)| KeyedSite { key, url: &site.0 })
    }

    pub fn engines(&self) -> impl Iterator<Item = KeyedEngine> {
        self.engines
            .iter()
            .map(|(key, engine)| KeyedEngine { key, engine })
    }
}

impl KeyedEngine<'_> {
    pub fn benchmark_runner_code(&self) -> &str {
        match self.engine {
            Engine::Servo { .. } => include_str!("../benchmark-servo.sh"),
            Engine::Chromium { .. } => include_str!("../benchmark-chromium.sh"),
        }
    }

    pub fn browser_path(&self) -> &str {
        match self.engine {
            Engine::Servo { path } => path,
            Engine::Chromium { path } => path,
        }
    }
}
