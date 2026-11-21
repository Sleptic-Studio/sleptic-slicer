@echo off
REM Copyright (c) 2026 Sleptic
REM SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
rem Run cargo with the MSVC build environment loaded (Windows).
rem Usage: scripts\cargo-msvc.bat test --workspace --offline
for %%E in (Community Professional Enterprise BuildTools) do (
  if exist "%ProgramFiles%\Microsoft Visual Studio\2022\%%E\VC\Auxiliary\Build\vcvars64.bat" (
    call "%ProgramFiles%\Microsoft Visual Studio\2022\%%E\VC\Auxiliary\Build\vcvars64.bat" >nul
    goto run
  )
)
echo Visual Studio 2022 C++ build tools not found; running cargo without them.
:run
cd /d "%~dp0.."
cargo %*
