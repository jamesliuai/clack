//! Atomic Windows owner-only creation for SPEC PRIV-005.
//!
//! The application crate remains unsafe-free. See SAFETY.md for this adapter's
//! demonstrated purpose, Win32 ownership contracts and ancestor trust boundary.
#![cfg(windows)]

use std::{
    ffi::{OsStr, c_void},
    fs::File,
    io,
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    ptr::{NonNull, null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACL,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        },
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid,
        GetSecurityDescriptorControl, GetTokenInformation, INHERIT_ONLY_ACE, IsValidAcl,
        IsValidSid, IsWellKnownSid, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PRESENT, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES,
        TOKEN_INFORMATION_CLASS, TOKEN_OWNER, TOKEN_QUERY, TOKEN_USER, TokenOwner, TokenUser,
        WinBuiltinAdministratorsSid,
    },
    Storage::FileSystem::{
        CREATE_NEW, CreateDirectoryW, CreateFileW, DELETE, FILE_ALL_ACCESS,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileAttributeTagInfo, FileDispositionInfo, GetFileInformationByHandleEx, OPEN_EXISTING,
        READ_CONTROL, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SetFileInformationByHandle,
    },
    System::{
        SystemServices::ACCESS_ALLOWED_ACE_TYPE,
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut result: Vec<_> = value.encode_wide().collect();
    if result.contains(&0) || result.len() >= 32_767 {
        return Err(invalid("Windows path or security descriptor is invalid"));
    }
    result.push(0);
    Ok(result)
}
fn absolute(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(invalid("private file path is empty"));
    }
    // GetFullPathNameW (used by std::path::absolute) can strip final dots and
    // spaces. Reject aliases in the original spelling before normalization.
    validate_components(path, true)?;
    let path = std::path::absolute(path)?;
    validate_components(&path, false)?;
    Ok(path)
}
fn validate_components(path: &Path, allow_dot_components: bool) -> io::Result<()> {
    wide(path.as_os_str())?;
    for part in path.components() {
        match part {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(_)
                | Prefix::VerbatimDisk(_)
                | Prefix::UNC(_, _)
                | Prefix::VerbatimUNC(_, _) => {}
                _ => return Err(invalid("device namespaces are not private file paths")),
            },
            Component::Normal(name) => {
                let units: Vec<_> = name.encode_wide().collect();
                if units.contains(&(b':' as u16))
                    || units.last().is_some_and(|last| matches!(*last, 32 | 46))
                {
                    return Err(invalid(
                        "alternate streams and aliased Windows paths are not supported",
                    ));
                }
                let stem = name
                    .to_string_lossy()
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_uppercase();
                if matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                ) || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
                    || ["COM", "LPT"].iter().any(|prefix| {
                        stem.strip_prefix(prefix)
                            .is_some_and(|suffix| matches!(suffix, "¹" | "²" | "³"))
                    })
                {
                    return Err(invalid(
                        "reserved Windows device name is not a private file",
                    ));
                }
            }
            Component::ParentDir | Component::CurDir => {
                if !allow_dot_components {
                    return Err(invalid(
                        "Windows private path must resolve its dot components",
                    ));
                }
            }
            Component::RootDir => {}
        }
    }
    Ok(())
}

/// Validates the destination namespace before a caller performs an atomic rename.
/// This lexical check does not create a path or change existing permissions;
/// creation APIs separately pin and inspect all existing parent directories.
pub fn validate_file_path(path: &Path) -> io::Result<()> {
    let path = absolute(path)?;
    if path.file_name().is_none() {
        return Err(invalid("private file path needs a file name"));
    }
    Ok(())
}

/// LocalAlloc-owned SID strings and security descriptors never escape this module.
struct LocalMemory(NonNull<c_void>);
impl LocalMemory {
    fn new(pointer: *mut c_void) -> io::Result<Self> {
        NonNull::new(pointer)
            .map(Self)
            .ok_or_else(|| io::Error::other("Windows returned no security descriptor"))
    }
}
impl Drop for LocalMemory {
    fn drop(&mut self) {
        // SAFETY: only successful LocalAlloc-returning Win32 calls construct this
        // owner; it is not cloned and its allocation is released exactly once.
        unsafe {
            LocalFree(self.0.as_ptr());
        }
    }
}

