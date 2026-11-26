# ADR-0095: Windows Interactive Login Task

Status: Accepted; Windows login/UAC validation pending maintainer testing.
Date: 2026-10-24
Supersedes: Windows backend in ADR-0043; macOS is unchanged.

## Cause and Reference

Issue #1161 reports login startup failing after enabling the executable's
"Run this program as an administrator" compatibility option. The permission
flow recommends exactly that option. `auto-launch` 0.6.0 registers a Run value
and StartupApproved flag; neither supplies an elevated interactive token.
Windows does not automatically elevate these login startup commands.
Changing HKCU to HKLM does not solve that mismatch.

Microsoft PowerToys uses Task Scheduler for this same requirement:
https://github.com/microsoft/PowerToys/blob/main/src/runner/auto_start_helper.cpp
Its task uses a user-specific logon trigger, an interactive token, a chosen
run level, no battery restrictions, and unlimited execution time. We reuse
those semantics through the existing `windows` binding rather than adding a
service, launcher, stored credentials, shell commands or a new dependency.

## Decision

- Register one task per environment and account, only on an explicit enable
  command. The account comes from the locally connected Task Scheduler service,
  not environment variables. Both principal and trigger specify that account.
- Use HighestAvailable when enabling from an elevated process, LeastPrivilege
  otherwise. Registering an elevated task requires the user to first launch
  BongoCat as administrator; there is no silent UAC bypass.
- Run the current executable with `--run-seconds 0`, only in an interactive
  login session; ignore duplicate instances and do not terminate after the
  scheduler's default time limit or when switching to battery power.
- Let Task Scheduler assign its default restrictive task ACL. Do not grant
  Everyone access or execute as SYSTEM. XML fields are escaped as data.
- COM interfaces and apartment initialization remain on the settings worker's
  calling thread; RAII releases interfaces before balancing initialization.
- Read the task's Enabled flag. Only a genuinely missing task falls back to
  the published Run registration; scheduler access/service errors are errors,
  not "disabled". This fallback preserves existing released startup settings.
- Explicit enable replaces the task definition (including path and privilege)
  before removing the old Run item. Disable deletes the task and old Run item.
  No automatic startup migration or configuration schema change is introduced.

## Verification and Remaining Gate

Host-independent contracts cover the logon account, privilege level, XML
escaping, Unicode paths, battery behavior and process lifetime. Existing opt-in
Development startup lifecycle smoke covers enable/disable and environment
isolation on Windows; it is not a real login/UAC test.

On Windows, launch an installed Production build as administrator, turn login
startup off and on, inspect the task principal/trigger/action, then sign out
and back in. Confirm startup without a UAC dialog and administrator capability
in diagnostics; repeat on battery, then disable and confirm no startup.
Also check an ordinary non-elevated registration, Task Scheduler disable,
application paths containing spaces/Unicode, and another local user's session.
Actual Windows login/UAC behavior remains unverified on the macOS host.
No user data needs deleting; existing users must toggle login startup once.
