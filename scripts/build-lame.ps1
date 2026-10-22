# Builds lame_enc.dll (LAME 3.100, LGPL-2.0-or-later) for the MP3 export (EXP-005, LIC-006).
#
# LAME is not linked into Xirius (MIT OR Apache-2.0) and is not a cargo dependency: the app
# loads this DLL at run time, and the DLL is shipped next to the executable with its licence
# and where its source came from (see assets/lame/README.md).
#
# What it does:
#   1. Downloads lame-3.100.tar.gz from SourceForge (the official release) and checks its SHA-256.
#   2. Compiles libmp3lame and mpglib with MSVC (cl), no assembler, /MT (no VC runtime needed).
#   3. Links lame_enc.dll with the export list LAME ships (include/lame.def, the lame_* API).
#   4. Copies the DLL and LAME's COPYING to target/lame/<arch>/, and next to the executables in
#      target/debug and target/release, where the app and the tests look for it.
#
# Usage (from the repository root, PowerShell):
#   powershell -ExecutionPolicy Bypass -File scripts/build-lame.ps1            # x64
#   powershell -ExecutionPolicy Bypass -File scripts/build-lame.ps1 -Arch arm64
#
# Needs: Visual Studio 2022 (or Build Tools) with the C++ workload. Nothing else.
# This file is ASCII only: Windows PowerShell 5.1 reads a UTF-8 file without a BOM as ANSI.

param(
    [ValidateSet('x64', 'arm64')]
    [string]$Arch = 'x64',
    # Where the download and the build go (inside target/, so cargo clean removes it).
    [string]$Work = '',
    # Skip copying the DLL into target/debug and target/release.
    [switch]$NoInstall
)

$ErrorActionPreference = 'Stop'

$Version = '3.100'
$Url = "https://downloads.sourceforge.net/project/lame/lame/$Version/lame-$Version.tar.gz"
$Sha256 = 'ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e'

$Root = Split-Path -Parent $PSScriptRoot
if ($Work -eq '') { $Work = Join-Path $Root 'target\lame' }
$Out = Join-Path $Work $Arch
New-Item -ItemType Directory -Force $Work | Out-Null
New-Item -ItemType Directory -Force $Out | Out-Null

# 1. Source
$Tarball = Join-Path $Work "lame-$Version.tar.gz"
if (-not (Test-Path $Tarball)) {
    Write-Host "downloading $Url"
    & curl.exe -sSL --fail -o $Tarball $Url
    if ($LASTEXITCODE -ne 0) { throw "download failed: $Url" }
}
# .NET rather than Get-FileHash, which is missing when PSModulePath comes from PowerShell 7.
$Sha = [System.Security.Cryptography.SHA256]::Create()
$Stream = [System.IO.File]::OpenRead($Tarball)
try { $Got = ([System.BitConverter]::ToString($Sha.ComputeHash($Stream)) -replace '-', '').ToLower() }
finally { $Stream.Dispose(); $Sha.Dispose() }
if ($Got -ne $Sha256) {
    Remove-Item $Tarball
    throw "SHA-256 mismatch for lame-$Version.tar.gz: got $Got, want $Sha256"
}
$Src = Join-Path $Work "lame-$Version"
if (-not (Test-Path $Src)) {
    # The tar of Windows (bsdtar) reads C:\ paths; GNU tar in Git Bash does not.
    & "$env:SystemRoot\System32\tar.exe" -xzf $Tarball -C $Work
    if ($LASTEXITCODE -ne 0) { throw 'extract failed' }
}
Copy-Item (Join-Path $Src 'configMS.h') (Join-Path $Src 'config.h') -Force

# 2. Compiler
$VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $VsWhere)) { throw 'vswhere.exe not found: install Visual Studio 2022 or its Build Tools (C++)' }
$Vs = & $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $Vs) { throw 'MSVC (C++ build tools) not found' }
$VcVars = Join-Path $Vs 'VC\Auxiliary\Build\vcvarsall.bat'
$Host_ = if ($Arch -eq 'arm64') { 'x64_arm64' } else { 'x64' }

