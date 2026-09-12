//! Filesystem persistence for the sample platformer's `.platformer` levels.
//!
//! The parser and writer remain owned by [`crate::level::Level`]. This module
//! provides the shared file operations used by game commands and editor code.

use core::{error::Error, fmt};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::level::{Level, ParseError};

static TEMPORARY_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
const TEMPORARY_FILE_ATTEMPTS: usize = 100;
const TEMPORARY_FILE_PREFIX: &str = ".platformer-level-";

/// Selects whether a level save may create or replace its destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveMode {
    /// Create the destination and fail if a path is already occupied.
    CreateNew,
    /// Replace an existing destination and fail if it does not exist.
    ReplaceExisting,
}

impl fmt::Display for SaveMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateNew => formatter.write_str("create a new"),
            Self::ReplaceExisting => formatter.write_str("replace the existing"),
        }
    }
}

/// An error encountered while loading a level file.
#[derive(Debug)]
pub enum LoadError {
    /// The level file could not be read as UTF-8 text.
    Read {
        /// The requested level path.
        path: PathBuf,
        /// The filesystem or UTF-8 error reported by the read operation.
        source: io::Error,
    },
    /// The file was read but did not contain one complete valid level.
    Parse {
        /// The requested level path.
        path: PathBuf,
        /// The parser's line and record context.
        source: ParseError,
    },
}

impl LoadError {
    /// Returns the path involved in the failed load.
    pub fn path(&self) -> &Path {
        match self {
            Self::Read { path, .. } | Self::Parse { path, .. } => path,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "failed to read level file {path:?}: {source}")
            }
            Self::Parse { path, source } => {
                write!(formatter, "failed to parse level file {path:?}: {source}")
            }
        }
    }
}

impl Error for LoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

/// An error encountered while saving a level file.
#[derive(Debug)]
pub enum SaveError {
    /// The destination does not name a file path.
    InvalidDestination {
        /// The requested level path.
        path: PathBuf,
    },
    /// A temporary file could not be created in the destination directory.
    TemporaryCreate {
        /// The requested level path.
        destination: PathBuf,
        /// The temporary path that was attempted.
        path: PathBuf,
        /// The filesystem error reported by temporary-file creation.
        source: io::Error,
    },
    /// The serialized level could not be completely written to the temporary file.
    TemporaryWrite {
        /// The requested level path.
        destination: PathBuf,
        /// The temporary path receiving the level contents.
        path: PathBuf,
        /// The filesystem error reported by writing or flushing the temporary file.
        source: io::Error,
    },
    /// Replacement was requested, but no destination currently exists.
    DestinationMissing {
        /// The requested level path.
        path: PathBuf,
    },
    /// The completed temporary file could not be published at the destination.
    Publish {
        /// The requested level path.
        path: PathBuf,
        /// The publication policy selected by the caller.
        mode: SaveMode,
        /// The filesystem error reported by publication.
        source: io::Error,
    },
}

impl SaveError {
    /// Returns the destination path involved in the failed save.
    pub fn path(&self) -> &Path {
        match self {
            Self::InvalidDestination { path } | Self::DestinationMissing { path } => path,
            Self::TemporaryCreate { destination, .. }
            | Self::TemporaryWrite { destination, .. } => destination,
            Self::Publish { path, .. } => path,
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDestination { path } => {
                write!(
                    formatter,
                    "cannot save level to {path:?}: destination is not a file path"
                )
            }
            Self::TemporaryCreate {
                destination,
                path,
                source,
            } => write!(
                formatter,
                "failed to create temporary level file {path:?} for destination {destination:?}: {source}"
            ),
            Self::TemporaryWrite {
                destination,
                path,
                source,
            } => write!(
                formatter,
                "failed to write temporary level file {path:?} for destination {destination:?}: {source}"
            ),
            Self::DestinationMissing { path } => write!(
                formatter,
                "cannot replace level file {path:?}: destination does not exist"
            ),
            Self::Publish { path, mode, source } => {
                write!(formatter, "failed to {mode} level file {path:?}: {source}")
            }
        }
    }
}

