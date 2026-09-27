//! The logger's tests, split by the module they cover.

use super::*;

use std::io::Read;
use tempfile::tempdir;

mod clock;
mod level;
mod parse;
mod privacy;
mod record;
mod retention;
mod writer;
