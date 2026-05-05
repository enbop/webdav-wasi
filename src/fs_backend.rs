use std::{
    fs,
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use async_trait::async_trait;

use crate::{BackendError, DirEntry, Metadata, Result, WebDavBackend, normalize_path};

#[derive(Debug, Clone)]
pub struct FileSystemBackend {
    root: PathBuf,
}

impl FileSystemBackend {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|error| map_io_error(error, root.display().to_string()))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve_host_path(&self, normalized: &str) -> Result<PathBuf> {
        let mut host_path = self.root.clone();

        if normalized.is_empty() {
            return Ok(host_path);
        }

        for segment in normalized.split('/') {
            host_path.push(segment);
            if let Ok(metadata) = fs::symlink_metadata(&host_path)
                && metadata.file_type().is_symlink()
            {
                return Err(BackendError::InvalidPath {
                    path: normalized.to_string(),
                });
            }
        }

        Ok(host_path)
    }

    fn ensure_parent_dir(&self, normalized: &str) -> Result<PathBuf> {
        let parent = parent_dir(normalized).ok_or_else(|| BackendError::PermissionDenied {
            path: normalized.to_string(),
        })?;
        let parent_path = self.resolve_host_path(parent)?;
        let metadata =
            fs::metadata(&parent_path).map_err(|error| map_io_error(error, parent.to_string()))?;
        if !metadata.is_dir() {
            return Err(BackendError::NotDirectory {
                path: parent.to_string(),
            });
        }
        Ok(parent_path)
    }
}

#[async_trait]
impl WebDavBackend for FileSystemBackend {
    async fn metadata(&self, path: &str) -> Result<Metadata> {
        let normalized = normalize_path(path)?;
        let host_path = self.resolve_host_path(&normalized)?;
        let metadata = fs::symlink_metadata(&host_path)
            .map_err(|error| map_io_error(error, normalized.clone()))?;
        Ok(metadata_from_fs(&metadata))
    }

