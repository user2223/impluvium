// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The process panic hook: a real panic is logged by location only.

#[path = "../src/logging.rs"]
mod logging;

use std::io::Write;
use std::sync::{Arc, Mutex};

use logging::{JsonLogger, install_panic_hook};

#[derive(Clone, Default)]
struct Memory(Arc<Mutex<Vec<u8>>>);

impl Write for Memory {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_panic_payload_never_reaches_the_log() {
    let memory = Memory::default();
    install_panic_hook(Arc::new(JsonLogger::new(Box::new(memory.clone()))));
    let result = std::panic::catch_unwind(|| {
        let secret = "SECRET-PANIC-PAYLOAD /home/someone/.secret";
        panic!("{secret}");
    });
    assert!(result.is_err());
    let text = String::from_utf8(memory.0.lock().unwrap().clone()).unwrap();
    assert!(!text.contains("SECRET-PANIC-PAYLOAD"), "{text}");
    assert!(!text.contains("/home/someone"), "{text}");
    let line: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(line["message"], "panic");
    assert!(
        line["file"]
            .as_str()
            .unwrap()
            .ends_with("server_bin_logging.rs")
    );
    assert!(line["line"].as_u64().unwrap() > 0);
}
