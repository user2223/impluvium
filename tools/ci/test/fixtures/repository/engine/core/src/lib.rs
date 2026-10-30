// #![feature(commented_out)]
/* A nested /* #![feature(also_commented)] */ block comment. */
pub const EXAMPLE: &str = r###"#![feature(in_a_string)]"###;
pub const HOST: &str = "https://api.example.invalid/192.0.2.10";
pub fn identity(value: u8) -> u8 { value }
