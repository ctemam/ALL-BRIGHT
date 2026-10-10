//! Append-only journal. Every event that hits the bus is journaled as a
//! BSON line so `allbright-research` can replay into the identical pipeline
//! (deterministic replay parity per the plan).
//!
//! Bounded retention (§ops): files roll at JOURNAL_MAX_FILE_MB (default
//! 512 MB) into `journal-<day>-<seq>.bsonl`, and the whole journal dir is
//! capped at JOURNAL_MAX_TOTAL_GB (default 20 GB) — oldest files deleted
//! first, never the active file. Unbounded journals were hitting ~15 GB/day.

use anyhow::Result;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use allbright_core::{bus, events::Event};

/// Per-file rollover bound and whole-dir retention cap.
fn max_file_bytes() -> u64 {
    std::env::var("JOURNAL_MAX_FILE_MB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(512)
        * 1_048_576
}

fn max_total_bytes() -> u64 {
    std::env::var("JOURNAL_MAX_TOTAL_GB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20)
        * 1_073_741_824
}

pub struct Journal {
    dir: PathBuf,
    file: std::fs::File,
    path: PathBuf,
    day: String,
    seq: u32,
    /// Bytes written to the active file — rollover trigger.
    written: u64,
}

impl Journal {
    pub fn new(dir: &str) -> Result<Self> {
        create_dir_all(dir)?;
        let day = today();
        let seq = 0;
        let (file, path) = open_for(Path::new(dir), &day, seq)?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut j = Journal { dir: dir.into(), file, path, day, seq, written };
        j.retention()?;
        j.marker(
            "journal_open",
            format!("{} ({}B carried)", j.path.display(), written),
        );
        Ok(j)
    }

    pub fn record(&mut self, ev: &Event) -> Result<()> {
        let day = today();
        if day != self.day {
            self.day = day;
            self.seq = 0;
            let (file, path) = open_for(self.dir.as_path(), &self.day, self.seq)?;
            self.marker("journal_rotate", format!("→ {}", path.display()));
            self.file = file;
            self.path = path;
            self.written = 0;
        }
        let bytes = bus::encode(ev)?;
        self.file.write_all(&(bytes.len() as u32).to_le_bytes())?;
        self.file.write_all(&bytes)?;
        self.written += 4 + bytes.len() as u64;
        if self.written >= max_file_bytes() {
            self.seq += 1;
            let (file, path) = open_for(self.dir.as_path(), &self.day, self.seq)?;
            self.marker("journal_rotate", format!("→ {}", path.display()));
            self.file = file;
            self.path = path;
            self.written = 0;
            self.retention()?;
        }
        Ok(())
    }

    /// Journaled lifecycle marker — restart/rotation attribution.
    fn marker(&mut self, action: &str, detail: String) {
        let ev = Event::System(allbright_core::events::SystemEvent {
            component: "ingest.recorder".into(),
            action: action.into(),
            detail,
            ts_ms: allbright_core::events::now_ms(),
        });
        if let Ok(bytes) = bus::encode(&ev) {
            let _ = self.file.write_all(&(bytes.len() as u32).to_le_bytes());
            let _ = self.file.write_all(&bytes);
            self.written += 4 + bytes.len() as u64;
        }
    }

    /// Oldest-first deletion while the journal dir exceeds the cap.
    /// The active file is never a deletion candidate.
    fn retention(&self) -> Result<()> {
        let mut files: Vec<(PathBuf, u64)> = std::fs::read_dir(&self.dir)?
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.starts_with("journal-") && n.ends_with(".bsonl")
            })
            .filter_map(|e| {
                e.metadata().ok().map(|m| (e.path(), m.len()))
            })
            .collect();
        files.sort_by(|(a, _), (b, _)| a.cmp(b)); // day+seq lexical = chronological
        let mut total: u64 = files.iter().map(|(_, s)| *s).sum();
        for (path, size) in files {
            if total <= max_total_bytes() {
                break;
            }
            if path == self.path {
                continue;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
        Ok(())
    }
}

fn open_for(dir: &Path, day: &str, seq: u32) -> Result<(std::fs::File, PathBuf)> {
    let name = if seq == 0 {
        format!("journal-{day}.bsonl")
    } else {
        format!("journal-{day}-{seq:03}.bsonl")
    };
    let path = dir.join(name);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((file, path))
}

fn today() -> String {
    // YYYYMMDD from unix time — no chrono dep.
    let secs = allbright_core::events::now_ms() / 1000;
    let days = secs / 86400;
    // civil-from-days algorithm
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}
