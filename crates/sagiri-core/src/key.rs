//! The secret key and its key file.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::paths::{PathError, data_dir_with};

const KEY_LEN: usize = 32;
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Errors creating, generating or loading a key.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// The key file does not have the expected format.
    #[error(
        "key file {} is malformed: expected 64 lowercase hex characters and a newline",
        .path.display()
    )]
    Malformed { path: PathBuf },
    /// The key file is readable or writable by group or others.
    #[error(
        "key file {} has insecure permissions; run: chmod 600 {}",
        .path.display(), .path.display()
    )]
    InsecurePermissions { path: PathBuf },
    /// An I/O operation on the key file failed.
    #[error("cannot access key file {}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// The operating system could not provide random bytes.
    #[error("cannot read random bytes from the operating system: {0}")]
    Random(getrandom::Error),
}

/// 32 secret bytes. Zeroized on drop.
pub struct Key(Zeroizing<[u8; KEY_LEN]>);

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

impl Key {
    /// A fresh key from the operating system's random source.
    pub fn generate() -> Result<Key, KeyError> {
        let mut b = [0u8; KEY_LEN];
        getrandom::fill(&mut b).map_err(KeyError::Random)?;
        let key = Key::from_bytes(b);
        zeroize::Zeroize::zeroize(&mut b);
        Ok(key)
    }

    /// Wraps existing key bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Key {
        Key(Zeroizing::new(bytes))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Loads the key at `path`, creating it (and parent directories) if missing.
    pub fn load_or_create(path: &Path) -> Result<Key, KeyError> {
        let io_err = |source| KeyError::Io {
            path: path.to_path_buf(),
            source,
        };
        match fs::symlink_metadata(path) {
            Ok(_) => return load(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(e)),
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(io_err)?;
        }
        let key = Key::generate()?;
        // Write a private temp file first and publish it with an atomic hard link, so a
        // concurrent reader never sees a partial key and a failed write leaves nothing behind.
        let mut suffix = [0u8; 8];
        getrandom::fill(&mut suffix).map_err(KeyError::Random)?;
        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(format!(".tmp{:016x}", u64::from_be_bytes(suffix)));
        let tmp = path.with_file_name(tmp_name);
        let written = (|| {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            let mut line = Zeroizing::new([0u8; KEY_LEN * 2 + 1]);
            for (i, b) in key.as_bytes().iter().enumerate() {
                line[2 * i] = HEX[(b >> 4) as usize];
                line[2 * i + 1] = HEX[(b & 15) as usize];
            }
            line[KEY_LEN * 2] = b'\n';
            f.write_all(&line[..])?;
            f.sync_all()?;
            fs::hard_link(&tmp, path)
        })();
        let _ = fs::remove_file(&tmp);
        match written {
            Ok(()) => Ok(key),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => load(path),
            Err(e) => Err(io_err(e)),
        }
    }
}

