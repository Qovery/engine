use std::future::Future;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use serde::Deserialize;

use crate::cmd::docker::BuilderPods;
use crate::metrics_registry::BuilderUsage;
use crate::runtime::block_on;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct PodMetricsList {
    items: Vec<PodMetrics>,
}

#[derive(Deserialize)]
struct PodMetrics {
    metadata: PodMetricsMetadata,
    containers: Vec<ContainerMetrics>,
}

#[derive(Deserialize)]
struct PodMetricsMetadata {
    name: String,
}

#[derive(Deserialize)]
struct ContainerMetrics {
    usage: ContainerUsage,
}

#[derive(Deserialize)]
struct ContainerUsage {
    cpu: String,
    memory: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    cpu_milli: u64,
    memory_mib: u64,
}

fn scaled(quantity: &str, units: &[(&str, f64)], plain_scale: f64) -> Option<u64> {
    let (number, scale) = units
        .iter()
        .find_map(|(suffix, scale)| quantity.strip_suffix(suffix).map(|number| (number, *scale)))
        .unwrap_or((quantity, plain_scale));
    let value = number
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    Some((value * scale).round() as u64)
}

fn cpu_milli(quantity: &str) -> Option<u64> {
    scaled(quantity, &[("n", 1e-6), ("u", 1e-3), ("m", 1.0)], 1000.0)
}

fn memory_mib(quantity: &str) -> Option<u64> {
    scaled(
        quantity,
        &[
            ("Ki", 1.0 / 1024.0),
            ("Mi", 1.0),
            ("Gi", 1024.0),
            ("Ti", 1024.0 * 1024.0),
        ],
        1.0 / (1024.0 * 1024.0),
    )
}

/// Busiest of the builder's pods, as limits apply per pod: buildx names each pod `{node_name}-{suffix}`.
/// A pod with an unreadable quantity is skipped; None when no pod of this builder can be read.
fn builder_sample(pod_metrics: &PodMetricsList, node_names: &[String]) -> Option<Sample> {
    let builder_pods = pod_metrics
        .items
        .iter()
        .filter(|pod| {
            node_names.iter().any(|node_name| {
                pod.metadata
                    .name
                    .strip_prefix(node_name.as_str())
                    .is_some_and(|suffix| suffix.starts_with('-'))
            })
        })
        .collect::<Vec<_>>();
    builder_pods
        .into_iter()
        .filter_map(|pod| {
            pod.containers.iter().try_fold(
                Sample {
                    cpu_milli: 0,
                    memory_mib: 0,
                },
                |pod_sample, container| {
                    Some(Sample {
                        cpu_milli: pod_sample.cpu_milli + cpu_milli(&container.usage.cpu)?,
                        memory_mib: pod_sample.memory_mib + memory_mib(&container.usage.memory)?,
                    })
                },
            )
        })
        .reduce(|busiest, pod_sample| Sample {
            cpu_milli: busiest.cpu_milli.max(pod_sample.cpu_milli),
            memory_mib: busiest.memory_mib.max(pod_sample.memory_mib),
        })
}

#[derive(Default)]
struct UsageAccumulator {
    cpu_milli_max: u64,
    cpu_milli_sum: u64,
    memory_mib_max: u64,
    samples: u32,
}

impl UsageAccumulator {
    fn add(&mut self, sample: Sample) {
        self.cpu_milli_max = self.cpu_milli_max.max(sample.cpu_milli);
        self.cpu_milli_sum += sample.cpu_milli;
        self.memory_mib_max = self.memory_mib_max.max(sample.memory_mib);
        self.samples += 1;
    }

    fn usage(&self) -> Option<BuilderUsage> {
        if self.samples == 0 {
            return None;
        }
        let to_u32 = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        Some(BuilderUsage {
            cpu_milli_max: to_u32(self.cpu_milli_max),
            cpu_milli_avg: to_u32(self.cpu_milli_sum / u64::from(self.samples)),
            memory_mib_max: to_u32(self.memory_mib_max),
            samples: self.samples,
        })
    }
}

/// Runs `work` while sampling the builder's CPU and memory on a scoped thread. Best effort: no builder
/// pods, a missing metrics API or a failed call only loses samples, never fails `work`.
pub fn sample_while<R>(pods: Option<&BuilderPods>, work: impl FnOnce() -> R) -> (R, Option<BuilderUsage>) {
    let Some(pods) = pods else {
        return (work(), None);
    };
    let client = match block_on(kube::Client::try_default()) {
        Ok(client) => client,
        Err(err) => {
            debug!("Cannot sample builder usage: {err}");
            return (work(), None);
        }
    };
    sample_with(
        SAMPLE_INTERVAL,
        move || {
            let client = client.clone();
            async move { fetch_sample(&client, pods).await }
        },
        work,
    )
}

