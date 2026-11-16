// SPDX-License-Identifier: 0BSD
//! Page-cache control for cold/warm runs (Linux). Elsewhere the functions report "unknown" (-1.0).
use std::path::Path;

#[cfg(target_os = "linux")]
pub fn drop_file(p: &Path) {
    use std::os::unix::io::AsRawFd;
    if let Ok(f) = std::fs::File::open(p) {
        unsafe {
            libc::fsync(f.as_raw_fd());
            libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
        }
    }
}

/// Fraction (0..1) of the file's pages resident in the page cache, via mincore on a read-only mapping.
#[cfg(target_os = "linux")]
pub fn resident(p: &Path, size: u64) -> Option<(u64, u64)> {
    use std::os::unix::io::AsRawFd;
    if size == 0 {
        return Some((0, 0));
    }
    let f = std::fs::File::open(p).ok()?;
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE) as u64;
        let len = size as usize;
        let addr = libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            f.as_raw_fd(),
            0,
        );
        if addr == libc::MAP_FAILED {
            return None;
        }
        let pages = size.div_ceil(page) as usize;
        let mut vec = vec![0u8; pages];
        let r = libc::mincore(addr, len, vec.as_mut_ptr());
        libc::munmap(addr, len);
        if r != 0 {
            return None;
        }
        Some((
            vec.iter().filter(|b| **b & 1 == 1).count() as u64,
            pages as u64,
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub fn drop_file(_p: &Path) {}
#[cfg(not(target_os = "linux"))]
pub fn resident(_p: &Path, _size: u64) -> Option<(u64, u64)> {
    None
}

/// Resident fraction over a set of files, -1.0 when unknown.
pub fn resident_fraction(files: &[(std::path::PathBuf, u64)]) -> f64 {
    let (mut r, mut t) = (0u64, 0u64);
    for (p, s) in files {
        match resident(p, *s) {
            Some((a, b)) => {
                r += a;
                t += b;
            }
            None => return -1.0,
        }
    }
    if t == 0 {
        1.0
    } else {
        r as f64 / t as f64
    }
}
