// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Adapted from the protocol crate corpus helper; hashes and the data-only JSON
//! reader use existing protocol facilities instead of additional dependencies.

use super::Sha256;
use super::hex;
use super::json::{self, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

pub struct Corpus {
    root: PathBuf,
    index: Value,
    recipes: BTreeMap<String, Value>,
}

pub fn text(value: &Value) -> &str {
    value.as_str().expect("corpus string")
}
pub fn number(value: &Value) -> usize {
    value.as_u64().expect("corpus unsigned integer") as usize
}
fn decimal(value: &Value) -> usize {
    text(value).parse().expect("corpus decimal")
}
pub fn array(value: &Value) -> &Vec<Value> {
    value.as_array().expect("corpus array")
}

impl Corpus {
    pub fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol/testdata");
        let index: Value = json::from_slice(&fs::read(root.join("INDEX.json")).unwrap()).unwrap();
        assert_eq!(index["format"], "corpus-index-v2");
        let mut corpus = Self {
            root,
            index,
            recipes: BTreeMap::new(),
        };
        let files: Vec<String> = array(&corpus.index["assets"])
            .iter()
            .map(|asset| text(&asset["file"]))
            .filter(|name| name.starts_with("recipes-") && name.ends_with(".json"))
            .map(str::to_owned)
            .collect();
        for file in files {
            let asset = corpus.pinned(&file);
            assert_eq!(asset["format"], "byte-recipes-v1");
            for recipe in array(&asset["recipes"]) {
                assert!(
                    corpus
                        .recipes
                        .insert(text(&recipe["id"]).to_owned(), recipe.clone())
                        .is_none()
                );
            }
        }
        assert!(corpus.recipes.len() <= 8192);
        corpus
    }

    pub fn pinned(&self, file: &str) -> Value {
        assert!(!file.contains('/') && !file.contains('\\'));
        let pin = array(&self.index["groups"])
            .iter()
            .chain(array(&self.index["assets"]))
            .find(|entry| entry["file"] == file)
            .expect("indexed asset");
        let path = self.root.join(file);
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(metadata.is_file() && metadata.len() <= 1_048_576);
        let bytes = fs::read(path).unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            text(&pin["sha256"]),
            "{file} pin"
        );
        let value: Value = json::from_slice(&bytes).unwrap();
        if let Some(count) = pin["count"].as_u64() {
            assert_eq!(array(&value["cases"]).len() as u64, count);
        }
        value
    }

    pub fn bytes(&self, value: &Value) -> Vec<u8> {
        if let Some(hex) = value.as_str() {
            return hex::decode(hex).expect("corpus hex");
        }
        assert_eq!(value.as_object().expect("byte marker").len(), 1);
        self.recipe_bytes(text(&value["$bytes"]))
    }

    fn recipe_bytes(&self, id: &str) -> Vec<u8> {
        self.recipe(id, 1, &mut 0, &mut BTreeSet::new(), &mut BTreeSet::new())
    }

    fn recipe(
        &self,
        id: &str,
        depth: usize,
        visits: &mut usize,
        reachable: &mut BTreeSet<String>,
        active: &mut BTreeSet<String>,
    ) -> Vec<u8> {
        *visits += 1;
        reachable.insert(id.to_owned());
        assert!(depth <= 32 && *visits <= 4096 && reachable.len() <= 256);
        assert!(active.insert(id.to_owned()), "cyclic recipe");
        let recipe = self.recipes.get(id).expect("recipe exists");
        let length = decimal(&recipe["byteLengthU64"]);
        assert!(length <= 33_554_432, "materialization limit");
        let mut child = |id: &str| self.recipe(id, depth + 1, visits, reachable, active);
        let bytes = match text(&recipe["op"]) {
            "literal" => {
                let bytes = hex::decode(text(&recipe["hex"])).unwrap();
                assert!(bytes.len() <= 65536);
                bytes
            }
            "repeat" => {
                let source = child(text(&recipe["source"]));
                let count = decimal(&recipe["countU64"]);
                assert_eq!(source.len().checked_mul(count), Some(length));
                source.repeat(count)
            }
            "concat" => {
                let parts = array(&recipe["parts"]);
                assert!(parts.len() <= 128);
                let mut result = Vec::new();
                for id in parts {
                    let part = child(text(id));
                    assert!(result.len() + part.len() <= length);
                    result.extend_from_slice(&part);
                }
                result
            }
            "slice" => {
                let source = child(text(&recipe["source"]));
                let from = decimal(&recipe["fromU64"]);
                let take = decimal(&recipe["takeU64"]);
                assert_eq!(length, take);
                source[from..from.checked_add(take).unwrap()].to_vec()
            }
            "insert" => {
                let source = child(text(&recipe["source"]));
                let insert = child(text(&recipe["insert"]));
                let at = decimal(&recipe["atU64"]);
                assert_eq!(source.len() + insert.len(), length);
                [&source[..at], insert.as_slice(), &source[at..]].concat()
            }
            "counter" => {
                let start: u64 = text(&recipe["startU64"]).parse().unwrap();
                let step: u64 = text(&recipe["stepU64"]).parse().unwrap();
                let width = number(&recipe["width"]);
                assert!(matches!(width, 1 | 2 | 4 | 8));
                assert_eq!(decimal(&recipe["outputBytesU64"]), length);
                let mut result = Vec::with_capacity(length);
                let mut word = start;
                while result.len() < length {
                    let take = width.min(length - result.len());
                    match text(&recipe["endian"]) {
                        "little" => result.extend_from_slice(&word.to_le_bytes()[..take]),
                        "big" => result
                            .extend_from_slice(&word.to_be_bytes()[8 - width..8 - width + take]),
                        other => panic!("unknown counter endian {other}"),
                    }
                    word = word.wrapping_add(step);
                }
                result
            }
            other => panic!("unknown byte recipe operation {other}"),
        };
        assert_eq!(bytes.len(), length, "recipe {id} length");
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            text(&recipe["sha256Hex"]),
            "recipe {id} hash"
        );
        active.remove(id);
        bytes
    }
}

