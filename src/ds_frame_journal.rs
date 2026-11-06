/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Opt-in actual guest-present timings; never overwrite an existing capture.
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub(crate) struct Journal {
    writer: BufWriter<File>,
    remaining: u64,
    pending: u32,
}

impl Journal {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "frame,interval_us")?;
        writer.flush()?;
        Ok(Self { writer, remaining: 1_000_000, pending: 0 })
    }

    pub(crate) fn record(&mut self, frame: u64, interval_us: u128) -> io::Result<()> {
        if self.remaining == 0 { return Ok(()); }
        // Disable after any I/O error; a full disk must not retry each frame.
        self.remaining -= 1;
        let result = (|| {
            writeln!(self.writer, "{},{}", frame, interval_us)?;
            self.pending += 1;
            if self.pending >= 30 || self.remaining == 0 {
                self.writer.flush()?;
                self.pending = 0;
            }
            Ok(())
        })();
        if result.is_err() { self.remaining = 0; }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_preserves_long_intervals_and_never_replaces_an_existing_file() {
        let path = std::env::temp_dir().join(format!("ds-frame-journal-{}-{}.csv",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut journal = Journal::open(&path).unwrap();
        journal.remaining = 2;
        journal.record(10, 16_667).unwrap();
        journal.record(11, 8_000_000).unwrap();
        journal.record(12, 42).unwrap();
        assert!(Journal::open(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "frame,interval_us\n10,16667\n11,8000000\n");
        drop(journal);
        std::fs::remove_file(path).unwrap();
    }
}
