use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, SamplingMode};

const DEFAULT_SAMPLE_SIZE: usize = 10;
const PROCESS_WARM_UP: Duration = Duration::from_millis(250);
const PROCESS_MEASUREMENT_TIME: Duration = Duration::from_secs(2);
const RUNNER_MEASUREMENT_TIME: Duration = Duration::from_secs(4);
const PROCESS_NOISE_THRESHOLD: f64 = 0.25;
const PROCESS_SIGNIFICANCE_LEVEL: f64 = 0.01;

pub fn configure_pure_group(group: &mut BenchmarkGroup<'_, WallTime>) {
    configure_group(group, Duration::from_millis(300));
}

pub fn configure_process_group(group: &mut BenchmarkGroup<'_, WallTime>) {
    configure_group(group, PROCESS_MEASUREMENT_TIME);
    group.warm_up_time(PROCESS_WARM_UP);
    group.noise_threshold(PROCESS_NOISE_THRESHOLD);
    group.significance_level(PROCESS_SIGNIFICANCE_LEVEL);
    group.sampling_mode(SamplingMode::Flat);
}

pub fn configure_runner_group(group: &mut BenchmarkGroup<'_, WallTime>) {
    configure_group(group, RUNNER_MEASUREMENT_TIME);
    group.warm_up_time(PROCESS_WARM_UP);
    group.noise_threshold(PROCESS_NOISE_THRESHOLD);
    group.significance_level(PROCESS_SIGNIFICANCE_LEVEL);
    group.sampling_mode(SamplingMode::Flat);
}

fn configure_group(group: &mut BenchmarkGroup<'_, WallTime>, measurement_time: Duration) {
    group.sample_size(DEFAULT_SAMPLE_SIZE);
    group.warm_up_time(Duration::from_millis(100));
    group.measurement_time(measurement_time);
}
