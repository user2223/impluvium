// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

pub mod corpus;
pub mod json;
pub struct Sha256(impluvium_protocol::hash::ContentHasher);
impl Sha256 {
    pub fn new() -> Self {
        Self(impluvium_protocol::hash::Algorithm::Sha256.hasher())
    }
    pub fn digest(bytes: &[u8]) -> [u8; 32] {
        impluvium_protocol::hash::Algorithm::Sha256.digest(bytes)
    }
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    pub fn finalize(self) -> [u8; 32] {
        self.0.finish()
    }
}
impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}
pub mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            out.push(char::from(DIGITS[(byte >> 4) as usize]));
            out.push(char::from(DIGITS[(byte & 15) as usize]));
        }
        out
    }
    pub fn decode(text: &str) -> Result<Vec<u8>, &'static str> {
        if !text.len().is_multiple_of(2) {
            return Err("odd hex length");
        }
        let (pairs, _) = text.as_bytes().as_chunks::<2>();
        pairs
            .iter()
            .map(|pair| {
                let digit = |b: u8| {
                    char::from(b)
                        .to_digit(16)
                        .map(|n| n as u8)
                        .ok_or("hex digit")
                };
                Ok((digit(pair[0])? << 4) | digit(pair[1])?)
            })
            .collect()
    }
    pub fn decode_to_slice(text: &str, out: &mut [u8]) -> Result<(), &'static str> {
        let bytes = decode(text)?;
        if bytes.len() != out.len() {
            return Err("hex length");
        }
        out.copy_from_slice(&bytes);
        Ok(())
    }
}