    async fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>> {
        let normalized = normalize_path(path)?;
        let host_path = self.resolve_host_path(&normalized)?;
        let metadata =
            fs::metadata(&host_path).map_err(|error| map_io_error(error, normalized.clone()))?;
        if !metadata.is_dir() {
            return Err(BackendError::NotDirectory { path: normalized });
        }

        let mut entries = Vec::new();
        let read_dir =
            fs::read_dir(&host_path).map_err(|error| map_io_error(error, path.to_string()))?;
        for entry in read_dir {
            let entry = entry.map_err(|error| map_io_error(error, path.to_string()))?;
            let metadata = entry
                .metadata()
                .map_err(|error| map_io_error(error, entry.path().display().to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();
            entries.push(DirEntry {
                name,
                metadata: metadata_from_fs(&metadata),
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    async fn read_chunk(&self, path: &str, start: u64, length: u64) -> Result<Vec<u8>> {
        let normalized = normalize_path(path)?;
        let host_path = self.resolve_host_path(&normalized)?;
        let metadata =
            fs::metadata(&host_path).map_err(|error| map_io_error(error, normalized.clone()))?;
        if metadata.is_dir() {
            return Err(BackendError::IsDirectory { path: normalized });
        }

        let mut file =
            fs::File::open(&host_path).map_err(|error| map_io_error(error, path.to_string()))?;
        file.seek(SeekFrom::Start(start))
            .map_err(|error| map_io_error(error, path.to_string()))?;
        let mut limited = file.take(length);
        let mut buffer = Vec::new();
        limited
            .read_to_end(&mut buffer)
            .map_err(|error| map_io_error(error, path.to_string()))?;
        Ok(buffer)
    }

    async fn write_chunk(&self, path: &str, start: u64, bytes: Vec<u8>) -> Result<u64> {
        let normalized = normalize_path(path)?;
        if normalized.is_empty() {
            return Err(BackendError::PermissionDenied {
                path: path.to_string(),
            });
        }

        self.ensure_parent_dir(&normalized)?;
        let host_path = self.resolve_host_path(&normalized)?;
        if let Ok(metadata) = fs::metadata(&host_path)
            && metadata.is_dir()
        {
            return Err(BackendError::IsDirectory { path: normalized });
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&host_path)
            .map_err(|error| map_io_error(error, path.to_string()))?;
        file.seek(SeekFrom::Start(start))
            .map_err(|error| map_io_error(error, path.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| map_io_error(error, path.to_string()))?;
        Ok(start.saturating_add(bytes.len() as u64))
    }

    async fn truncate(&self, path: &str, size: u64) -> Result<()> {
        let normalized = normalize_path(path)?;
        let host_path = self.resolve_host_path(&normalized)?;
        let metadata =
            fs::metadata(&host_path).map_err(|error| map_io_error(error, normalized.clone()))?;
        if metadata.is_dir() {
            return Err(BackendError::IsDirectory { path: normalized });
        }

        let file = OpenOptions::new()
            .write(true)
            .open(&host_path)
            .map_err(|error| map_io_error(error, path.to_string()))?;
        file.set_len(size)
            .map_err(|error| map_io_error(error, path.to_string()))
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let normalized = normalize_path(path)?;
        if normalized.is_empty() {
            return Ok(());
        }
        self.ensure_parent_dir(&normalized)?;
        let host_path = self.resolve_host_path(&normalized)?;
        fs::create_dir(&host_path).map_err(|error| map_io_error(error, path.to_string()))
    }

    async fn remove_dir(&self, path: &str) -> Result<()> {
        let normalized = normalize_path(path)?;
        if normalized.is_empty() {
            return Err(BackendError::PermissionDenied {
                path: path.to_string(),
            });
        }
        let host_path = self.resolve_host_path(&normalized)?;
        fs::remove_dir(&host_path).map_err(|error| map_io_error(error, path.to_string()))
    }

    async fn remove_file(&self, path: &str) -> Result<()> {
        let normalized = normalize_path(path)?;
        let host_path = self.resolve_host_path(&normalized)?;
        fs::remove_file(&host_path).map_err(|error| map_io_error(error, path.to_string()))
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let from = normalize_path(from)?;
        let to = normalize_path(to)?;
        if from.is_empty() || to.is_empty() {
            return Err(BackendError::PermissionDenied {
                path: if from.is_empty() { from } else { to },
            });
        }

        self.ensure_parent_dir(&to)?;
        let source = self.resolve_host_path(&from)?;
        let destination = self.resolve_host_path(&to)?;
        let source_metadata =
            fs::metadata(&source).map_err(|error| map_io_error(error, from.clone()))?;

        if let Ok(destination_metadata) = fs::metadata(&destination) {
            if source_metadata.is_dir() || destination_metadata.is_dir() {
                return Err(BackendError::AlreadyExists { path: to });
            }
            fs::remove_file(&destination)
                .map_err(|error| map_io_error(error, destination.display().to_string()))?;
        }

        fs::rename(source, destination).map_err(|error| map_io_error(error, from))
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let from = normalize_path(from)?;
        let to = normalize_path(to)?;
        if from.is_empty() || to.is_empty() {
            return Err(BackendError::PermissionDenied {
                path: if from.is_empty() { from } else { to },
            });
        }

        self.ensure_parent_dir(&to)?;
        let source = self.resolve_host_path(&from)?;
        let destination = self.resolve_host_path(&to)?;
        copy_path(&source, &destination)
    }
}

fn metadata_from_fs(metadata: &fs::Metadata) -> Metadata {
    Metadata {
        len: metadata.len(),
        is_dir: metadata.is_dir(),
        is_file: metadata.is_file(),
        is_symlink: metadata.file_type().is_symlink(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        accessed: metadata.accessed().ok(),
        permissions: permissions_mode(metadata),
    }
}

fn permissions_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode()
    }

    #[cfg(not(unix))]
    {
        if metadata.permissions().readonly() {
            0o444
        } else if metadata.is_dir() {
            0o755
        } else {
            0o644
        }
    }
}

fn parent_dir(path: &str) -> Option<&str> {
    if path.is_empty() {
        return None;
    }
    path.rsplit_once('/').map(|(parent, _)| parent).or(Some(""))
}

fn map_io_error(error: std::io::Error, path: String) -> BackendError {
    use std::io::ErrorKind;

    match error.kind() {
        ErrorKind::NotFound => BackendError::NotFound { path },
        ErrorKind::PermissionDenied => BackendError::PermissionDenied { path },
        ErrorKind::AlreadyExists => BackendError::AlreadyExists { path },
        ErrorKind::DirectoryNotEmpty => BackendError::DirectoryNotEmpty { path },
        ErrorKind::IsADirectory => BackendError::IsDirectory { path },
        ErrorKind::NotADirectory => BackendError::NotDirectory { path },
        ErrorKind::StorageFull => BackendError::NoSpace,
        ErrorKind::FileTooLarge => BackendError::FileTooLarge,
        ErrorKind::ReadOnlyFilesystem => BackendError::ReadOnly,
        ErrorKind::Unsupported => BackendError::NotSupported { operation: path },
        _ => BackendError::Other {
            message: error.to_string(),
        },
    }
}

fn copy_path(source: &Path, destination: &Path) -> Result<()> {
    let metadata =
        fs::metadata(source).map_err(|error| map_io_error(error, source.display().to_string()))?;

    if metadata.is_dir() {
        if destination.exists() {
            return Err(BackendError::AlreadyExists {
                path: destination.display().to_string(),
            });
        }
        fs::create_dir(destination)
            .map_err(|error| map_io_error(error, destination.display().to_string()))?;
        for entry in fs::read_dir(source)
            .map_err(|error| map_io_error(error, source.display().to_string()))?
        {
            let entry = entry.map_err(|error| map_io_error(error, source.display().to_string()))?;
            copy_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }

    if let Ok(destination_metadata) = fs::metadata(destination) {
        if destination_metadata.is_dir() {
            return Err(BackendError::AlreadyExists {
                path: destination.display().to_string(),
            });
        }
    }

    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| map_io_error(error, destination.display().to_string()))
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::FileSystemBackend;
    use crate::WebDavBackend;

    fn temp_root() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        env::temp_dir().join(format!("webdav-wasi-test-{unique}-{}", std::process::id()))
    }

    #[tokio::test]
    async fn file_backend_reads_and_truncates() {
        let root = temp_root();
        let backend = FileSystemBackend::new(&root).expect("create backend");

        backend
            .write_chunk("hello.txt", 0, b"hello world".to_vec())
            .await
            .expect("write file");
        backend
            .truncate("hello.txt", 5)
            .await
            .expect("truncate file");

        let chunk = backend
            .read_chunk("hello.txt", 0, 32)
            .await
            .expect("read file");
        assert_eq!(chunk, b"hello");

        std::fs::remove_dir_all(root).expect("cleanup temp dir");
    }
}