impl Error for SaveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::TemporaryCreate { source, .. }
            | Self::TemporaryWrite { source, .. }
            | Self::Publish { source, .. } => Some(source),
            Self::InvalidDestination { .. } | Self::DestinationMissing { .. } => None,
        }
    }
}

/// Loads and completely validates a `.platformer` level file.
///
/// The file is read in full before the existing [`Level`] parser validates its
/// header, required records, IDs, geometry, and all record fields. A successful
/// return therefore contains a complete valid level rather than a partially
/// loaded value.
pub fn load_level(path: impl AsRef<Path>) -> Result<Level, LoadError> {
    let path = path.as_ref();
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) => {
            return Err(LoadError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    contents.parse().map_err(|source| LoadError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Saves a level using the requested create or replacement policy.
///
/// Serialization is written to a uniquely named temporary file in the
/// destination directory. Only after the complete contents have been written
/// and flushed is that temporary file published. [`SaveMode::CreateNew`]
/// publishes with no-overwrite semantics, so an occupied destination is never
/// replaced even when another process creates it during the save. The
/// [`SaveMode::ReplaceExisting`] policy requires a destination to exist when
/// publication begins and replaces it through the platform's file rename
/// operation. A handled failure before publication leaves an existing destination
/// untouched; the temporary file is removed when cleanup succeeds. Cleanup is
/// best effort because a filesystem failure can prevent removal.
pub fn save_level(path: impl AsRef<Path>, level: &Level, mode: SaveMode) -> Result<(), SaveError> {
    let destination = path.as_ref().to_path_buf();
    let directory = destination_directory(&destination)?;
    let mut temporary = create_temporary_file(&destination, directory)?;

    if let Err(source) = temporary.write_level(level) {
        return Err(SaveError::TemporaryWrite {
            destination,
            path: temporary.path.clone(),
            source,
        });
    }
    temporary.close();

    if mode == SaveMode::ReplaceExisting {
        match fs::symlink_metadata(&destination) {
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Err(SaveError::DestinationMissing { path: destination });
            }
            Err(source) => {
                return Err(SaveError::Publish {
                    path: destination,
                    mode,
                    source,
                });
            }
        }
    }

    let result = match mode {
        SaveMode::CreateNew => fs::hard_link(&temporary.path, &destination),
        SaveMode::ReplaceExisting => fs::rename(&temporary.path, &destination),
    };
    match result {
        Ok(()) => {
            if mode == SaveMode::ReplaceExisting {
                temporary.disarm();
            }
            Ok(())
        }
        Err(source) => Err(SaveError::Publish {
            path: destination,
            mode,
            source,
        }),
    }
}

fn destination_directory(path: &Path) -> Result<&Path, SaveError> {
    if path.file_name().is_none() {
        return Err(SaveError::InvalidDestination {
            path: path.to_path_buf(),
        });
    }

    Ok(path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new(".")))
}

fn create_temporary_file(destination: &Path, directory: &Path) -> Result<TemporaryFile, SaveError> {
    let mut last_path = None;
    for _ in 0..TEMPORARY_FILE_ATTEMPTS {
        let counter = TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            "{TEMPORARY_FILE_PREFIX}{}-{counter}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                return Ok(TemporaryFile {
                    path,
                    file: Some(file),
                    remove_on_drop: true,
                });
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                last_path = Some(path);
            }
            Err(source) => {
                return Err(SaveError::TemporaryCreate {
                    destination: destination.to_path_buf(),
                    path,
                    source,
                });
            }
        }
    }

    let path = last_path.expect("temporary filename attempts must be nonzero");
    Err(SaveError::TemporaryCreate {
        destination: destination.to_path_buf(),
        path,
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not choose a unique temporary filename",
        ),
    })
}

struct TemporaryFile {
    path: PathBuf,
    file: Option<File>,
    remove_on_drop: bool,
}

