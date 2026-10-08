//! Append-only journal. Every event that hits the bus is journaled as a
//! BSON line so `allbright-research` can replay into the identical pipeline
//! (deterministic replay parity per the plan).

use anyhow::Result;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use allbright_core::{bus, events::Event};

pub struct Journal {
    dir: PathBuf,
    file: std::fs::File,
    day: String,
}

impl Journal {
    pub fn new(dir: &str) -> Result<Self> {
        create_dir_all(dir)?;
        let day = today();
        let file = open_for(Path::new(dir), &day)?;
        Ok(Journal { dir: dir.into(), file, day })
    }

    pub fn record(&mut self, ev: &Event) -> Result<()> {
        let day = today();
        if day != self.day {
            self.file = open_for(self.dir.as_path(), &day)?;
            self.day = day;
        }
        let bytes = bus::encode(ev)?;
        self.file.write_all(&(bytes.len() as u32).to_le_bytes())?;
        self.file.write_all(&bytes)?;
        Ok(())
    }
}

fn open_for(dir: &Path, day: &str) -> Result<std::fs::File> {
    let path = dir.join(format!("journal-{day}.bsonl"));
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
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