struct Identity {
    // usize alignment is sufficient for TOKEN_USER and its inline SID storage.
    storage: Vec<usize>,
    owner_storage: Vec<usize>,
}
impl Identity {
    fn current() -> io::Result<Self> {
        let mut raw = null_mut();
        // SAFETY: GetCurrentProcess is a borrowed pseudo-handle; raw is a writable
        // out parameter, and successful OpenProcessToken returns one owned handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful OpenProcessToken returned a unique valid handle.
        let token = unsafe { OwnedHandle::from_raw_handle(raw) };
        Ok(Self {
            storage: token_information(&token, TokenUser, size_of::<TOKEN_USER>())?,
            owner_storage: token_information(&token, TokenOwner, size_of::<TOKEN_OWNER>())?,
        })
    }
    fn sid(&self) -> PSID {
        // SAFETY: current() initialized an aligned TOKEN_USER; self owns its
        // complete backing buffer for the lifetime of this borrowed pointer.
        unsafe { (*self.storage.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
    fn default_owner(&self) -> PSID {
        // SAFETY: current() initialized an aligned TOKEN_OWNER and retained its
        // backing buffer; its SID pointer remains valid until self is dropped.
        unsafe { (*self.owner_storage.as_ptr().cast::<TOKEN_OWNER>()).Owner }
    }
    fn require_supported_child_owner(&self) -> io::Result<()> {
        // SAFETY: both pointers borrow valid token information retained by self.
        // SQLite's newly created children use this token default, not their
        // parent's owner. Refuse any unreviewed group owner before SQLite opens.
        if unsafe {
            EqualSid(self.default_owner(), self.sid()) == 0
                && IsWellKnownSid(self.default_owner(), WinBuiltinAdministratorsSid) == 0
        } {
            return Err(denied(
                "Windows default file owner must be the current user or Builtin Administrators",
            ));
        }
        Ok(())
    }
    fn descriptor(&self, directory: bool) -> io::Result<LocalMemory> {
        let mut text = null_mut();
        // SAFETY: sid() refers to the valid, live TokenUser SID; text is an out pointer.
        if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let allocation = LocalMemory::new(text.cast())?;
        let mut length = 0;
        // SAFETY: this API returns a NUL-terminated UTF-16 SID string. Its maximum
        // documented SID representation is shorter than 256 UTF-16 units.
        while unsafe { *text.add(length) } != 0 {
            length += 1;
            if length >= 256 {
                return Err(io::Error::other("invalid Windows SID string"));
            }
        }
        // SAFETY: length identifies the initialized string in the live allocation.
        let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
            .map_err(|_| io::Error::other("invalid Windows SID encoding"))?;
        drop(allocation);
        let inheritance = if directory { "OICI" } else { "" };
        let sddl = format!("O:{sid}D:P(A;{inheritance};FA;;;{sid})");
        let text = wide(OsStr::new(&sddl))?;
        let mut descriptor = null_mut();
        // SAFETY: text is terminated and live; the API allocates a validated
        // security descriptor owned exclusively by the resulting LocalMemory.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        LocalMemory::new(descriptor)
    }
}

fn token_information(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
    minimum: usize,
) -> io::Result<Vec<usize>> {
    let mut needed = 0;
    // SAFETY: the documented zero-length query writes only needed.
    let first =
        unsafe { GetTokenInformation(token.as_raw_handle(), class, null_mut(), 0, &mut needed) };
    let error = io::Error::last_os_error();
    if first != 0
        || error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || needed < minimum as u32
        || needed > 16_384
    {
        return Err(io::Error::other(
            "cannot determine the current Windows user SID",
        ));
    }
    let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    // SAFETY: the aligned allocation contains at least needed writable bytes;
    // TokenUser/TokenOwner writes valid data with pointers into that allocation.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            class,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(storage)
}

fn attributes(descriptor: &LocalMemory) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.as_ptr(),
        bInheritHandle: 0,
    }
}
fn owned_file(raw: HANDLE) -> io::Result<File> {
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    if raw.is_null() {
        return Err(io::Error::other("Windows returned no file handle"));
    }
    // SAFETY: callers pass only a successful CreateFileW handle, exactly once.
    Ok(unsafe { File::from_raw_handle(raw) })
}
fn kind(file: &File, directory: bool) -> io::Result<()> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: info is a correctly sized initialized output buffer; file stays live.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
    {
        return Err(denied(
            "private path must be a regular file/directory, without reparse points",
        ));
    }
    Ok(())
}
fn open_existing(path: &Path, directory: bool) -> io::Result<File> {
    let path = wide(path.as_os_str())?;
    // Metadata-only opens do not participate in Windows sharing checks. A
    // directory guard needs list access for omitted FILE_SHARE_DELETE to pin
    // the directory against rename/delete for the lifetime of this handle.
    let access =
        READ_CONTROL | FILE_READ_ATTRIBUTES | if directory { FILE_LIST_DIRECTORY } else { 0 };
    // SAFETY: terminated path and no security attributes; OPEN_EXISTING cannot
    // create or truncate. Directory list access is read-only.
    let file = owned_file(unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS
                | FILE_FLAG_OPEN_REPARSE_POINT
                | SECURITY_SQOS_PRESENT
                | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    })?;
    kind(&file, directory)?;
    Ok(file)
}
fn check_acl(file: &File, identity: &Identity, directory: bool, protected: bool) -> io::Result<()> {
    let mut owner = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: valid live handle and correctly typed out pointers. All returned
    // owner/ACL pointers borrow the single LocalAlloc descriptor allocation.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let descriptor = LocalMemory::new(descriptor)?;
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: descriptor is the valid live result of GetSecurityInfo.
    if unsafe { GetSecurityDescriptorControl(descriptor.0.as_ptr(), &mut control, &mut revision) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    if owner.is_null()
        || dacl.is_null()
        || control & SE_DACL_PRESENT == 0
        || (protected && control & SE_DACL_PROTECTED == 0)
    {
        return Err(denied(
            "private path requires an explicit protected owner-only DACL",
        ));
    }
    // SAFETY: Windows returned validated descriptor components within descriptor;
    // identity retains its SID buffer. Neither allocation is changed or freed here.
    if unsafe {
        IsValidSid(owner) == 0
            || (EqualSid(owner, identity.sid()) == 0
                && (protected
                    || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) == 0
                    || EqualSid(owner, identity.default_owner()) == 0))
            || IsValidAcl(dacl) == 0
            || (*dacl).AceCount != 1
    } {
        return Err(denied(
            "private path is not owned exclusively by the current Windows user",
        ));
    }
    let mut raw = null_mut();
    // SAFETY: dacl was validated and contains exactly one ACE; raw is writable.
    if unsafe { GetAce(dacl, 0, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if raw.is_null() {
        return Err(denied("private DACL has no access entry"));
    }
    // SAFETY: GetSecurityInfo returned a valid ACL allocation. Bound the ACE
    // header within that ACL before reading it, independently of IsValidAcl.
    let acl_size = unsafe { (*dacl).AclSize as usize };
    let ace_offset = (raw as usize)
        .checked_sub(dacl as usize)
        .filter(|offset| *offset <= acl_size)
        .ok_or_else(|| denied("private DACL access entry is outside its allocation"))?;
    let available = acl_size - ace_offset;
    if available < size_of::<windows_sys::Win32::Security::ACE_HEADER>() {
        return Err(denied("private DACL access entry is truncated"));
    }
    // SAFETY: GetAce provides the initialized ACE_HEADER. Validate its advertised
    // type/length before reading the ACCESS_ALLOWED_ACE prefix or inline SID.
    let header = unsafe { &*raw.cast::<windows_sys::Win32::Security::ACE_HEADER>() };
    if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE
        || usize::from(header.AceSize) > available
        || usize::from(header.AceSize) < offset_of!(ACCESS_ALLOWED_ACE, SidStart) + 8
        || u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0
        || (directory
            && u32::from(header.AceFlags) & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE)
                != (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE))
    {
        return Err(denied(
            "private DACL must grant effective inheritable owner access",
        ));
    }
    // SAFETY: validated ACE type and size guarantee this fixed prefix exists.
    let ace = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
    let sid: PSID = std::ptr::addr_of!(ace.SidStart).cast_mut().cast();
    // SAFETY: the fixed SID header (revision, subauthority count, authority) is
    // eight bytes, all bounded above. Check the variable portion before passing
    // its pointer to APIs that inspect the complete SID.
    let subauthorities = unsafe { *sid.cast::<u8>().add(1) } as usize;
    let sid_length = 8 + subauthorities * size_of::<u32>();
    if subauthorities > 15
        || sid_length > usize::from(header.AceSize) - offset_of!(ACCESS_ALLOWED_ACE, SidStart)
    {
        return Err(denied("private DACL SID is truncated or invalid"));
    }
    // SAFETY: the full SID extent is now contained in the validated ACE and its
    // backing descriptor remains live throughout validation and comparison.
    if ace.Mask != FILE_ALL_ACCESS
        || unsafe { IsValidSid(sid) } == 0
        || unsafe { GetLengthSid(sid) as usize } != sid_length
        || unsafe { EqualSid(sid, identity.sid()) } == 0
    {
        return Err(denied(
            "private DACL permits an identity other than its full-control owner",
        ));
    }
    Ok(())
}

/// Holds ancestor and leaf directory handles without FILE_SHARE_DELETE.
/// Keep this guard alive until the SQLite connection and journals are closed.
#[derive(Debug)]
pub struct PrivateDirectory {
    _handles: Vec<File>,
}

fn directories(path: &Path, create: bool) -> io::Result<Vec<File>> {
    let mut handles = Vec::new();
    let mut current = PathBuf::new();
    let identity = Identity::current()?;
    let descriptor = identity.descriptor(true)?;
    let security = attributes(&descriptor);
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let file = match open_existing(&current, true) {
            Ok(file) => file,
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                let name = wide(current.as_os_str())?;
                // SAFETY: terminated path, valid descriptor and attributes remain
                // live throughout this atomic creation call. No existing ACL changes.
                if unsafe { CreateDirectoryW(name.as_ptr(), &security) } == 0 {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
                let file = open_existing(&current, true)?;
                check_acl(&file, &identity, true, true)?;
                file
            }
            Err(error) => return Err(error),
        };
        handles.push(file);
    }
    Ok(handles)
}

