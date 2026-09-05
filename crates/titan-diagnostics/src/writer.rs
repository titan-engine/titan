use crate::{BundleCapture, DiagnosticBundle};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use titan::render::Image;
use titan_protocol::{ResponseEnvelope, ResponseOutcome};
static NEXT: AtomicU64 = AtomicU64::new(0);
const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
#[derive(Debug)]
pub enum BundleWriteError {
    Io(io::Error),
    Json(serde_json::Error),
    Png(png::EncodingError),
    TooLarge,
}
impl std::fmt::Display for BundleWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "{e}"),
            Self::Png(e) => write!(f, "{e}"),
            Self::TooLarge => write!(f, "diagnostic artifact exceeds 64 MiB limit"),
        }
    }
}
impl std::error::Error for BundleWriteError {}
impl From<io::Error> for BundleWriteError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for BundleWriteError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<png::EncodingError> for BundleWriteError {
    fn from(e: png::EncodingError) -> Self {
        Self::Png(e)
    }
}
#[derive(Clone, Debug)]
pub struct WrittenBundle {
    pub directory: PathBuf,
    pub manifest: PathBuf,
    pub capture: Option<PathBuf>,
}
/// Creates a unique self-contained artifact directory. Paths never use request IDs.
/// Unix bundles/files are owner-only; failed writes remove only their new directory.
pub fn write_bundle(
    root: &Path,
    bundle: &DiagnosticBundle,
    capture: Option<&Image>,
) -> Result<WrittenBundle, BundleWriteError> {
    if capture.is_some_and(|image| image.pixels().len() > MAX_ARTIFACT_BYTES) {
        return Err(BundleWriteError::TooLarge);
    }
    fs::create_dir_all(root)?;
    let root = root.canonicalize()?;
    let directory = loop {
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = root.join(format!("bundle-{}-{now}-{nonce}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => break path,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| {
        let mut bundle = bundle.clone();
        let capture_path = if let Some(image) = capture {
            let path = directory.join("capture.png");
            let file = private_file(&path)?;
            crate::write_png(image, file)?;
            let checksum = image
                .pixels()
                .iter()
                .fold(0xcbf29ce484222325u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
                });
            bundle.capture = Some(BundleCapture {
                artifact: "capture.png".into(),
                format: "png".into(),
                width: image.width(),
                height: image.height(),
                checksum: format!("{checksum:016x}"),
            });
            Some(path)
        } else {
            bundle.capture = None;
            None
        };
        let bytes = serde_json::to_vec_pretty(&bundle)?;
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(BundleWriteError::TooLarge);
        }
        let manifest = directory.join("bundle.json");
        let temporary = directory.join("bundle.json.part");
        private_file(&temporary)?.write_all(&bytes)?;
        if let Some(api) = &bundle.api_summary {
            private_file(&directory.join("api.txt"))?.write_all(api.compact_text().as_bytes())?;
        }
        fs::rename(temporary, &manifest)?;
        Ok(WrittenBundle {
            directory: directory.clone(),
            manifest,
            capture: capture_path,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}
fn private_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
/// Add the artifact location only to a failure. Successful response schema stays intact.
pub fn attach_failure_path(response: &mut ResponseEnvelope, path: &Path) -> bool {
    if let ResponseOutcome::Failure { error } = &mut response.outcome {
        error.details.insert(
            "diagnostic_bundle".into(),
            serde_json::Value::String(path.to_string_lossy().into_owned()),
        );
        true
    } else {
        false
    }
}