fn load(path: &Path) -> Result<Key, KeyError> {
    let io_err = |source| KeyError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::open(path).map_err(io_err)?;
    let meta = file.metadata().map_err(io_err)?;
    if meta.mode() & 0o077 != 0 {
        return Err(KeyError::InsecurePermissions {
            path: path.to_path_buf(),
        });
    }
    let malformed = || KeyError::Malformed {
        path: path.to_path_buf(),
    };
    // Read at most one byte more than a valid file, so a huge file is cheap to reject.
    let mut buf = Zeroizing::new(Vec::with_capacity(KEY_LEN * 2 + 2));
    (&mut file)
        .take((KEY_LEN * 2 + 2) as u64)
        .read_to_end(&mut buf)
        .map_err(io_err)?;
    if buf.len() != KEY_LEN * 2 + 1 || buf[KEY_LEN * 2] != b'\n' {
        return Err(malformed());
    }
    let mut bytes = Zeroizing::new([0u8; KEY_LEN]);
    for (i, pair) in buf[..KEY_LEN * 2].chunks_exact(2).enumerate() {
        let hi = nibble(pair[0]).ok_or_else(malformed)?;
        let lo = nibble(pair[1]).ok_or_else(malformed)?;
        bytes[i] = hi << 4 | lo;
    }
    Ok(Key::from_bytes(*bytes))
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Key file location: the flag, else `SAGIRI_KEY_FILE`, else `<data dir>/key`.
pub fn resolve_path(
    flag: Option<&Path>,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<PathBuf, PathError> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }
    if let Some(v) = env("SAGIRI_KEY_FILE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(v));
    }
    Ok(data_dir_with(env)?.join("key"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn write_private(path: &Path, content: &[u8]) {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        f.write_all(content).unwrap();
    }

    #[test]
    fn creates_file_and_dirs_with_private_modes_and_reloads() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a/b/key");
        let k1 = Key::load_or_create(&path).unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(path.parent().unwrap().parent().unwrap()), 0o700);
        let k2 = Key::load_or_create(&path).unwrap();
        assert_eq!(k1.as_bytes(), k2.as_bytes());
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.len(), 65);
        // No temp file is left behind.
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn concurrent_creation_yields_one_key_and_one_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("d/key");
        let keys: Vec<[u8; 32]> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8)
                .map(|_| s.spawn(|| *Key::load_or_create(&path).unwrap().as_bytes()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(keys.iter().all(|k| *k == keys[0]));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn rejects_group_or_other_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("key");
        Key::load_or_create(&path).unwrap();
        for m in [0o644, 0o640, 0o604, 0o660] {
            fs::set_permissions(&path, fs::Permissions::from_mode(m)).unwrap();
            assert!(matches!(
                Key::load_or_create(&path),
                Err(KeyError::InsecurePermissions { .. })
            ));
        }
        let msg = Key::load_or_create(&path).unwrap_err().to_string();
        assert!(msg.contains("chmod 600"));
        assert!(msg.contains(path.to_str().unwrap()));
    }

    #[test]
    fn rejects_malformed_files_without_leaking_content() {
        let good = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let cases = [
            good[..63].to_string() + "\n",
            good.to_string() + "00\n",
            good.to_uppercase() + "\n",
            good.replace("00", "zz") + "\n",
            good.to_string(),
            good.to_string() + "\r\n",
            String::new(),
        ];
        let tmp = tempfile::tempdir().unwrap();
        for (i, content) in cases.iter().enumerate() {
            let path = tmp.path().join(format!("key{i}"));
            write_private(&path, content.as_bytes());
            let err = Key::load_or_create(&path).unwrap_err();
            assert!(matches!(err, KeyError::Malformed { .. }), "case {i}");
            let msg = err.to_string();
            assert!(!msg.contains(&good[..16]), "case {i}");
            assert!(msg.contains(path.to_str().unwrap()));
        }
    }

    #[test]
    fn valid_file_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("key");
        write_private(
            &path,
            b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\n",
        );
        let k = Key::load_or_create(&path).unwrap();
        let expected: Vec<u8> = (0..32).collect();
        assert_eq!(&k.as_bytes()[..], &expected[..]);
    }

    #[test]
    fn debug_is_redacted() {
        let k = Key::from_bytes([0xab; 32]);
        let s = format!("{k:?}");
        assert_eq!(s, "Key(<redacted>)");
        assert!(!s.contains("ab"));
    }

    #[test]
    fn generate_gives_different_keys() {
        let a = Key::generate().unwrap();
        let b = Key::generate().unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn resolve_path_order() {
        let env = |k: &str| match k {
            "SAGIRI_KEY_FILE" => Some(OsString::from("/env/key")),
            "XDG_DATA_HOME" => Some(OsString::from("/xdg")),
            "HOME" => Some(OsString::from("/home/u")),
            _ => None,
        };
        let flag = Path::new("/flag/key");
        assert_eq!(resolve_path(Some(flag), &env).unwrap(), flag);
        assert_eq!(resolve_path(None, &env).unwrap(), PathBuf::from("/env/key"));
        let env2 = |k: &str| match k {
            "SAGIRI_KEY_FILE" => Some(OsString::new()),
            "XDG_DATA_HOME" => Some(OsString::from("/xdg")),
            _ => None,
        };
        assert_eq!(
            resolve_path(None, &env2).unwrap(),
            PathBuf::from("/xdg/sagiri/key")
        );
        let env3 = |k: &str| (k == "HOME").then(|| OsString::from("/home/u"));
        assert_eq!(
            resolve_path(None, &env3).unwrap(),
            PathBuf::from("/home/u/.local/share/sagiri/key")
        );
        let none = |_: &str| None;
        assert!(matches!(resolve_path(None, &none), Err(PathError::NoHome)));
    }
}
