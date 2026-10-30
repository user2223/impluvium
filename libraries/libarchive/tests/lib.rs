extern crate libarchive_drop;

pub mod util;

use libarchive_drop::archive::{self, Entry, ReadFilter, ReadFormat};
use libarchive_drop::error::ArchiveError;
use libarchive_drop::reader::{self, Reader};
use libarchive_drop::writer;
use std::fs::{self, File};
use util::path::TempDir;

/// The single file stored in `sample.tar.gz`.
const SAMPLE_FILE: &str = "hello.txt";
const SAMPLE_CONTENT: &[u8] = b"hello, world!\n";
const SAMPLE_FILE_SIZE: usize = SAMPLE_CONTENT.len();

fn sample_builder() -> reader::Builder {
    let mut builder = reader::Builder::new();
    builder.support_format(ReadFormat::All).ok();
    builder.support_filter(ReadFilter::All).ok();
    builder
}

#[test]
fn reading_from_file() {
    let tar = util::path::fixture("sample.tar.gz");
    let mut reader = sample_builder().open_file(tar).unwrap();
    let entry = reader.next_header().expect("archive has an entry");
    assert_eq!(entry.pathname(), SAMPLE_FILE);
    assert_eq!(entry.size(), SAMPLE_FILE_SIZE as i64);
    assert!(reader.next_header().is_none());
}

#[test]
fn read_archive_from_stream() {
    let tar = util::path::fixture("sample.tar.gz");
    let f = File::open(tar).unwrap();
    let mut reader = sample_builder().open_stream(f).unwrap();
    assert_eq!(reader.header_position(), 0);
    let out = TempDir::new("read_archive_from_stream");
    let count = reader_write(&mut reader, Some(out.as_str()));
    assert_eq!(count, SAMPLE_FILE_SIZE);
    assert_eq!(reader.header_position(), 1024);
    assert_extracted(&out);
}

#[test]
fn extracting_from_file() {
    let tar = util::path::fixture("sample.tar.gz");
    let mut reader = sample_builder().open_file(tar).unwrap();
    let out = TempDir::new("extracting_from_file");
    let count = reader_write(&mut reader, Some(out.as_str()));
    assert_eq!(count, SAMPLE_FILE_SIZE);
    assert_extracted(&out);
}

#[test]
fn extracting_an_archive_with_options() {
    let tar = util::path::fixture("sample.tar.gz");
    let mut reader = sample_builder().open_file(tar).unwrap();
    let mut opts = archive::ExtractOptions::new();
    opts.add(archive::ExtractOption::Time);
    let writer = writer::Disk::new();
    writer.set_options(&opts).unwrap();
    let out = TempDir::new("extracting_an_archive_with_options");
    let count = writer
        .write(&mut reader, Some(out.as_str()))
        .expect("extraction succeeds");
    assert_eq!(count, SAMPLE_FILE_SIZE);
    assert_extracted(&out);
}

#[test]
fn extracting_a_reader_twice() {
    let tar = util::path::fixture("sample.tar.gz");
    let mut reader = sample_builder().open_file(tar).unwrap();
    let out = TempDir::new("extracting_a_reader_twice");
    let writer = writer::Disk::new();
    writer
        .write(&mut reader, Some(out.as_str()))
        .expect("first extraction succeeds");
    // The reader has been consumed, so a second extraction must be refused.
    match writer.write(&mut reader, Some(out.as_str())) {
        Err(ArchiveError::HeaderPosition) => {}
        other => panic!(
            "expected a header position error, got {:?}",
            other.map(|_| ())
        ),
    }
}

fn reader_write<T: Reader>(reader: &mut T, prefix: Option<&str>) -> usize {
    writer::Disk::new()
        .write(reader, prefix)
        .expect("extraction succeeds")
}

fn assert_extracted(out: &TempDir) {
    let file = out.path().join(SAMPLE_FILE);
    let content = fs::read(&file).expect("extracted file exists");
    assert_eq!(content, SAMPLE_CONTENT);
}
