# Stage 2 administrator verification — completed

The agent ran the final script through normal Windows RunAs on **2026-10-12,
16:15:50–16:17:45 UTC**. **All 24 controller and ordinary-user checks pass**.
The installed helper hash is
`151539B785DBAF56E80AC8F403BCF383988A48762FE8A13B47F21652700D9560`.
The real Downloads (500), Documents (565) and Desktop (24) have zero missing,
extra or stale entries individually and within the whole-drive index. The
3,015-entry first/live/closed fixture, 10/10 accuracy, denied descendants,
deleted-parent replay and invalidated-history recovery all pass.

First whole-C: index **helper 21.932 s / walker 22.768 s**; approximately
3.016M entries. Saved whole-C: app cached presentation **277.791 ms**, USN
catch-up **2.732 s**, private working set **45.70 MiB**, close/exit
**110.675 ms / 0**. Helper after disconnect: **0 ms CPU / 3.147 s**, private
working set **8.97 MiB**. These are live-drive measurements with existing
OS caches, not fsearch parity or sustained-idle certification.
See [short results](benchmarks/stage2-summary.json) and
[all results](benchmarks/stage2-final-results.json).

Four low-priority native metadata workers use independent volume handles and
disjoint file-reference ranges covering all 48-bit record ordinals, including
MFT growth after geometry was read. Sixteen bounded impersonated workers
check ordinary-user directory access; authorized parents precede descendants.
Logical kernel metadata retains hard links, resident/empty stream lengths and
provider folders. Bounded historical replay recovers deleted-parent paths;
unresolved/corrupt history still explicitly reconciles. Intentional pipe
cancellation is recognized before fallback, preventing a drive walk on quit.
The controller uses the ordinary Explorer parent and checks the suspended
child's actual SID/non-elevated token before resume. No account, token,
desktop or unrelated service permissions were changed.

There is no remaining administrator acceptance block for Stage 2. The script
below is retained for reproduction and provides the helper uninstall option;
you do not need to repeat the passed run before Stage 3.
Close Seek and any seek-cli command. Open **administrator PowerShell from the
same Windows account that owns this checkout**, then run the single script:
Use Start, type **PowerShell**, and choose **Run as administrator**. Opening a
normal PowerShell or passing `-ExecutionPolicy Bypass` does not grant elevation.

Alternatively, paste this into ordinary PowerShell and choose **Yes** at the
Windows administrator prompt. The new window stays open to show the results:

```powershell
Start-Process "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe" -Verb RunAs -ArgumentList '-NoExit -NoProfile -ExecutionPolicy Bypass -File "%USERPROFILE%\Seek\scripts\finish-stage2.ps1"'
```

If PowerShell is already elevated, use:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File %USERPROFILE%\Seek\scripts\finish-stage2.ps1
```

Allow several minutes for two first whole-C: indexes. No compiler is required
for this run: the rebuilt release executables are already in
`target/x86_64-pc-windows-msvc/release`.

The controller updates only `SeekHelper-<your SID>` and its protected Program
Files binary. It verifies the binary hash, service identity and permissions.
It launches all engine and app checks with that user's **ordinary desktop
token**, and refuses a different administrator account or elevated verifier.
An early helper/correctness failure skips the expensive whole-drive timings.

Checks include helper first/live/closed correctness and top-five accuracy;
the actual Windows Downloads, Documents and Desktop (including redirection);
recovery after invalidating only owned saved checkpoints, including closed
create/rename/move/delete changes; a temporary denied-directory check that
must not leak children from the elevated catalog; replay of closed changes
including a deleted nested parent without a reset/full reconciliation;
separate initial C: walker/helper timings;
and coverage of the three real folders inside the resulting whole-drive index.
When correctness and speed pass, it measures whole-C: saved-index application
launch/catch-up, memory and window-close exit. Measurement messages use a
user-restricted pipe, avoiding trace writes on the indexed C: drive. It also
samples the helper's CPU and memory with no connected client.

All results, failures and commands are collected in **one file**:
`docs/benchmarks/stage2-final-results.json`. Intermediate artifacts stay in a
new `.bench-data/stage2-final/run-*` directory. Tell the agent **done** after
the script exits, even if it reports failure; it will read that file itself.

Safety: real user folders are read-only; no file contents are intentionally
read. OneDrive metadata listing can contact its provider. The script never
changes Windows' journal, accounts, policies, permissions outside Seek's
helper, unrelated services, scheduled tasks or app startup. It does not stop
an existing app; it asks that Seek be closed before starting. Fixtures are
new owned files; the denied-directory check restores its original ACL in a
`finally` block. Only the newly created, path-checked deleted-parent fixture
is removed recursively. Only exact child processes
created by the script can be terminated on a timeout.

The speed gate requires an actual NTFS baseline and a first index faster than
the forced walker run. Journal counters distinguish replay, history reset,
conservative reconciliation and NTFS fallback. Provider-backed Known Folder
roots may intentionally use listing plus USN; decoder failures cannot pass
that exception. These sequential live-drive timings do not establish a
cache-cold launch or parity with fsearch. Saved whole-C: app launch must use
replay without a history reset or full reconciliation. No Stage 3 work begins
automatically.

To uninstall **only Seek's optional helper** afterwards, use the same script
from administrator PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File %USERPROFILE%\Seek\scripts\finish-stage2.ps1 -Uninstall
```

This leaves user files and indexes intact. Normal walker indexing remains
available with `seek.exe --walker`. Uninstall writes its own result into the
same results file while preserving validation results.

For a read-only script compilation/path check without elevation, run with
`-CheckOnly`. This also launches a read-only CLI child through the ordinary
desktop parent and checks its actual token before resuming it. It does not
update the helper or run the long drive tests, and does not certify an elevated
worker launch.

Build prerequisites: Rust **1.96.0 MSVC**, Visual Studio 2022 C++ Build Tools,
Windows SDK **10.0.26100.0**. Rebuild using
`cargo build --workspace --release --all-features --locked`.

Primary API references: [file-record retrieval](https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ni-winioctl-fsctl_get_ntfs_file_record),
[bulk NTFS layouts](https://learn.microsoft.com/en-us/windows-hardware/drivers/ifs/fsctl-query-file-layout),
[layout selection](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_query_file_layout_input),
[directory ID opens](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-openfilebyid).
The launcher uses
[CreateProcessW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw)
and the documented token inheritance from
[PROC_THREAD_ATTRIBUTE_PARENT_PROCESS](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).
Service exit waiting uses a running-process ID from
[QueryServiceStatusEx](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-queryservicestatusex)
and a retained process handle with
[WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject).
