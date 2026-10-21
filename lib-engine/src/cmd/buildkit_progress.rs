use crate::metrics_registry::StepName;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportStepTiming {
    pub started_at: Instant,
    pub duration: Duration,
    pub failed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildkitExportTimings {
    pub image_push: Option<ExportStepTiming>,
    pub cache_export: Option<ExportStepTiming>,
}

impl BuildkitExportTimings {
    pub fn failed_step(&self) -> Option<ExportStep> {
        let failed = |timing: Option<ExportStepTiming>| timing.is_some_and(|timing| timing.failed);
        if failed(self.image_push) {
            Some(ExportStep::ImagePush)
        } else if failed(self.cache_export) {
            Some(ExportStep::CacheExport)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStep {
    ImagePush,
    CacheExport,
}

impl From<ExportStep> for StepName {
    fn from(step: ExportStep) -> Self {
        match step {
            ExportStep::ImagePush => StepName::ImagePush,
            ExportStep::CacheExport => StepName::CacheExport,
        }
    }
}

struct ExportVertex {
    step: ExportStep,
    ended: bool,
}

#[derive(Default)]
struct StepSpan {
    started_at: Option<Instant>,
    ended_at: Option<Instant>,
    running_vertices: usize,
    failed: bool,
}

impl StepSpan {
    fn timing(&self) -> Option<ExportStepTiming> {
        match (self.started_at, self.ended_at) {
            (Some(started_at), Some(ended_at)) if self.running_vertices == 0 => Some(ExportStepTiming {
                started_at,
                duration: ended_at.saturating_duration_since(started_at),
                failed: self.failed,
            }),
            _ => None,
        }
    }
}

/// Times the image push and the cache export from buildkit `--progress=plain` output, where each
/// line is `#<vertex> <text>` and a vertex's first line is its name.
#[derive(Default)]
pub struct BuildkitExportProgress {
    vertices: HashMap<u32, ExportVertex>,
    image_push: StepSpan,
    cache_export: StepSpan,
}

impl BuildkitExportProgress {
    /// Returns the export step the line belongs to, `None` for any other build output.
    pub fn observe(&mut self, line: &str, at: Instant) -> Option<ExportStep> {
        let (vertex_id, text) = parse_vertex_line(line)?;

        let Some(vertex) = self.vertices.get_mut(&vertex_id) else {
            let step = export_step(text)?;
            self.vertices.insert(vertex_id, ExportVertex { step, ended: false });
            let span = self.span_mut(step);
            span.started_at.get_or_insert(at);
            span.running_vertices += 1;
            return Some(step);
        };

        let step = vertex.step;
        if vertex.ended {
            return Some(step);
        }
        let Some(failed) = vertex_outcome(text) else {
            return Some(step);
        };
        vertex.ended = true;
        let span = self.span_mut(step);
        span.running_vertices -= 1;
        span.ended_at = Some(at);
        span.failed |= failed;
        Some(step)
    }

    pub fn timings(&self) -> BuildkitExportTimings {
        BuildkitExportTimings {
            image_push: self.image_push.timing(),
            cache_export: self.cache_export.timing(),
        }
    }

    fn span_mut(&mut self, step: ExportStep) -> &mut StepSpan {
        match step {
            ExportStep::ImagePush => &mut self.image_push,
            ExportStep::CacheExport => &mut self.cache_export,
        }
    }
}

fn parse_vertex_line(line: &str) -> Option<(u32, &str)> {
    let (vertex_id, text) = line.trim().strip_prefix('#')?.split_once(' ')?;
    Some((vertex_id.parse().ok()?, text))
}

fn export_step(vertex_name: &str) -> Option<ExportStep> {
    if vertex_name.starts_with("exporting to image") {
        Some(ExportStep::ImagePush)
    } else if vertex_name.starts_with("exporting cache") {
        Some(ExportStep::CacheExport)
    } else {
        None
    }
}

/// `Some(failed)` when the line closes its vertex.
fn vertex_outcome(text: &str) -> Option<bool> {
    match text.split_whitespace().next()?.trim_end_matches(':') {
        "DONE" | "CACHED" => Some(false),
        "ERROR" | "CANCELED" => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[(u64, &str)]) -> (Instant, BuildkitExportTimings) {
        let origin = Instant::now();
        let mut progress = BuildkitExportProgress::default();
        for (at_secs, line) in lines {
            progress.observe(line, origin + Duration::from_secs(*at_secs));
        }
        (origin, progress.timings())
    }

    fn timing(origin: Instant, start_secs: u64, end_secs: u64, failed: bool) -> Option<ExportStepTiming> {
        Some(ExportStepTiming {
            started_at: origin + Duration::from_secs(start_secs),
            duration: Duration::from_secs(end_secs - start_secs),
            failed,
        })
    }

    #[test]
    fn single_arch_push_spans_the_exporting_to_image_vertex() {
        let (origin, timings) = feed(&[
            (0, "#8 [2/2] RUN echo hello"),
            (1, "#8 0.214 hello"),
            (2, "#8 DONE 0.3s"),
            (3, ""),
            (3, "#9 exporting to image"),
            (4, "#9 exporting layers 0.4s done"),
            (5, "#9 exporting manifest sha256:4b3c done"),
            (9, "#9 pushing layers 3.2s done"),
            (10, "#9 pushing manifest for registry.example.com/app:abc@sha256:4b3c"),
            (11, "#9 pushing manifest for registry.example.com/app:abc@sha256:4b3c 0.5s done"),
            (12, "#9 DONE 9.1s"),
        ]);

        assert_eq!(
            timings,
            BuildkitExportTimings {
                image_push: timing(origin, 3, 12, false),
                cache_export: None,
            }
        );
    }

    #[test]
    fn multi_arch_push_spans_from_first_vertex_start_to_last_vertex_end() {
        let (origin, timings) = feed(&[
            (2, "#20 exporting to image"),
            (4, "#21 exporting to image"),
            (5, "#20 pushing layers 2.0s done"),
            (6, "#20 DONE 4.0s"),
            (9, "#21 pushing layers 4.8s done"),
            (11, "#21 DONE 7.0s"),
        ]);

        assert_eq!(timings.image_push, timing(origin, 2, 11, false));
    }

    #[test]
    fn multi_arch_push_with_a_vertex_still_running_reports_nothing() {
        let (_, timings) = feed(&[
            (2, "#20 exporting to image"),
            (4, "#21 exporting to image"),
            (6, "#20 DONE 4.0s"),
        ]);

        assert_eq!(timings.image_push, None);
    }

    #[test]
    fn cache_export_spans_the_exporting_cache_vertex() {
        let (origin, timings) = feed(&[
            (1, "#9 exporting to image"),
            (5, "#9 DONE 4.0s"),
            (6, "#10 exporting cache to registry"),
            (7, "#10 preparing build cache for export"),
            (9, "#10 writing layer sha256:aa11 2.1s done"),
            (10, "#10 writing cache manifest sha256:bb22 0.3s done"),
            (14, "#10 DONE 8.0s"),
        ]);

        assert_eq!(
            timings,
            BuildkitExportTimings {
                image_push: timing(origin, 1, 5, false),
                cache_export: timing(origin, 6, 14, false),
            }
        );
    }

    #[test]
    fn older_buildkit_cache_export_name_is_recognised() {
        let (origin, timings) = feed(&[(3, "#10 exporting cache"), (7, "#10 DONE 4.0s")]);

        assert_eq!(timings.cache_export, timing(origin, 3, 7, false));
    }

    #[test]
    fn failed_push_ends_the_step_as_failed() {
        let (origin, timings) = feed(&[
            (2, "#9 exporting to image"),
            (4, "#9 pushing layers"),
            (
                30,
                "#9 ERROR: failed to push registry.example.com/app:abc: unexpected status: 500",
            ),
            (30, "ERROR: failed to solve: failed to push registry.example.com/app:abc"),
        ]);

        assert_eq!(timings.image_push, timing(origin, 2, 30, true));
    }

    #[test]
    fn one_failed_vertex_marks_a_multi_arch_push_as_failed() {
        let (origin, timings) = feed(&[
            (2, "#20 exporting to image"),
            (3, "#21 exporting to image"),
            (6, "#20 DONE 4.0s"),
            (8, "#21 CANCELED"),
        ]);

        assert_eq!(timings.image_push, timing(origin, 2, 8, true));
    }

    #[test]
    fn cached_vertex_ends_the_step_successfully() {
        let (origin, timings) = feed(&[(4, "#10 exporting cache to registry"), (5, "#10 CACHED")]);

        assert_eq!(timings.cache_export, timing(origin, 4, 5, false));
    }

    #[test]
    fn output_without_export_vertices_reports_nothing() {
        let (_, timings) = feed(&[
            (0, "#1 [internal] load build definition from Dockerfile"),
            (1, "#1 DONE 0.1s"),
            (2, "#5 [2/2] RUN echo exporting to image"),
            (3, "#5 0.120 exporting to image"),
            (
                4,
                "#5 ERROR: process \"/bin/sh -c exit 1\" did not complete successfully: exit code: 1",
            ),
        ]);

        assert_eq!(timings, BuildkitExportTimings::default());
    }

    #[test]
    fn export_vertex_never_finished_reports_nothing() {
        let (_, timings) = feed(&[(2, "#9 exporting to image"), (4, "#9 pushing layers")]);

        assert_eq!(timings.image_push, None);
    }

    #[test]
    fn lines_after_a_vertex_ended_do_not_move_its_end() {
        let (origin, timings) = feed(&[(2, "#9 exporting to image"), (5, "#9 DONE 3.0s"), (8, "#9 DONE 3.0s")]);

        assert_eq!(timings.image_push, timing(origin, 2, 5, false));
    }

    #[test]
    fn each_line_is_attributed_to_the_export_step_of_its_vertex() {
        let mut progress = BuildkitExportProgress::default();
        let now = Instant::now();

        let steps: Vec<Option<ExportStep>> = [
            "#11 [3/5] RUN npm ci",
            "#11 4.512 added 1204 packages",
            "#12 exporting to image",
            "#13 exporting cache to registry",
            "#12 pushing layers 2.3s done",
            "#12 DONE 4.1s",
            "#12 pushing manifest for registry/api:sha",
            "#13 DONE 1.0s",
            "waiting for the builder",
        ]
        .into_iter()
        .map(|line| progress.observe(line, now))
        .collect();

        assert_eq!(
            steps,
            vec![
                None,
                None,
                Some(ExportStep::ImagePush),
                Some(ExportStep::CacheExport),
                Some(ExportStep::ImagePush),
                Some(ExportStep::ImagePush),
                Some(ExportStep::ImagePush),
                Some(ExportStep::CacheExport),
                None,
            ]
        );
    }
}
