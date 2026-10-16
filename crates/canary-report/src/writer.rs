//! Atomic file writes for rendered reports.
//!
//! `--output` must never leave a truncated or partially written report on
//! disk: a consumer (or the next run) reading the file after a crash would
//! see unparseable JSON. Writing to a sibling temporary file and renaming
//! it into place makes the replacement atomic — the destination always
//! holds either the old bytes or the new ones, never a mixture.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Writes `content` to `path` atomically and returns only once the new
/// bytes are in place.
///
/// The content is written to a temporary file with a `.tmp` suffix in
/// `path`'s own directory, flushed to disk, and then renamed over `path`.
/// Because the rename happens on the same filesystem it is atomic: a reader
/// at any moment sees either the complete previous report or the complete
/// new one — a crash, `SIGKILL` or a full disk can no longer leave a
/// truncated file at `path`.
///
/// Guarantees:
///
/// - **Permissions are preserved.** When `path` already exists, its
///   permissions are copied onto the replacement before the rename, so
///   overwriting a report never widens or drops the file's mode.
/// - **No orphans.** If any step fails, the temporary file is removed
///   again before the error is returned; a failed write leaves the
///   directory as it was.
/// - **Actionable errors.** The returned error names `path`, so a caller
///   (the CLI's `--output`) can point at the unusable destination.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// canary_report::atomic_write_report(Path::new("report.json"), "{}\n")?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn atomic_write_report(path: &Path, content: &str) -> io::Result<()> {
    let temp = temp_path(path)?;
    if let Err(err) = write_via_temp(&temp, path, content) {
        // Best effort: never leave a partial `.tmp` file behind, whatever
        // went wrong (unwritable directory, full disk, ...).
        let _ = fs::remove_file(&temp);
        return Err(io::Error::new(
            err.kind(),
            format!("failed to write report to {}: {err}", path.display()),
        ));
    }
    Ok(())
}

/// The sibling temporary file used for `path`: the same directory, the same
/// file name plus a `.tmp` suffix — same directory so the final rename
/// stays on one filesystem and is atomic.
fn temp_path(path: &Path) -> io::Result<PathBuf> {
    if path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} names no report file", path.display()),
        ));
    }
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".tmp");
    Ok(PathBuf::from(temp))
}

fn write_via_temp(temp: &Path, destination: &Path, content: &str) -> io::Result<()> {
    let mut file = fs::File::create(temp)?;
    file.write_all(content.as_bytes())?;
    // Flush before the rename so the destination never names data that is
    // still only in this process's buffers.
    file.sync_all()?;
    drop(file);

    // Carry an existing report's permissions onto the replacement. Skipped
    // for a first write, which gets the default permissions a plain create
    // would have produced.
    if let Ok(metadata) = fs::metadata(destination) {
        fs::set_permissions(temp, metadata.permissions())?;
    }

    fs::rename(temp, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_without_a_file_name_is_rejected_before_any_write() {
        let err = atomic_write_report(Path::new("no-file-name/.."), "x").unwrap_err();
        assert!(
            err.to_string().contains("names no report file"),
            "actionable message, got: {err}"
        );
    }

    #[test]
    fn the_temp_file_is_the_destination_with_a_tmp_suffix_in_the_same_directory() {
        let temp = temp_path(Path::new("out/report.json")).unwrap();
        assert_eq!(temp, PathBuf::from("out/report.json.tmp"));
    }
}