/// Resolve expectation metadata without changing the pinned corpus. Byte and
/// text references use the same checked recipe expansion as input references.
pub fn expected(corpus: &Corpus, shard: &Value, case: &Value) -> Value {
    fn resolve(corpus: &Corpus, shard: &Value, value: &Value, depth: usize) -> Value {
        assert!(depth <= 64, "expectation reference depth");
        match value {
            Value::Object(fields) => {
                if fields.len() == 1 {
                    if let Some(key) = fields.get("$value") {
                        let referenced = shard["values"]
                            .get(text(key))
                            .expect("expectation value exists");
                        return resolve(corpus, shard, referenced, depth + 1);
                    }
                    if fields.contains_key("$bytes") {
                        return Value::String(hex::encode(corpus.bytes(value)));
                    }
                    if let Some(id) = fields.get("$utf8") {
                        return Value::String(
                            String::from_utf8(corpus.recipe_bytes(text(id)))
                                .expect("expectation recipe is strict UTF-8"),
                        );
                    }
                }
                Value::Object(
                    fields
                        .iter()
                        .map(|(key, value)| (key.clone(), resolve(corpus, shard, value, depth + 1)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| resolve(corpus, shard, value, depth + 1))
                    .collect(),
            ),
            _ => value.clone(),
        }
    }
    resolve(corpus, shard, &case["expected"], 0)
}

impl Corpus {
    /// Validate the recipe graph and hash every reachable stream in bounded blocks.
    /// Large size probes never materialize their designated body.
    pub fn stream(&self, id: &str, mut sink: impl FnMut(&[u8])) -> u64 {
        fn graph(
            c: &Corpus,
            id: &str,
            depth: usize,
            visits: &mut usize,
            active: &mut BTreeSet<String>,
            reachable: &mut BTreeSet<String>,
        ) -> u64 {
            *visits += 1;
            assert!(depth <= 32 && *visits <= 4096);
            assert!(active.insert(id.to_owned()), "cyclic recipe");
            reachable.insert(id.to_owned());
            assert!(reachable.len() <= 256);
            let r = &c.recipes[id];
            let mut child = |id: &str| graph(c, id, depth + 1, visits, active, reachable);
            let length = match text(&r["op"]) {
                "literal" => {
                    let n = text(&r["hex"]).len() / 2;
                    assert!(n <= 65536);
                    n as u64
                }
                "repeat" => child(text(&r["source"]))
                    .checked_mul(text(&r["countU64"]).parse().unwrap())
                    .unwrap(),
                "concat" => {
                    assert!(array(&r["parts"]).len() <= 128);
                    array(&r["parts"])
                        .iter()
                        .map(|v| child(text(v)))
                        .try_fold(0u64, u64::checked_add)
                        .unwrap()
                }
                "slice" => {
                    let n = child(text(&r["source"]));
                    let from: u64 = text(&r["fromU64"]).parse().unwrap();
                    let take: u64 = text(&r["takeU64"]).parse().unwrap();
                    assert!(from <= n && take <= n - from);
                    take
                }
                "insert" => {
                    let n = child(text(&r["source"]));
                    assert!(text(&r["atU64"]).parse::<u64>().unwrap() <= n);
                    n.checked_add(child(text(&r["insert"]))).unwrap()
                }
                "counter" => {
                    let width = number(&r["width"]);
                    assert!(matches!(width, 1 | 2 | 4 | 8));
                    let start: u64 = text(&r["startU64"]).parse().unwrap();
                    let step: u64 = text(&r["stepU64"]).parse().unwrap();
                    if width < 8 {
                        assert!(start < (1 << (width * 8)) && step < (1 << (width * 8)));
                    }
                    text(&r["outputBytesU64"]).parse().unwrap()
                }
                other => panic!("unknown byte recipe operation {other}"),
            };
            assert_eq!(length, text(&r["byteLengthU64"]).parse::<u64>().unwrap());
            assert!(length <= 1_073_741_824);
            active.remove(id);
            length
        }
        let mut reachable = BTreeSet::new();
        let length = graph(self, id, 1, &mut 0, &mut BTreeSet::new(), &mut reachable);
        let mut block = vec![0u8; 65536];
        for node in reachable {
            let size: u64 = text(&self.recipes[&node]["byteLengthU64"]).parse().unwrap();
            let mut hash = Sha256::new();
            let mut pos = 0;
            while pos < size {
                let take = ((size - pos).min(65536)) as usize;
                self.range(&node, pos, &mut block[..take]);
                hash.update(&block[..take]);
                pos += take as u64;
            }
            assert_eq!(
                hex::encode(hash.finalize()),
                text(&self.recipes[&node]["sha256Hex"]),
                "recipe {node} hash"
            );
        }
        let mut pos = 0;
        while pos < length {
            let take = (length - pos).min(65536) as usize;
            self.range(id, pos, &mut block[..take]);
            sink(&block[..take]);
            pos += take as u64;
        }
        length
    }

    fn range(&self, id: &str, from: u64, out: &mut [u8]) {
        if out.is_empty() {
            return;
        }
        let r = &self.recipes[id];
        let length = |id: &str| {
            text(&self.recipes[id]["byteLengthU64"])
                .parse::<u64>()
                .unwrap()
        };
        match text(&r["op"]) {
            "literal" => {
                let start = from as usize * 2;
                hex::decode_to_slice(&text(&r["hex"])[start..start + out.len() * 2], out).unwrap();
            }
            "repeat" => {
                let source = text(&r["source"]);
                let size = length(source);
                assert!(size != 0);
                let mut used = 0;
                while used < out.len() {
                    let at = (from + used as u64) % size;
                    let take = (size - at).min((out.len() - used) as u64) as usize;
                    // Once one full period has been filled, copy the already
                    // verified pattern within this fixed-size processing block.
                    if used as u64 >= size {
                        let source_start = (used as u64 % size) as usize;
                        let take = (out.len() - used).min(used - source_start);
                        out.copy_within(source_start..source_start + take, used);
                        used += take;
                    } else {
                        self.range(source, at, &mut out[used..used + take]);
                        used += take;
                    }
                }
            }
            "slice" => self.range(
                text(&r["source"]),
                from + text(&r["fromU64"]).parse::<u64>().unwrap(),
                out,
            ),
            "concat" => {
                let mut skip = from;
                let mut used = 0;
                for part in array(&r["parts"]) {
                    let part = text(part);
                    let size = length(part);
                    if skip >= size {
                        skip -= size;
                        continue;
                    }
                    let take = (size - skip).min((out.len() - used) as u64) as usize;
                    self.range(part, skip, &mut out[used..used + take]);
                    used += take;
                    skip = 0;
                    if used == out.len() {
                        break;
                    }
                }
                assert_eq!(used, out.len());
            }
            "insert" => {
                let source = text(&r["source"]);
                let insert = text(&r["insert"]);
                let at = text(&r["atU64"]).parse::<u64>().unwrap();
                let size = length(insert);
                let mut pos = from;
                let mut used = 0;
                while used < out.len() {
                    let (id, offset, remain) = if pos < at {
                        (source, pos, at - pos)
                    } else if pos < at + size {
                        (insert, pos - at, at + size - pos)
                    } else {
                        (source, pos - size, u64::MAX)
                    };
                    let take = remain.min((out.len() - used) as u64) as usize;
                    self.range(id, offset, &mut out[used..used + take]);
                    used += take;
                    pos += take as u64;
                }
            }
            "counter" => {
                let width = number(&r["width"]);
                let start: u64 = text(&r["startU64"]).parse().unwrap();
                let step: u64 = text(&r["stepU64"]).parse().unwrap();
                for (i, b) in out.iter_mut().enumerate() {
                    let offset = from + i as u64;
                    let word = start.wrapping_add((offset / width as u64).wrapping_mul(step));
                    let byte = offset as usize % width;
                    *b = match text(&r["endian"]) {
                        "little" => word.to_le_bytes()[byte],
                        "big" => word.to_be_bytes()[8 - width + byte],
                        other => panic!("unknown endian {other}"),
                    };
                }
            }
            other => panic!("unknown byte recipe operation {other}"),
        }
    }
}

impl Corpus {
    pub fn groups(&self) -> Vec<String> {
        array(&self.index["groups"])
            .iter()
            .map(|g| text(&g["file"]).to_owned())
            .collect()
    }
}
