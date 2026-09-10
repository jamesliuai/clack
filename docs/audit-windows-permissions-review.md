# Independent Windows permissions review

**Historical review:** the hashes and source-only conclusions below describe the
original implementation. Native [CI run #16](https://github.com/jamesliuai/clack/actions/runs/34421894879)
subsequently found that its metadata-only directory handles did not prevent
rename. The follow-up correction requests `FILE_LIST_DIRECTORY` so those handles
participate in sharing checks while omitting `FILE_SHARE_DELETE`. The existing
native guard test remains the regression check; the historical review is not
independent validation of that correction. See the updated
[sharing contract](../vendor/clack-private-fs/SAFETY.md).

The final source review found no unresolved issue in the scoped Windows privacy
correction. This is source and cross-compilation evidence, not native Windows ACL
validation. The Windows implementation addresses SPEC §12 / PRIV-005 with the
documented ARCH-004 exception for a narrow unsafe platform adapter. The
application still forbids unsafe code.

The reviewed helper is
`vendor/clack-private-fs/src/lib.rs`, SHA-256
`563f88b19eaa563d11a4e9efb21fb3c7c9d024b8b50efe9c5c88b0a0c72cfc9e`.
Its eight native Windows tests are in `vendor/clack-private-fs/src/tests.rs`,
SHA-256 `58c11c627d9564ced664177ee8802c1f73ee13ca25e275b45333bf743b6a240c`.
The associated safety argument is in
[SAFETY.md](../vendor/clack-private-fs/SAFETY.md). The machine-readable
[review manifest](measurements/windows-permissions-review-final.json) records
the inspected integration files and their hashes.

## Resolved review findings

1. **SID bounds:** `IsValidAcl` alone does not establish a complete inline SID.
   The final code bounds the ACE header within `AclSize`, checks the allowed-ACE
   type and fixed SID header, then checks the advertised subauthority extent
   before calling the SID APIs. This closes the initial unsafe proof gap;
   no exploitable native failure was observed or claimed.
   [IsValidAcl](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-isvalidacl).
2. **Device aliases and normalization:** the final code rejects reserved
   superscript COM/LPT aliases and validates the original spelling before
   `std::path::absolute`, then validates the resolved path again. This matters
   because Windows normalization can remove trailing spaces and periods before
   a later check sees them. Ordinary relative dot resolution remains available;
   device namespaces, alternate streams and embedded NULs are refused.
   [Windows naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file),
   [Windows path normalization](https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats#trim-characters),
   [Rust 1.95 Windows path implementation](https://raw.githubusercontent.com/rust-lang/rust/1.95.0/library/std/src/sys/path/windows.rs).
3. **SQLite child ownership:** omitted creation ownership follows `TokenOwner`,
   which can differ from `TokenUser`. The guard now refuses unsupported default
   owners before SQLite opens. Existing children may be owned by Builtin
   Administrators only when that SID is the current process default owner;
   their sole effective full-control ACE must still name the current user.
   Newly created helper objects always have explicit current-user ownership.
   This is the primary agent's approved administrator trust boundary, with no
   process-token mutation or additional group grant.
   [Owner of a new object](https://learn.microsoft.com/en-us/windows/win32/secauthz/owner-of-a-new-object),
   [TOKEN_OWNER](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_owner).

## Requirement and safety checks

| Reviewed boundary | Source conclusion and evidence |
| --- | --- |
| Atomic private creation | `CREATE_NEW` receives a protected security descriptor before any payload. New directories get object/container inheritance. File kind and exact ACL are inspected through the returned handle before the caller can write. Existing objects are neither truncated nor ACL-modified. |
| Token alignment and lifetime | Both token queries allocate initialized, bounded `Vec<usize>` buffers. Their alignment accommodates the Win32 pointer-bearing structures, and embedded SID pointers remain within retained backing storage. The pseudo process handle is borrowed; the successful token handle is owned once. |
| Descriptor allocation | SID strings and security descriptors have one `LocalMemory` owner and one `LocalFree`. Owner, DACL and ACE pointers borrow the descriptor until all comparisons finish. No raw pointer escapes the adapter. |
| ACE layout | Win32 specifies DWORD alignment for ACLs and ACEs. The code verifies lengths and the variable SID extent before SID inspection, and accepts one effective current-user `FILE_ALL_ACCESS` allow ACE. A null, broad or unsuitable DACL fails closed. |
| Handle cleanup | Successful file handles become `File` once. Error unwinding releases token, descriptor and directory handles. Failed new-file readback marks the held object for deletion rather than unlinking a potentially replaced pathname. A failed directory creation/readback may leave an empty directory, with no private payload. |
| Reparse and replacement boundary | Each opened path component uses `OPEN_REPARSE_POINT` and a kind check. Ancestor and leaf directory handles omit `FILE_SHARE_DELETE`. The SQLite guard retains those handles through connection close; other file creation retains them through creation/readback. No shared ancestor permissions are changed. |
| SQLite lifetime and sidecars | `ReadStore.connection` precedes `_private_directory` in deliberate field drop order. Both writable and existing read-only opens validate the private parent and every present main database, rollback journal, WAL and SHM file before invoking SQLite. Future sidecars inherit the protected parent policy. Missing read-only history remains empty and creates nothing. |
| Application creation sites | Config uses a private temporary file and validated destination before atomic replacement. CLI exports, benchmark reports, recovery exports, language import directories/files and new history files use the helper under `cfg(windows)`. Remaining `OpenOptions` sites are non-Windows creation, existing terminal channels, or the existing test-only observer FD. |
| Architecture and scope | Safe API, pinned existing `windows-sys` dependency, no process policy mutation, asynchronous signal work, runtime network client or new worker. `unsafe_op_in_unsafe_fn` is denied locally; application `unsafe_code` remains forbidden. |

The Win32 ownership/allocation contracts were checked against
[GetTokenInformation](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation)
and [GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo).
The layout and sharing conclusions use
[ACCESS_ALLOWED_ACE](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-access_allowed_ace)
and [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew).
Microsoft documents that an NTFS directory must be empty before a reparse point
can be established; retaining the next inspected component constrains such
replacement of a nonempty ancestor.
[Reparse points](https://learn.microsoft.com/en-us/windows/win32/fileio/reparse-points).

## Validation and external work

The reviewer inspected all eight helper tests: broad-parent private creation,
protected/inherited children and directory pinning, broad existing-object
preservation, invalid names and wrong kinds, ordinary drive/UNC/verbatim lexical
controls, the actual runner's default-owner policy, reparse refusal, and repeated
error handle release. Application fixtures additionally cover Windows config
replacement/ADS refusal and actual SQLite WAL/read-store lifetime and unsaved
refusal. These are meaningful native tests prepared for Windows; they have not
executed on this macOS host. Lexical UNC tests do not access a server.

The release owner ran final Windows-target stable Clippy and Rust 1.88
all-target/all-feature checks, including the helper tests. The reviewer read
both successful logs and verified their hashes against the exact commands in
the [compilation report](measurements/windows-privacy-compilation.json). The
runtime source freeze is
`31586c3bc81633d5d3f912b7e717800caf185ecfce5358f7feeb92b3aa8129e3`.
This proves target-specific compilation and linting only. Parent-reported macOS test results
and pre-correction PTY reports do not establish Windows ACL enforcement.

Remaining external validation is native Windows execution under standard and
elevated tokens, the symlink-privileged reparse fixture, actual ACL-incapable
filesystem refusal, and real supported UNC/filesystem behavior. The threat
boundary trusts Windows and the filesystem server to enforce their reported
ACLs/sharing rules; administrator/backup privileges, already-authorized handles,
same-user processes and malicious filesystem servers are not excluded by a
user-only DACL. No native Windows, remote-share or hostile-filesystem success is
inferred from this review.
