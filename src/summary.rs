use std::{collections::BTreeMap, fmt::Display, time::Duration};

use jane_eyre::eyre::{self, OptionExt};

pub trait Sample {
    fn durations(&self) -> &BTreeMap<String, Duration>;
    fn events(&self) -> eyre::Result<Vec<Event>>;
}

pub struct Event {
    pub name: String,
    pub start: Duration,
    pub duration: Option<Duration>,
}

pub struct Analysis<SampleType> {
    pub samples: Vec<SampleType>,
}

#[derive(Debug)]
pub struct Summary<T> {
    pub n: usize,
    pub mean: T,
    pub stdev: T,
    pub min: T,
    pub max: T,
}

impl<SampleType> Analysis<SampleType> {
    pub fn summary<T: Into<Option<f64>>>(
        &self,
        mut getter: impl FnMut(&SampleType) -> T,
    ) -> eyre::Result<Summary<f64>> {
        let xs = self
            .samples
            .iter()
            .filter_map(|x| getter(x).into())
            .collect::<Vec<f64>>();
        let n = xs.len();
        let mean = xs.iter().sum::<f64>() / (n as f64);
        let stdev =
            (xs.iter().map(|x| (x - mean).powf(2.0)).sum::<f64>() / ((n - 1) as f64)).sqrt();
        let min = xs
            .iter()
            .cloned()
            .min_by(|p, q| p.total_cmp(q))
            .ok_or_eyre("No minimum")?;
        let max = xs
            .iter()
            .cloned()
            .max_by(|p, q| p.total_cmp(q))
            .ok_or_eyre("No maximum")?;

        Ok(Summary {
            n: self.samples.len(),
            mean,
            stdev,
            min,
            max,
        })
    }
}

impl Display for Summary<Duration> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={}, μ={:?}, s={:?}, min={:?}, max={:?}",
            self.n, self.mean, self.stdev, self.min, self.max
        )
    }
}

impl Display for Summary<f64> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = |x| {
            if x >= 1.0 {
                (x, "s")
            } else if x * 1000.0 >= 1.0 {
                (x * 1000.0, "ms")
            } else if x * 1000000.0 >= 1.0 {
                (x * 1000000.0, "μs")
            } else {
                (x * 1000000000.0, "ns")
            }
        };
        let dp = |x| {
            let (value, _) = value(x);
            if value >= 1000.0 {
                0
            } else if value >= 100.0 {
                1
            } else if value >= 10.0 {
                2
            } else {
                3
            }
        };
        let (mean, mean_unit) = value(self.mean);
        let (stdev, stdev_unit) = value(self.stdev);
        let (min, min_unit) = value(self.min);
        let (max, max_unit) = value(self.max);
        write!(
            f,
            "n={}, μ={:.*?}{}, s={:.*?}{}, min={:.*?}{}, max={:.*?}{}",
            self.n,
            dp(self.mean),
            mean,
            mean_unit,
            dp(self.stdev),
            stdev,
            stdev_unit,
            dp(self.min),
            min,
            min_unit,
            dp(self.max),
            max,
            max_unit,
        )
    }
}

impl<T> Summary<T> {
    fn convert<U>(&self, mut f: impl FnMut(&T) -> U) -> Summary<U> {
        Summary {
            n: self.n,
            mean: f(&self.mean),
            stdev: f(&self.stdev),
            min: f(&self.min),
            max: f(&self.max),
        }
    }
}