/// Creates only missing directories with protected, owner-only inheritable ACLs.
/// Existing ancestors are checked for reparse points and are never ACL-modified.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    directories(&absolute(path)?, true).map(drop)
}

/// Creates exactly one new private directory without accepting an existing path.
/// The caller's parent must already exist; its ACL is never changed.
pub fn create_dir(path: &Path) -> io::Result<()> {
    let path = absolute(path)?;
    let _parents = directories(
        path.parent()
            .ok_or_else(|| invalid("private directory needs a parent"))?,
        false,
    )?;
    let identity = Identity::current()?;
    let descriptor = identity.descriptor(true)?;
    let security = attributes(&descriptor);
    let name = wide(path.as_os_str())?;
    // SAFETY: terminated path and a valid descriptor remain live through this
    // exclusive creation; an existing object produces AlreadyExists unchanged.
    if unsafe { CreateDirectoryW(name.as_ptr(), &security) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let file = open_existing(&path, true)?;
    check_acl(&file, &identity, true, true)
}

/// Opens and validates a private directory, retaining its path against replacement.
pub fn require_private_directory(path: &Path) -> io::Result<PrivateDirectory> {
    let handles = directories(&absolute(path)?, false)?;
    let file = handles
        .last()
        .ok_or_else(|| invalid("private directory has no leaf"))?;
    let identity = Identity::current()?;
    check_acl(file, &identity, true, true)?;
    identity.require_supported_child_owner()?;
    Ok(PrivateDirectory { _handles: handles })
}

/// Refuses existing broad files; never changes an ACL or writes content.
/// Inherited owner-only ACLs are accepted for SQLite-created journal/sidecar files.
/// An Administrators owner is accepted only when it is this process's default
/// token owner; the DACL must still grant only the current user's SID.
pub fn require_private_file(path: &Path) -> io::Result<()> {
    let path = absolute(path)?;
    let _parents = directories(
        path.parent()
            .ok_or_else(|| invalid("private file needs a parent"))?,
        false,
    )?;
    let file = open_existing(&path, false)?;
    check_acl(&file, &Identity::current()?, false, false)
}

/// Atomically creates a new file with a protected current-user DACL. The descriptor
/// and file kind are read back from its handle before any payload can be written.
pub fn create_new_file(path: &Path) -> io::Result<File> {
    let path = absolute(path)?;
    let _parents = directories(
        path.parent()
            .ok_or_else(|| invalid("private file needs a parent"))?,
        false,
    )?;
    let identity = Identity::current()?;
    let descriptor = identity.descriptor(false)?;
    let security = attributes(&descriptor);
    let name = wide(path.as_os_str())?;
    // SAFETY: all pointers refer to initialized live data; CREATE_NEW cannot open
    // or truncate an existing object. owned_file immediately owns a successful handle.
    let file = owned_file(unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &security,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL
                | FILE_FLAG_OPEN_REPARSE_POINT
                | SECURITY_SQOS_PRESENT
                | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    })?;
    if let Err(error) = kind(&file, false).and_then(|()| check_acl(&file, &identity, false, true)) {
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: mark only the object held by this new handle for deletion; this
        // cannot follow a swapped pathname. No private payload has been written.
        let _ = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        return Err(error);
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
