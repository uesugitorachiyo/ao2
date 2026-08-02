#![cfg(unix)]
#![allow(unsafe_code)]

use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::Result;

// Audited Unix direct-child boundary: the caller retains an opened directory,
// and the child name has already been reduced to one normal UTF-8 component.
pub(crate) fn open_direct_child(directory: &File, name: &str) -> Result<File> {
    let name = CString::new(Path::new(name).as_os_str().as_bytes())?;
    // SAFETY: name contains no NUL and is one validated direct-child component.
    // The borrowed directory fd remains alive. On success, the returned fd is
    // transferred exactly once into OwnedFd.
    let owned = unsafe {
        let fd = libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        OwnedFd::from_raw_fd(fd)
    };
    Ok(File::from(owned))
}
