#![cfg(windows)]
#![allow(unsafe_code)]

use std::fs;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use anyhow::{bail, Result};
use windows_sys::Win32::Storage::FileSystem::{
    FileAttributeTagInfo, GetFileInformationByHandleEx, GetFileType, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_TYPE_DISK, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT,
};

// Audited Windows input-handle boundary: callers pass already-opened read-only
// handles, and this module only inspects handle type and reparse attributes.
pub(crate) fn open_flags() -> u32 {
    FILE_FLAG_OPEN_REPARSE_POINT | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION
}

pub(crate) fn validate_disk_handle(file: &fs::File, path: &Path) -> Result<()> {
    validate_file_type(file_type(file), path)
}

pub(crate) fn validate_non_reparse_disk_handle(file: &fs::File, path: &Path) -> Result<()> {
    let tag_info = file_attribute_tag_info(file, path)?;
    validate_file_characteristics(file_type(file), tag_info.FileAttributes, path)
}

fn file_type(file: &fs::File) -> u32 {
    // SAFETY: GetFileType only inspects the borrowed HANDLE value. The handle
    // comes from std::fs::File, remains owned by the caller, and is not closed
    // or retained by this call.
    unsafe { GetFileType(file.as_raw_handle()) }
}

fn file_attribute_tag_info(file: &fs::File, path: &Path) -> Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: The buffer points to a properly aligned FILE_ATTRIBUTE_TAG_INFO
    // value for the duration of the call, and the byte count matches the
    // buffer type. The borrowed HANDLE is not closed or retained.
    let inspected = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            std::ptr::from_mut(&mut tag_info).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if inspected == 0 {
        bail!(
            "inspect Windows input reparse attributes before read: {}",
            path.display()
        );
    }
    Ok(tag_info)
}

fn validate_file_type(file_type: u32, path: &Path) -> Result<()> {
    if file_type != FILE_TYPE_DISK {
        bail!(
            "Windows input handle must have FILE_TYPE_DISK before read: {}",
            path.display()
        );
    }
    Ok(())
}

fn validate_file_characteristics(file_type: u32, file_attributes: u32, path: &Path) -> Result<()> {
    if file_type != FILE_TYPE_DISK || file_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        bail!(
            "Windows input must be a non-reparse FILE_TYPE_DISK before read: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_TYPE_CHAR, FILE_TYPE_PIPE,
    };

    #[test]
    fn input_open_flags_reject_reparse_traversal_and_limit_named_pipe_impersonation() {
        let flags = open_flags();

        assert_eq!(
            flags & FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_FLAG_OPEN_REPARSE_POINT
        );
        assert_eq!(flags & SECURITY_SQOS_PRESENT, SECURITY_SQOS_PRESENT);
        assert_eq!(flags & SECURITY_IDENTIFICATION, SECURITY_IDENTIFICATION);
    }

    #[test]
    fn draft_input_handle_type_accepts_only_disk_files() {
        let path = Path::new("input.json");

        assert!(validate_file_type(FILE_TYPE_DISK, path).is_ok());
        assert!(validate_file_type(FILE_TYPE_PIPE, path).is_err());
        assert!(validate_file_type(FILE_TYPE_CHAR, path).is_err());
    }

    #[test]
    fn support_bundle_input_characteristics_reject_non_disk_and_reparse_handles() {
        let path = Path::new("support-bundle.json");

        assert!(validate_file_characteristics(FILE_TYPE_DISK, FILE_ATTRIBUTE_NORMAL, path).is_ok());
        assert!(
            validate_file_characteristics(FILE_TYPE_PIPE, FILE_ATTRIBUTE_NORMAL, path).is_err()
        );
        assert!(
            validate_file_characteristics(FILE_TYPE_DISK, FILE_ATTRIBUTE_REPARSE_POINT, path)
                .is_err()
        );
    }
}
