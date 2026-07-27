use std::fs;
use std::time::{Duration, Instant};

use tempfile::tempdir;

use super::*;

mod context_files;
mod dialect_fallback;
mod output_behavior;
mod pattern_behavior;
mod reporting;
mod rg_execution;
