#![cfg(windows)]
#![allow(unsafe_code)]

use std::fs;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use anyhow::{bail, Result};
use windows_sys::Win32::Storage::FileSystem::{
    FileAttributeTagInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_TYPE_DISK,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};

// Audited Windows input-handle boundary: callers pass already-opened read-only
// handles, and this module only inspects handle type, identity, link count, and
// reparse attributes.
pub(crate) fn open_flags() -> u32 {
    FILE_FLAG_OPEN_REPARSE_POINT | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION
}

pub(crate) fn root_open_flags() -> u32 {
    open_flags() | FILE_FLAG_BACKUP_SEMANTICS
}

pub(crate) fn root_share_mode() -> u32 {
    FILE_SHARE_READ
}

pub(crate) fn validate_disk_handle(file: &fs::File, path: &Path) -> Result<()> {
    validate_file_type(file_type(file), path)
}

pub(crate) fn validate_non_reparse_disk_handle(file: &fs::File, path: &Path) -> Result<()> {
    let tag_info = file_attribute_tag_info(file, path)?;
    validate_file_characteristics(file_type(file), tag_info.FileAttributes, path)
}

pub(crate) fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DiskFileIdentity {
    volume_serial_number: u32,
    file_index: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DiskFileState {
    pub(crate) identity: DiskFileIdentity,
    pub(crate) number_of_links: u32,
}

pub(crate) fn disk_file_identity(file: &fs::File, path: &Path) -> Result<DiskFileIdentity> {
    Ok(disk_file_state(file, path)?.identity)
}

pub(crate) fn disk_file_state(file: &fs::File, path: &Path) -> Result<DiskFileState> {
    let inspection = inspect_disk_handle(file, path)?;
    Ok(DiskFileState {
        identity: identity_from_file_information(&inspection.file_information),
        number_of_links: inspection.file_information.nNumberOfLinks,
    })
}

fn file_type(file: &fs::File) -> u32 {
    // SAFETY: GetFileType only inspects the borrowed HANDLE value. The handle
    // comes from std::fs::File, remains owned by the caller, and is not closed
    // or retained by this call.
    unsafe { GetFileType(file.as_raw_handle()) }
}

fn file_attribute_tag_info(file: &fs::File, path: &Path) -> Result<FILE_ATTRIBUTE_TAG_INFO> {
    Ok(inspect_disk_handle(file, path)?.tag_info)
}

struct DiskHandleInspection {
    tag_info: FILE_ATTRIBUTE_TAG_INFO,
    file_information: BY_HANDLE_FILE_INFORMATION,
}

fn inspect_disk_handle(file: &fs::File, path: &Path) -> Result<DiskHandleInspection> {
    let mut tag_info = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut file_information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: Both buffers are properly aligned for the respective Windows
    // inspection APIs. The borrowed HANDLE remains owned by std::fs::File and
    // is neither closed nor retained by either call.
    let (tag_inspected, identity_inspected) = unsafe {
        (
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                std::ptr::from_mut(&mut tag_info).cast(),
                std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            ),
            GetFileInformationByHandle(
                file.as_raw_handle(),
                std::ptr::from_mut(&mut file_information),
            ),
        )
    };
    if tag_inspected == 0 || identity_inspected == 0 {
        bail!(
            "inspect Windows input identity and reparse attributes before read: {}",
            path.display()
        );
    }
    Ok(DiskHandleInspection {
        tag_info,
        file_information,
    })
}

fn identity_from_file_information(info: &BY_HANDLE_FILE_INFORMATION) -> DiskFileIdentity {
    DiskFileIdentity {
        volume_serial_number: info.dwVolumeSerialNumber,
        file_index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    }
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
    fn root_open_flags_open_directories_without_reparse_following_or_delete_sharing() {
        let flags = root_open_flags();

        assert_eq!(
            flags & FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_FLAG_OPEN_REPARSE_POINT
        );
        assert_eq!(
            flags & FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_BACKUP_SEMANTICS
        );
        assert_eq!(root_share_mode(), FILE_SHARE_READ);
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

    #[test]
    fn disk_file_identity_compares_volume_and_full_file_index() {
        let first = DiskFileIdentity {
            volume_serial_number: 7,
            file_index: u64::from(u32::MAX) + 1,
        };
        let same = first;
        let different_volume = DiskFileIdentity {
            volume_serial_number: 8,
            file_index: first.file_index,
        };
        let different_index = DiskFileIdentity {
            volume_serial_number: first.volume_serial_number,
            file_index: first.file_index + 1,
        };

        assert_eq!(first, same);
        assert_ne!(first, different_volume);
        assert_ne!(first, different_index);
    }

    #[test]
    fn disk_file_state_distinguishes_same_size_files_and_reports_hardlinks() {
        let temp = tempfile::tempdir().unwrap();
        let first_path = temp.path().join("first.bin");
        let second_path = temp.path().join("second.bin");
        let alias_path = temp.path().join("alias.bin");
        fs::write(&first_path, b"same-size").unwrap();
        fs::write(&second_path, b"same-size").unwrap();

        let first = fs::File::open(&first_path).unwrap();
        let second = fs::File::open(&second_path).unwrap();
        let first_state = disk_file_state(&first, &first_path).unwrap();
        let second_state = disk_file_state(&second, &second_path).unwrap();
        assert_ne!(first_state.identity, second_state.identity);
        assert_eq!(first_state.number_of_links, 1);
        assert_eq!(second_state.number_of_links, 1);

        fs::hard_link(&first_path, &alias_path).unwrap();
        let alias = fs::File::open(&alias_path).unwrap();
        let alias_state = disk_file_state(&alias, &alias_path).unwrap();
        assert_eq!(first_state.identity, alias_state.identity);
        assert_eq!(alias_state.number_of_links, 2);
    }

    #[test]
    fn disk_file_state_detects_path_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("input.bin");
        fs::write(&path, b"same-size").unwrap();
        let opened = fs::File::open(&path).unwrap();
        let opened_state = disk_file_state(&opened, &path).unwrap();

        fs::rename(&path, temp.path().join("old.bin")).unwrap();
        fs::write(&path, b"same-size").unwrap();
        let replacement = fs::File::open(&path).unwrap();
        let replacement_state = disk_file_state(&replacement, &path).unwrap();

        assert_ne!(opened_state.identity, replacement_state.identity);
    }

    #[test]
    fn retained_directory_handle_blocks_root_replacement_and_keeps_identity() {
        use std::os::windows::fs::OpenOptionsExt;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let replacement = temp.path().join("replacement");
        fs::create_dir(&root).unwrap();
        let retained = fs::OpenOptions::new()
            .read(true)
            .share_mode(root_share_mode())
            .custom_flags(root_open_flags())
            .open(&root)
            .unwrap();
        let before = disk_file_identity(&retained, &root).unwrap();

        assert!(fs::rename(&root, &replacement).is_err());
        assert_eq!(disk_file_identity(&retained, &root).unwrap(), before);
    }
}
