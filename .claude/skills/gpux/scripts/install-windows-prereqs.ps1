<#
.SYNOPSIS
Checks or installs Windows prerequisites for the gpux workspace.

.NOTES
Run from the Gpux repository root. This helper lives at
.claude\skills\gpux\scripts\install-windows-prereqs.ps1 and supports the
gpux workspace rooted at the repository root.
#>
[CmdletBinding()]
param(
    [switch]$CheckOnly,
    [switch]$InstallBuildTools,
    [switch]$InstallVCRedist,
    [switch]$InstallLLVM,
    [switch]$InstallWasmBindgen,
    [switch]$AddCargoBinToPath,
    [switch]$AddFxcToPath,
    [switch]$All,
    [string]$WasmBindgenVersion = "0.2.129"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($All) {
    $InstallBuildTools = $true
    $InstallVCRedist = $true
    $InstallLLVM = $true
    $InstallWasmBindgen = $true
    $AddCargoBinToPath = $true
    $AddFxcToPath = $true
}

if (-not ($CheckOnly -or $InstallBuildTools -or $InstallVCRedist -or $InstallLLVM -or $InstallWasmBindgen -or $AddCargoBinToPath -or $AddFxcToPath)) {
    $CheckOnly = $true
}

function Test-CommandAvailable {
    param([Parameter(Mandatory)][string]$Name)
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Write-Section {
    param([Parameter(Mandatory)][string]$Text)
    Write-Host ""
    Write-Host "== $Text =="
}

function Add-UserPathEntry {
    param([Parameter(Mandatory)][string]$PathEntry)

    if (-not (Test-Path -LiteralPath $PathEntry)) {
        Write-Warning "PATH entry does not exist: $PathEntry"
        return
    }

    $current = [Environment]::GetEnvironmentVariable("PATH", "User")
    $entries = @()
    if ($current) {
        $entries = $current -split ";"
    }

    if ($entries -contains $PathEntry) {
        Write-Host "PATH already contains: $PathEntry"
        return
    }

    $newPath = if ($current) { "$PathEntry;$current" } else { $PathEntry }
    [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
    Write-Host "Added to user PATH: $PathEntry"
}

function Invoke-Winget {
    param([Parameter(Mandatory)][string[]]$Arguments)

    if (-not (Test-CommandAvailable "winget")) {
        throw "winget is not available. Install App Installer or run the command manually."
    }

    Write-Host "winget $($Arguments -join ' ')"
    & winget @Arguments
}

function Get-HostArchitecture {
    return [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
}

function Find-FxcPath {
    $root = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path -LiteralPath $root)) {
        return $null
    }

    $arch = Get-HostArchitecture
    $preferredArch = if ($arch -eq "arm64") { "arm64" } else { "x64" }
    $fallbackArch = if ($preferredArch -eq "arm64") { "x64" } else { "arm64" }

    $candidates = Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
        ForEach-Object {
            foreach ($candidateArch in @($preferredArch, $fallbackArch)) {
                $fxc = Join-Path $_.FullName "$candidateArch\fxc.exe"
                if (Test-Path -LiteralPath $fxc) {
                    [pscustomobject]@{
                        Version = $_.Name
                        Path = $fxc
                    }
                }
            }
        } |
        Sort-Object -Property Version -Descending

    if ($candidates) {
        return $candidates[0].Path
    }

    return $null
}

function Show-ToolStatus {
    Write-Section "Host"
    Write-Host "OS architecture: $(Get-HostArchitecture)"
    Write-Host "PowerShell: $($PSVersionTable.PSVersion)"

    Write-Section "Tools"
    foreach ($tool in @("winget", "cargo", "rustc", "fxc", "clang", "wasm-bindgen")) {
        $available = Test-CommandAvailable $tool
        Write-Host ("{0,-14} {1}" -f $tool, $(if ($available) { "found" } else { "missing" }))
    }

    $fxcPath = Find-FxcPath
    if ($fxcPath) {
        Write-Host "Detected fxc.exe: $fxcPath"
    } else {
        Write-Warning "No Windows SDK fxc.exe found under the standard Windows Kits path."
    }
}

Show-ToolStatus

if ($CheckOnly) {
    Write-Section "Check only"
    Write-Host "No installers or PATH changes were run."
    exit 0
}

if ($InstallBuildTools) {
    Write-Section "Visual Studio 2022 Build Tools"
    Invoke-Winget @(
        "install",
        "Microsoft.VisualStudio.2022.BuildTools",
        "--override",
        "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    )
}

if ($InstallVCRedist) {
    Write-Section "Microsoft Visual C++ Redistributables"
    $arch = Get-HostArchitecture
    if ($arch -eq "arm64") {
        Invoke-Winget @("install", "--id", "Microsoft.VCRedist.2015+.arm64", "--exact", "--accept-package-agreements", "--accept-source-agreements")
    }
    Invoke-Winget @("install", "--id", "Microsoft.VCRedist.2015+.x64", "--exact", "--accept-package-agreements", "--accept-source-agreements")
}

if ($InstallLLVM) {
    Write-Section "LLVM/Clang"
    Invoke-Winget @("install", "LLVM.LLVM", "--accept-package-agreements", "--accept-source-agreements")
    Add-UserPathEntry "C:\Program Files\LLVM\bin"
}

if ($InstallWasmBindgen) {
    Write-Section "wasm-bindgen-cli"
    if (-not (Test-CommandAvailable "cargo")) {
        throw "cargo is required before installing wasm-bindgen-cli."
    }
    & cargo install wasm-bindgen-cli --version $WasmBindgenVersion --locked
}

if ($AddCargoBinToPath) {
    Write-Section "Cargo bin PATH"
    Add-UserPathEntry (Join-Path $HOME ".cargo\bin")
}

if ($AddFxcToPath) {
    Write-Section "Windows SDK fxc.exe PATH"
    $fxcPath = Find-FxcPath
    if (-not $fxcPath) {
        throw "Could not find fxc.exe. Install Visual Studio Build Tools with the Windows SDK first."
    }
    Add-UserPathEntry (Split-Path -Parent $fxcPath)
}

Write-Section "Done"
Write-Host "Restart PowerShell after PATH changes, then verify: fxc /help; clang --version; wasm-bindgen --version"
