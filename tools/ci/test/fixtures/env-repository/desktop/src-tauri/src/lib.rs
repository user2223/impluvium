use std::{env, fs};

const BUILT: Option<&str> = option_env!("EXAMPLE_BUILT");
const DIR: &str = env!("EXAMPLE_MANIFEST");

pub fn read() {
    // std::env::var("EXAMPLE_COMMENT") in a comment is not a read.
    let level = env::var("EXAMPLE_LOG").unwrap_or_default();
    let home = std::env::var_os("EXAMPLE_HOME");
    let _ = (BUILT, DIR, level, home, fs::read("example.txt"));
}

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {}
}