async fn fetch_sample(client: &kube::Client, pods: &BuilderPods) -> anyhow::Result<Option<Sample>> {
    let request =
        http::Request::get(format!("/apis/metrics.k8s.io/v1beta1/namespaces/{}/pods", pods.namespace)).body(vec![])?;
    let pod_metrics: PodMetricsList = tokio::time::timeout(REQUEST_TIMEOUT, client.request(request))
        .await
        .context("metrics API timed out")??;
    Ok(builder_sample(&pod_metrics, &pods.node_names))
}

/// Waits for a sample in flight when `work` ends, at most `REQUEST_TIMEOUT`.
fn sample_with<R, F: Future<Output = anyhow::Result<Option<Sample>>>>(
    interval: Duration,
    fetch_sample: impl Fn() -> F + Send,
    work: impl FnOnce() -> R,
) -> (R, Option<BuilderUsage>) {
    thread::scope(|scope| {
        let (stop, stopped) = mpsc::channel::<()>();
        let sampler = thread::Builder::new()
            .name("builder-usage".to_string())
            .spawn_scoped(scope, move || {
                let mut usage = UsageAccumulator::default();
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                    match block_on(fetch_sample()) {
                        Ok(Some(sample)) => usage.add(sample),
                        Ok(None) => debug!("No metrics for the builder pods yet"),
                        Err(err) => debug!("Cannot sample builder usage: {err:#}"),
                    }
                }
                usage
            });
        if let Err(err) = &sampler {
            debug!("Cannot start builder usage sampler: {err}");
        }

        let result = work();
        drop(stop);
        let usage = sampler
            .ok()
            .and_then(|sampler| sampler.join().ok())
            .and_then(|usage| usage.usage());
        (result, usage)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn pod_metrics(pods: &[(&str, &[(&str, &str)])]) -> PodMetricsList {
        let items = pods
            .iter()
            .map(|(name, containers)| {
                let containers = containers
                    .iter()
                    .map(|(cpu, memory)| format!(r#"{{"name":"c","usage":{{"cpu":"{cpu}","memory":"{memory}"}}}}"#))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    r#"{{"metadata":{{"name":"{name}","namespace":"builders"}},"timestamp":"2026-10-06T10:00:00Z","window":"15s","containers":[{containers}]}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        serde_json::from_str(&format!(
            r#"{{"kind":"PodMetricsList","apiVersion":"metrics.k8s.io/v1beta1","metadata":{{}},"items":[{items}]}}"#
        ))
        .expect("valid PodMetricsList")
    }

    fn node_names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn cpu_quantities_convert_to_millicores() {
        assert_eq!(cpu_milli("1500000000n"), Some(1500));
        assert_eq!(cpu_milli("250000u"), Some(250));
        assert_eq!(cpu_milli("750m"), Some(750));
        assert_eq!(cpu_milli("2"), Some(2000));
        assert_eq!(cpu_milli("0.5"), Some(500));
        assert_eq!(cpu_milli("abc"), None);
    }

    #[test]
    fn memory_quantities_convert_to_mib() {
        assert_eq!(memory_mib("2097152Ki"), Some(2048));
        assert_eq!(memory_mib("512Mi"), Some(512));
        assert_eq!(memory_mib("3Gi"), Some(3072));
        assert_eq!(memory_mib("1048576"), Some(1));
        assert_eq!(memory_mib("12Zz"), None);
    }

    #[test]
    fn sample_keeps_the_busiest_pod_of_this_builder_with_its_containers_summed() {
        let metrics = pod_metrics(&[
            ("build-exec-0-amd64-7d9f8-abcde", &[("1000m", "1024Mi"), ("250m", "512Mi")]),
            ("build-exec-0-arm64-5c4b3-fghij", &[("500000000n", "1048576Ki")]),
            ("build-other-0-amd64-1a2b3-klmno", &[("4", "8Gi")]),
        ]);

        let sample = builder_sample(&metrics, &node_names(&["build-exec-0-amd64", "build-exec-0-arm64"]));

        assert_eq!(
            sample,
            Some(Sample {
                cpu_milli: 1250,
                memory_mib: 1536
            })
        );
    }

    #[test]
    fn multi_arch_sample_stays_under_the_per_pod_limit_when_no_pod_exceeds_it() {
        let metrics = pod_metrics(&[
            ("build-exec-0-amd64-7d9f8-abcde", &[("3000m", "3Gi")]),
            ("build-exec-0-arm64-5c4b3-fghij", &[("3500m", "2Gi")]),
        ]);

        let sample = builder_sample(&metrics, &node_names(&["build-exec-0-amd64", "build-exec-0-arm64"]));

        assert_eq!(
            sample,
            Some(Sample {
                cpu_milli: 3500,
                memory_mib: 3072
            })
        );
    }

    #[test]
    fn sample_is_none_when_no_pod_of_the_builder_is_reported() {
        let metrics = pod_metrics(&[("build-other-0-amd64-1a2b3-klmno", &[("4", "8Gi")])]);

        assert_eq!(builder_sample(&metrics, &node_names(&["build-exec-0-amd64"])), None);
    }

    #[test]
    fn sample_is_none_on_an_unreadable_quantity() {
        let metrics = pod_metrics(&[("build-exec-0-amd64-7d9f8-abcde", &[("1000m", "1024Zz")])]);

        assert_eq!(builder_sample(&metrics, &node_names(&["build-exec-0-amd64"])), None);
    }

    #[test]
    fn an_unreadable_pod_does_not_hide_the_other_builder_pods() {
        let metrics = pod_metrics(&[
            ("build-exec-0-amd64-7d9f8-abcde", &[("2000m", "2Gi")]),
            ("build-exec-0-arm64-5c4b3-fghij", &[("1000m", "1024Zz")]),
        ]);

        let sample = builder_sample(&metrics, &node_names(&["build-exec-0-amd64", "build-exec-0-arm64"]));

        assert_eq!(
            sample,
            Some(Sample {
                cpu_milli: 2000,
                memory_mib: 2048
            })
        );
    }

    #[test]
    fn usage_keeps_max_cpu_mean_cpu_max_memory_and_sample_count() {
        let mut accumulator = UsageAccumulator::default();
        for (cpu_milli, memory_mib) in [(1000, 3000), (4000, 1000), (1000, 2000)] {
            accumulator.add(Sample { cpu_milli, memory_mib });
        }

        assert_eq!(
            accumulator.usage(),
            Some(BuilderUsage {
                cpu_milli_max: 4000,
                cpu_milli_avg: 2000,
                memory_mib_max: 3000,
                samples: 3,
            })
        );
    }

    #[test]
    fn usage_is_none_without_samples() {
        assert_eq!(UsageAccumulator::default().usage(), None);
    }

    #[test]
    fn sampler_aggregates_samples_taken_while_the_work_runs() {
        let (result, usage) = sample_with(
            Duration::from_millis(5),
            || async {
                Ok(Some(Sample {
                    cpu_milli: 1000,
                    memory_mib: 512,
                }))
            },
            || {
                thread::sleep(Duration::from_millis(60));
                "built"
            },
        );

        let usage = usage.expect("usage after samples");
        assert_eq!(result, "built");
        assert_eq!(usage.cpu_milli_max, 1000);
        assert_eq!(usage.memory_mib_max, 512);
        assert!(usage.samples >= 2);
    }

    #[test]
    fn sampler_stops_when_the_work_ends() {
        let started = Instant::now();

        let (_, usage) = sample_with(Duration::from_secs(60), || async { Ok(None) }, || ());

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "sampler kept running after the work"
        );
        assert_eq!(usage, None);
    }

    #[test]
    fn sampler_reports_nothing_when_every_fetch_fails() {
        let (_, usage) = sample_with(
            Duration::from_millis(1),
            || async { Err(anyhow::anyhow!("metrics API not found")) },
            || thread::sleep(Duration::from_millis(30)),
        );

        assert_eq!(usage, None);
    }

    #[test]
    fn sampler_awaits_fetches_inside_the_engine_runtime() {
        let (_, usage) = thread::spawn(|| {
            sample_with(
                Duration::from_millis(5),
                || async {
                    tokio::time::timeout(Duration::from_secs(1), async {
                        Ok::<_, anyhow::Error>(Some(Sample {
                            cpu_milli: 1000,
                            memory_mib: 512,
                        }))
                    })
                    .await?
                },
                || thread::sleep(Duration::from_millis(40)),
            )
        })
        .join()
        .expect("sampler thread must not panic");

        assert!(usage.is_some_and(|usage| usage.samples >= 1));
    }

    #[test]
    fn no_builder_pods_runs_the_work_without_sampling() {
        let (result, usage) = sample_while(None, || 42);

        assert_eq!((result, usage), (42, None));
    }
}
