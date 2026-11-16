// SPDX-License-Identifier: 0BSD
#[macro_export]
macro_rules! progress { ($($arg:tt)*) => { eprintln!($($arg)*); }; }
pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}
pub fn io_err<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}
pub fn median(v: &[f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[s.len() / 2]
}