impl TemporaryFile {
    fn write_level(&mut self, level: &Level) -> io::Result<()> {
        let file = self
            .file
            .as_mut()
            .expect("temporary file must remain open while writing");
        let mut writer = BufWriter::new(file);
        write!(writer, "{level}")?;
        writer.flush()
    }

    fn close(&mut self) {
        drop(self.file.take());
    }

    fn disarm(&mut self) {
        self.remove_on_drop = false;
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        drop(self.file.take());
        if self.remove_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SaveError, SaveMode, TEMPORARY_FILE_PREFIX, load_level, save_level};
    use crate::level::{Level, SpawnPoint};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

    const VALID_DOCUMENT: &str =
        "platformer-level 1\nnext-id 2\nspawn 64 64\nplatform 1 0 0 400 32\n";

    fn test_directory() -> PathBuf {
        loop {
            let counter = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "titan-platformer-persistence-{}-{counter}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create unique test directory {path:?}: {error}"),
            }
        }
    }

    fn sample_level() -> Level {
        VALID_DOCUMENT.parse().expect("valid test level")
    }

    fn temporary_files(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .expect("read test directory")
            .map(|entry| entry.expect("read directory entry").path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(TEMPORARY_FILE_PREFIX))
            })
            .collect()
    }

    fn remove_test_directory(path: &Path) {
        fs::remove_dir_all(path).expect("remove test directory");
    }

    #[test]
    fn saves_and_reopens_new_then_replaced_levels() {
        let directory = test_directory();
        let path = directory.join("level.platformer");
        let level = sample_level();

        save_level(&path, &level, SaveMode::CreateNew).expect("create level");
        assert_eq!(load_level(&path).expect("load created level"), level);

        let mut replacement = level.clone();
        replacement
            .set_spawn(SpawnPoint { x: 12.0, y: 34.0 })
            .expect("valid spawn");
        save_level(&path, &replacement, SaveMode::ReplaceExisting).expect("replace level");
        assert_eq!(load_level(&path).expect("load replaced level"), replacement);
        assert!(temporary_files(&directory).is_empty());

        remove_test_directory(&directory);
    }

    #[test]
    fn create_new_does_not_overwrite_and_cleans_temporary_file() {
        let directory = test_directory();
        let path = directory.join("level.platformer");
        fs::write(&path, b"keep this level").expect("write occupied destination");

        let error = save_level(&path, &sample_level(), SaveMode::CreateNew)
            .expect_err("create-new must reject an occupied path");
        assert!(matches!(
            error,
            SaveError::Publish {
                mode: SaveMode::CreateNew,
                ..
            }
        ));
        assert_eq!(
            fs::read(&path).expect("read occupied destination"),
            b"keep this level"
        );
        assert!(temporary_files(&directory).is_empty());

        remove_test_directory(&directory);
    }

    #[test]
    fn replacement_failure_preserves_destination_and_cleans_temporary_file() {
        let directory = test_directory();
        let path = directory.join("occupied");
        fs::create_dir(&path).expect("create directory destination");

        let error = save_level(&path, &sample_level(), SaveMode::ReplaceExisting)
            .expect_err("replacing a directory must fail");
        assert!(matches!(
            error,
            SaveError::Publish {
                mode: SaveMode::ReplaceExisting,
                ..
            }
        ));
        assert!(path.is_dir());
        assert!(temporary_files(&directory).is_empty());

        remove_test_directory(&directory);
    }

    #[test]
    fn replacement_requires_an_existing_destination() {
        let directory = test_directory();
        let path = directory.join("missing.platformer");

        let error = save_level(&path, &sample_level(), SaveMode::ReplaceExisting)
            .expect_err("replace-existing must reject a missing path");
        assert!(matches!(error, SaveError::DestinationMissing { .. }));
        assert!(!path.exists());
        assert!(temporary_files(&directory).is_empty());

        remove_test_directory(&directory);
    }
}
