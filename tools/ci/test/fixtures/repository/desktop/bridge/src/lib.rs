#![feature(existing_gate)]
#![cfg_attr(
    feature = "experimental",
    cfg_attr(all(unix, not(test)), feature(conditional_gate)),
    allow(dead_code)
)]
pub fn example<'a>(value: &'a str) -> &'a str { value }
