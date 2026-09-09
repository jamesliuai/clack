//! Native Windows tests: the broad-ACL controls would fail if creation merely
//! inherited the parent's permissions. These tests are compiled on other hosts
//! only with an explicit Windows target and must run on Windows for evidence.
use super::*;
use std::{
    fs,
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "clack-private-fs-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("cannot create Windows ACL fixture: {error}"),
            }
        }
        panic!("cannot allocate a Windows ACL fixture")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn broad_directory(path: &Path) {
    let text = wide(OsStr::new("D:P(A;OICI;FA;;;WD)")).unwrap();
    let mut descriptor = null_mut();
    // SAFETY: a constant terminated SDDL descriptor is converted into one owned
    // LocalAlloc allocation; the out pointer is valid and the call is checked.
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        },
        0
    );
    let descriptor = LocalMemory::new(descriptor).unwrap();
    let name = wide(path.as_os_str()).unwrap();
    let attributes = attributes(&descriptor);
    // SAFETY: this creates only this test's unique new empty directory; buffers
    // remain alive and no real user ancestor's permissions are modified.
    assert_ne!(unsafe { CreateDirectoryW(name.as_ptr(), &attributes) }, 0);
}

fn sddl(path: &Path, directory: bool) -> String {
    let file = open_existing(path, directory).unwrap();
    let mut descriptor = null_mut();
    // SAFETY: the held file and descriptor out pointer are valid; the allocated
    // descriptor is immediately placed into a unique LocalMemory owner.
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let descriptor = LocalMemory::new(descriptor).unwrap();
    let mut text = null_mut();
    let mut length = 0;
    // SAFETY: the source descriptor remains live and the API allocates the
    // terminated result plus its complete length in UTF-16 code units.
    assert_ne!(
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor.0.as_ptr(),
                SDDL_REVISION_1,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut text,
                &mut length,
            )
        },
        0
    );
    let _text = LocalMemory::new(text.cast()).unwrap();
    assert!(length > 0);
    // SAFETY: the API-supplied string length includes the terminating NUL and
    // describes initialized UTF-16 in the still-live LocalMemory allocation.
    String::from_utf16(unsafe { std::slice::from_raw_parts(text, length as usize - 1) }).unwrap()
}

#[test]
fn atomic_file_creation_is_private_under_a_deliberately_broad_parent() {
    let fixture = Fixture::new();
    let broad = fixture.0.join("shared");
    broad_directory(&broad);
    let before = sddl(&broad, true);
    assert!(before.contains(";;;WD)"), "negative-control ACL: {before}");
    let path = broad.join("export.json");
    let mut file = create_new_file(&path).unwrap();
    // Creation has already checked the descriptor before this first payload.
    file.write_all(b"private export\n").unwrap();
    drop(file);
    let actual = sddl(&path, false);
    assert!(actual.contains("D:P"), "not protected: {actual}");
    assert_eq!(actual.matches("(A;").count(), 1, "{actual}");
    assert!(
        !actual.contains(";;;WD)"),
        "inherited broad access: {actual}"
    );
    assert!(
        !actual.contains(";;;BA)"),
        "inherited administrator group: {actual}"
    );
    require_private_file(&path).unwrap();
    assert_eq!(
        sddl(&broad, true),
        before,
        "existing parent ACL was changed"
    );
    assert_eq!(fs::read(&path).unwrap(), b"private export\n");
}

#[test]
fn private_directory_protects_sqlite_style_inherited_children() {
    let fixture = Fixture::new();
    let broad = fixture.0.join("shared");
    broad_directory(&broad);
    let private = broad.join("clack");
    create_dir_all(&private).unwrap();
    let directory_acl = sddl(&private, true);
    assert!(directory_acl.contains("D:P"), "{directory_acl}");
    assert!(
        directory_acl.contains("OICI"),
        "missing child inheritance: {directory_acl}"
    );
    assert!(!directory_acl.contains(";;;WD)"), "{directory_acl}");
    let guard = require_private_directory(&private).unwrap();
    for name in [
        "history.sqlite3",
        "history.sqlite3-journal",
        "history.sqlite3-wal",
        "history.sqlite3-shm",
    ] {
        let path = private.join(name);
        // std creation is intentional: SQLite also creates new sidecars by
        // inheriting the parent's policy instead of calling this adapter.
        fs::write(&path, b"SQLite child fixture").unwrap();
        let acl = sddl(&path, false);
        assert!(!acl.contains(";;;WD)"), "{name}: {acl}");
        assert_eq!(acl.matches("(A;").count(), 1, "{name}: {acl}");
        require_private_file(&path).unwrap();
    }
    assert!(fs::rename(&private, broad.join("moved")).is_err());
    assert!(fs::rename(&broad, fixture.0.join("swapped")).is_err());
    drop(guard);
    fs::rename(&private, broad.join("moved")).unwrap();
}

