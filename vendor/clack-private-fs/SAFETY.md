# Windows private filesystem adapter

SPEC §12 / PRIV-005 requires user-only access for new private files wherever the
platform supports it. Windows supports discretionary ACLs, but default Rust file
creation inherits the parent descriptor. That is insufficient for an explicit
shared `--data-dir` or export/config destination. This crate is the narrow,
reviewed architecture exception for that demonstrated requirement. The main
application still forbids unsafe code. The adapter uses the already locked
`windows-sys` 0.61.2, adds no registry dependency, and is absent on Unix.

Rust's stable Windows `OpenOptionsExt` exposes attributes and sharing flags but
does not accept a creation security descriptor. Cached `tempfile` and
`winapi-util` APIs do not provide the required atomic descriptor creation. An ACL
applied after writing would leave a disclosure window. The small local adapter
therefore uses the documented Win32 creation and handle inspection APIs rather
than changing permissions after opening private content.

## Policy and integration

New files use `CREATE_NEW` with an explicit process-user owner and a protected
DACL containing exactly one effective full-control allow entry for that SID.
New directories use the same policy with object/container inheritance. Existing
ancestors are never ACL-modified. Handles are read back for regular file/directory
kind and exact ACL policy before a new file is returned to its writer. Failed
file readback marks that exact new handle for deletion; no payload has been
written. A failed directory readback can leave an empty directory, never private
content.

Microsoft documents that omitted creation descriptors inherit from the parent,
existing objects ignore creation descriptors, and existing/moved child ACLs are
independent of parent access. These rules explain both atomic creation and
individual database/sidecar checks. [File security and access rights](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights),
[CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew).

SQLite persistence additionally requires a protected private parent. Storage
retains `PrivateDirectory` after its `Connection` field so the connection and
journals close before the guard. The guard keeps every inspected ancestor and
the leaf open without `FILE_SHARE_DELETE`; Windows then refuses conflicting
delete/rename access until those handles close. Existing database, journal, WAL
and SHM files must each have only the current user's effective full-control ACE.
New SQLite children inherit that same ACE. Broad existing objects are refused
without changing their ACLs; callers report an honest unsaved/storage error.

Windows assigns a default new-object owner from `TokenOwner`, which can differ
from `TokenUser`. Before SQLite opens, the guard rejects default owners other
than the current user or Builtin Administrators. Existing children may have an
Administrators owner only if it equals this process's default owner; their DACL
still grants only the current SID. No arbitrary group owner or extra group ACE
is accepted and no token is mutated. This permits ordinary elevated operation
within the unavoidable administrator/backup-privilege trust boundary. All
helper-created files and directories remain explicitly user-owned.
[Owner of a new object](https://learn.microsoft.com/en-us/windows/win32/secauthz/owner-of-a-new-object),
[TOKEN_OWNER](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_owner).

Config replacement validates its final destination namespace and writes a private
temporary file before sync/rename. CLI exports, benchmark reports, recovery
exports and imported language files also use atomic private creation. Unix
creation and permission code remains unchanged.

## Unsafe ownership and bounds

- `OpenProcessToken` returns one token handle, immediately wrapped in
  `OwnedHandle`. The process pseudo-handle is borrowed and never closed.
  `TokenUser`/`TokenOwner` queries use bounded, pointer-aligned `Vec<usize>`
  buffers. Embedded SID pointers remain borrowed from the retained buffers.
- `CreateFileW` successful handles become `File` exactly once. Both invalid and
  null handles are rejected. RAII closes success and error paths; the public API
  never exposes a raw handle or asks the caller to free memory.
- Conversion and `GetSecurityInfo` results are unique `LocalMemory` owners and
  are released only with `LocalFree`. Owner/ACL/SID pointers never outlive their
  descriptor. UTF-16 input is terminated and live for every call.
- `GetAce` output is bounded within `AclSize` before its header is read. The
  allowed-ACE type and complete fixed SID header are checked before the inline
  SID is accessed. Its subauthority count/extent must fit the ACE before
  `IsValidSid`, `GetLengthSid` or `EqualSid` sees it. No unaligned packed Rust
  layout is assumed: these are the Win32 C layouts supplied by `windows-sys`.
  `IsValidAcl` alone is deliberately insufficient for this proof.
  [GetSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo),
  [IsValidAcl](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-isvalidacl).
- Every unsafe block has a local argument/lifetime justification, and
  `unsafe_op_in_unsafe_fn` is denied. There are no unsafe public APIs, globals,
  signal handlers, token changes, network clients or background workers.

## Path and threat boundaries

Ordinary drive and UNC filesystem prefixes, including their verbatim variants,
are supported. Embedded NULs, alternate streams, device namespaces, reserved
device names (including COM/LPT superscript digits), trailing-dot/space aliases
and remaining dot components are refused. Every opened directory component and
file leaf uses `OPEN_REPARSE_POINT` and is inspected to reject reparse points.
No test contacts a UNC server merely to validate lexical syntax.
[Windows naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

The policy assumes the current process identity is the user requesting storage,
and the operating system/filesystem server correctly enforces its reported ACLs
and sharing rules. It does not defend against administrator/backup privileges,
kernel compromise, a malicious remote filesystem, an attacker already holding
an authorized handle, or other processes running as this same user. Shared
ancestors are pinned during path inspection/creation; an existing SQLite private
directory remains pinned for the full connection lifetime. The adapter never
relaxes shared ancestor permissions to obtain access. Unsupported ACL or reparse
paths fail before private payload is written.

## Validation evidence

`src/tests.rs` defines native Windows adversarial tests for deliberately broad
parents, protected new files/directories, inherited journal ACLs, moved broad
children, no-overwrite collisions, lexical device/stream rejection, reparse
paths, token-default ownership and handle cleanup after repeated errors.
The reparse fixture requires native symlink privilege and fails explicitly if
the runner cannot provide it. Application tests additionally cover config
replacement/ADS refusal, SQLite lifetime/unsaved refusal and CLI private exports.

Root workspace defaults include this crate, so locked all-target Clippy and
tests cover it on the Windows CI runner. A Windows cross-compile checks types
and target-specific linting, not live ACL enforcement. Native Windows execution
must be recorded separately; no such execution is claimed by this document.
