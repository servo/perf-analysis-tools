use std::{collections::BTreeMap, fmt::Display, time::Duration};

use jane_eyre::eyre::{self, OptionExt};

pub static SYNTHETIC_NAMES: &'static str = "Parse Script Layout Rasterise FP FCP";

pub trait Sample {
    fn path(&self) -> &str;
    fn real_events(&self) -> eyre::Result<Vec<Event>>;
    fn synthetic_events(&self) -> eyre::Result<Vec<Event>>;
}

#[derive(Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub start: Duration,
    /// Some if the event is a span, None if the event is instantaneous.
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

impl Event {
    pub fn end(&self) -> Duration {
        if let Some(duration) = self.duration {
            self.start + duration
        } else {
            self.start.clone()
        }
    }

    pub fn generate_merged_events<'event>(
        events: impl Iterator<Item = &'event Event>,
        merged_name: &str,
    ) -> eyre::Result<Vec<Event>> {
        enum Edge {
            Start,
            End,
        }

        let mut edges: BTreeMap<Duration, Vec<Edge>> = BTreeMap::default();
        for event in events {
            edges.entry(event.start).or_default().push(Edge::Start);
            edges.entry(event.end()).or_default().push(Edge::End);
        }

        let mut result = vec![];
        let mut active_count = 0usize;
        let mut start_time = None;
        for (time, edges) in edges {
            let mut new_active_count = active_count;
            for edge in edges {
                match edge {
                    Edge::Start => new_active_count += 1,
                    Edge::End => new_active_count -= 1,
                }
            }
            if active_count > 0 && new_active_count == 0 {
                let start_time = start_time.ok_or_eyre("No start time")?;
                let duration = time - start_time;
                result.push(Event {
                    name: merged_name.to_owned(),
                    start: start_time,
                    duration: Some(duration),
                });
            } else if active_count == 0 && new_active_count > 0 {
                start_time = Some(time);
            }
            active_count = new_active_count;
        }

        Ok(result)
    }
}

#[test]
fn test_generate_merged_events() -> eyre::Result<()> {
    let result = Event::generate_merged_events(
        [
            Event {
                name: "".to_owned(),
                start: Duration::from_secs(1),
                duration: None,
            },
            Event {
                name: "".to_owned(),
                start: Duration::from_secs(2),
                duration: Some(Duration::from_secs(2)),
            },
            Event {
                name: "".to_owned(),
                start: Duration::from_secs(3),
                duration: Some(Duration::from_secs(2)),
            },
            Event {
                name: "".to_owned(),
                start: Duration::from_secs(5),
                duration: Some(Duration::from_secs(2)),
            },
        ]
        .iter(),
        "",
    )?;
    assert_eq!(
        result,
        [Event {
            name: "".to_owned(),
            start: Duration::from_secs(2),
            duration: Some(Duration::from_secs(5))
        },]
    );
    Ok(())
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