#[test]
fn broad_existing_objects_are_refused_without_rewriting_or_truncating() {
    let fixture = Fixture::new();
    let broad = fixture.0.join("shared");
    broad_directory(&broad);
    let path = broad.join("history.sqlite3");
    fs::write(&path, b"preserve this content").unwrap();
    let before_file = sddl(&path, false);
    let before_parent = sddl(&broad, true);
    assert!(before_file.contains(";;;WD)"), "{before_file}");
    assert_eq!(
        require_private_directory(&broad).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        require_private_file(&path).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        create_new_file(&path).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        create_dir(&broad).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(&path).unwrap(), b"preserve this content");
    assert_eq!(sddl(&path, false), before_file);
    assert_eq!(sddl(&broad, true), before_parent);
    // Moving a permissive existing child into a private parent preserves its
    // file ACL; directory protection alone must not satisfy validation.
    let moved = fixture.0.join("moved.sqlite3");
    fs::rename(&path, &moved).unwrap();
    assert!(require_private_file(&moved).is_err());
    assert_eq!(fs::read(moved).unwrap(), b"preserve this content");
}

#[test]
fn invalid_names_and_wrong_object_kinds_never_create_payloads() {
    let fixture = Fixture::new();
    for name in [
        "history.sqlite3:private",
        "NUL",
        "CON.txt",
        "COM1",
        "LPT9",
        "COM¹.txt",
        "COM²",
        "COM³.dat",
        "LPT¹",
        "LPT².txt",
        "LPT³",
        "aliased.",
        "aliased ",
    ] {
        let path = fixture.0.join(name);
        assert_eq!(
            create_new_file(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidInput,
            "{name}"
        );
        assert!(validate_file_path(&path).is_err(), "{name}");
    }
    assert!(validate_file_path(Path::new(r"\\.\PhysicalDrive0")).is_err());
    assert!(
        validate_file_path(Path::new(r"\\?\GLOBALROOT\Device\HarddiskVolume1\private")).is_err()
    );
    let directory = fixture.0.join("directory");
    create_dir(&directory).unwrap();
    assert!(require_private_file(&directory).is_err());
    let file = fixture.0.join("file");
    drop(create_new_file(&file).unwrap());
    assert!(require_private_directory(&file).is_err());
    assert!(create_new_file(&file.join("child")).is_err());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
}

#[test]
fn lexical_validation_preserves_filesystem_prefixes_without_accessing_them() {
    for path in [
        r"C:\ordinary\config.toml",
        r"\\server\share\private\export.json",
        r"\\?\C:\ordinary\config.toml",
        r"\\?\UNC\server\share\private\export.json",
    ] {
        validate_file_path(Path::new(path)).unwrap();
    }
    for path in [
        "C:\\ordinary\\bad\0name",
        r"C:\ordinary\name:stream",
        r"C:\ordinary\name::$DATA",
    ] {
        assert_eq!(
            validate_file_path(Path::new(path)).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn inherited_children_follow_the_reviewed_process_default_owner_policy() {
    let fixture = Fixture::new();
    let identity = Identity::current().unwrap();
    identity.require_supported_child_owner().unwrap();
    let path = fixture.0.join("inherited.sqlite3-wal");
    fs::write(&path, b"owner-default fixture").unwrap();
    let file = open_existing(&path, false).unwrap();
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: all out parameters are valid; owner borrows the single allocated
    // descriptor, kept alive through the comparisons below.
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    let _descriptor = LocalMemory::new(descriptor).unwrap();
    // SAFETY: GetSecurityInfo and TokenOwner provide live, valid SID buffers.
    assert_ne!(unsafe { EqualSid(owner, identity.default_owner()) }, 0);
    // The test exercises the actual runner's default. It does not simulate an
    // elevated token on a standard-user host or change process token policy.
    check_acl(&file, &identity, false, false).unwrap();
    drop(file);
    require_private_file(&path).unwrap();
    eprintln!("native default owner: {}", sddl(&path, false));
}

#[test]
fn reparse_points_are_refused_at_the_leaf_and_in_ancestors() {
    let fixture = Fixture::new();
    let real = fixture.0.join("real");
    create_dir(&real).unwrap();
    let linked = fixture.0.join("linked");
    std::os::windows::fs::symlink_dir(&real, &linked)
        .expect("native Windows privacy CI requires directory symlink creation privilege");
    assert!(require_private_directory(&linked).is_err());
    assert!(create_new_file(&linked.join("payload")).is_err());
    assert!(!real.join("payload").exists());
    fs::remove_dir(&linked).unwrap();
}

#[test]
fn repeated_errors_release_handles_and_preserve_existing_acl() {
    let fixture = Fixture::new();
    let broad = fixture.0.join("shared");
    broad_directory(&broad);
    let before = sddl(&broad, true);
    for _ in 0..128 {
        assert!(require_private_directory(&broad).is_err());
        assert!(require_private_file(&broad.join("absent")).is_err());
        assert!(create_new_file(&broad.join("absent-parent/file")).is_err());
    }
    // Both ancestor and leaf no-delete-sharing handles must have been closed on
    // every error branch; otherwise these renames fail with a sharing violation.
    let moved = fixture.0.join("moved");
    fs::rename(&broad, &moved).unwrap();
    assert_eq!(sddl(&moved, true), before);
    fs::remove_dir(moved).unwrap();
}
