use anyhow::{bail, Context, Result};
#[cfg(unix)]
use std::os::{
    fd::{AsRawFd, FromRawFd},
    unix::ffi::OsStrExt,
};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, PathBuf},
};

pub fn id() -> Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn valid(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(unix)]
fn root() -> Result<File> {
    let path = if let Some(p) = std::env::var_os("IPTV_SLAYER_DATA_DIR") {
        PathBuf::from(p)
    } else if let Some(p) =
        std::env::var_os("XDG_DATA_HOME").filter(|p| PathBuf::from(p).is_absolute())
    {
        PathBuf::from(p).join("iptv-slayer")
    } else {
        PathBuf::from(std::env::var_os("HOME").context("HOME not set")?)
            .join(".local/share/iptv-slayer")
    };
    if !path.is_absolute() {
        bail!("data directory must be absolute");
    }
    let mut dir = File::open("/")?;
    let components: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            Component::RootDir => None,
            Component::Normal(n) => Some(Ok(n)),
            _ => Some(Err(anyhow::anyhow!("invalid data directory"))),
        })
        .collect::<Result<_>>()?;
    if components.is_empty() {
        bail!("invalid data directory");
    }
    for (i, component) in components.iter().enumerate() {
        let name = std::ffi::CString::new(component.as_bytes())?;
        unsafe {
            libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700);
        }
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        dir = unsafe { File::from_raw_fd(fd) };
        if i + 1 == components.len() {
            let metadata = dir.metadata()?;
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != unsafe { libc::geteuid() } {
                bail!("data directory ownership");
            }
            if unsafe { libc::fchmod(dir.as_raw_fd(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    Ok(dir)
}
#[cfg(unix)]
fn client(id: &str, create: bool) -> Result<File> {
    if !valid(id) {
        bail!("invalid client");
    }
    let root = root()?;
    let name = std::ffi::CString::new(id)?;
    if create {
        unsafe {
            libc::mkdirat(root.as_raw_fd(), name.as_ptr(), 0o700);
        }
    }
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let dir = unsafe { File::from_raw_fd(fd) };
    use std::os::unix::fs::MetadataExt;
    if dir.metadata()?.uid() != unsafe { libc::geteuid() } {
        bail!("client ownership");
    }
    if unsafe { libc::fchmod(dir.as_raw_fd(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(dir)
}
const MAX_STATE: usize = 128 * 1024 * 1024;

#[cfg(unix)]
pub fn read(id: &str) -> Result<String> {
    let dir = client(id, false)?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c"channels.json".as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.is_file() {
        bail!("invalid state file");
    }
    let mut out = String::new();
    file.take(MAX_STATE as u64 + 1).read_to_string(&mut out)?;
    if out.len() > MAX_STATE {
        bail!("state too large");
    }
    Ok(out)
}
#[cfg(unix)]
pub fn write(id: &str, data: &str, committed: impl FnOnce()) -> Result<()> {
    if data.len() > MAX_STATE {
        bail!("state too large");
    }
    let dir = client(id, true)?;
    write_in_dir(&dir, data, committed, |dir| dir.sync_all())
}

#[cfg(unix)]
fn write_in_dir(
    dir: &File,
    data: &str,
    committed: impl FnOnce(),
    sync: impl FnOnce(&File) -> std::io::Result<()>,
) -> Result<()> {
    let temp = std::ffi::CString::new(format!(".{}", self::id()?))?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            temp.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| -> Result<()> {
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
        if unsafe {
            libc::renameat(
                dir.as_raw_fd(),
                temp.as_ptr(),
                dir.as_raw_fd(),
                c"channels.json".as_ptr(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        committed();
        sync(dir)?;
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0);
        }
    }
    result
}
#[cfg(not(unix))]
pub fn read(_: &str) -> Result<String> {
    bail!("secure storage unavailable")
}
#[cfg(not(unix))]
pub fn write(_: &str, _: &str, _: impl FnOnce()) -> Result<()> {
    bail!("secure storage unavailable")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn rename_is_reported_before_directory_sync_failure() {
        let path = tempfile::tempdir().unwrap();
        let dir = File::open(path.path()).unwrap();
        let mut committed = false;
        let result = write_in_dir(
            &dir,
            "new",
            || committed = true,
            |_| Err(std::io::Error::other("sync failure")),
        );
        assert!(result.is_err());
        assert!(committed);
        assert_eq!(
            std::fs::read_to_string(path.path().join("channels.json")).unwrap(),
            "new"
        );
    }
}