$Lib = @('bitstream', 'encoder', 'fft', 'gain_analysis', 'id3tag', 'lame', 'newmdct', 'psymodel',
    'quantize', 'quantize_pvt', 'set_get', 'vbrquantize', 'reservoir', 'tables', 'takehiro', 'util',
    'mpglib_interface', 'VbrTag', 'presets', 'version') | ForEach-Object { "libmp3lame\$_.c" }
if ($Arch -eq 'x64') { $Lib += 'libmp3lame\vector\xmm_quantize_sub.c' }
$Hip = @('common', 'dct64_i386', 'decode_i386', 'layer1', 'layer2', 'layer3', 'tabinit', 'interface') |
    ForEach-Object { "mpglib\$_.c" }

# The export list LAME ships, renamed to the DLL it becomes.
$Def = Join-Path $Src 'lame_enc.def'
$Lines = Get-Content (Join-Path $Src 'include\lame.def') | Where-Object { $_ -notmatch '^\s*LIBRARY' }
Set-Content -Encoding ASCII $Def (@('LIBRARY lame_enc.dll') + $Lines)

$Obj = Join-Path $Src "obj-$Arch"
New-Item -ItemType Directory -Force $Obj | Out-Null
$Flags = '/nologo /O2 /MT /W0 /DNDEBUG /DHAVE_CONFIG_H /DHAVE_MPGLIB /DBS_FORMAT=BINARY /DFLOAT8=float /DREAL_IS_FLOAT=1 /D_CRT_SECURE_NO_WARNINGS -I. -Iinclude -Ilibmp3lame -Impglib'
$Cmd = Join-Path $Src "build-$Arch.cmd"
@(
    '@echo off',
    "call `"$VcVars`" $Host_ >nul || exit /b 1",
    "cd /d `"$Src`"",
    "cl $Flags /Fo`"$Obj\\`" /c $($Lib -join ' ') $($Hip -join ' ') || exit /b 1",
    "rc /nologo /d NDEBUG /d _DLL=lame_enc.dll /l 0x409 /fo `"$Obj\lame_enc.res`" libmp3lame\lame.rc || exit /b 1",
    "link /nologo /DLL /DEF:`"$Def`" /OUT:`"$Out\lame_enc.dll`" `"$Obj\*.obj`" `"$Obj\lame_enc.res`" || exit /b 1"
) | Set-Content -Encoding ASCII $Cmd
& cmd.exe /c "`"$Cmd`""
if ($LASTEXITCODE -ne 0) { throw "build failed (see the output above; the commands are in $Cmd)" }

# 3. Licence and where the source came from, next to the DLL.
Copy-Item (Join-Path $Src 'COPYING') (Join-Path $Out 'LAME-COPYING.txt') -Force
@(
    "LAME $Version, built by scripts/build-lame.ps1 from the official release:",
    "  $Url",
    "  SHA-256 $Sha256",
    'Licence: GNU Library General Public License 2.0 or later (LAME-COPYING.txt).',
    'Not modified. Replace lame_enc.dll with another build of the same name to use that instead.'
) | Set-Content -Encoding ASCII (Join-Path $Out 'LAME-SOURCE.txt')

Write-Host "built $Out\lame_enc.dll"

# 4. Next to the executables (the app looks beside itself; tests run from target/<profile>/deps).
if (-not $NoInstall -and $Arch -eq 'x64') {
    foreach ($Prof in @('debug', 'release')) {
        $Dir = Join-Path $Root "target\$Prof"
        New-Item -ItemType Directory -Force $Dir | Out-Null
        foreach ($F in @('lame_enc.dll', 'LAME-COPYING.txt', 'LAME-SOURCE.txt')) {
            Copy-Item (Join-Path $Out $F) (Join-Path $Dir $F) -Force
        }
        Write-Host "copied to $Dir"
    }
}
