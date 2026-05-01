$ErrorActionPreference = "Stop"

$Repo = "4lador/leantime-mcp"
$BinaryName = "leantmcp"
$InstallDir = Join-Path $env:USERPROFILE ".local\bin"

function Write-Info($msg)  { Write-Host -ForegroundColor Green "-> $msg" }
function Write-Warn($msg)  { Write-Host -ForegroundColor Yellow "! $msg" }
function Write-Err($msg)   { Write-Host -ForegroundColor Red "X $msg"; exit 1 }

function Get-Arch {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
    switch ($arch) {
        { $_ -in "x64", "amd64" } { return "x86_64" }
        { $_ -in "arm64", "aarch64" } { return "aarch64" }
        default { Write-Err "Unsupported architecture: $arch" }
    }
}

function Get-LatestVersion {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "leantmcp-installer" }
    return $release.tag_name
}

$arch = Get-Arch

Write-Info "Detecting latest version..."
$version = Get-LatestVersion
if (-not $version) { Write-Err "Could not determine latest version" }
Write-Info "Latest version: $version"

$artifactName = "$BinaryName-windows-$arch.exe"
$url = "https://github.com/$Repo/releases/download/$version/$artifactName"
$dest = Join-Path $InstallDir "$BinaryName.exe"

Write-Info "Downloading $url..."
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Invoke-WebRequest -Uri $url -OutFile $dest

Write-Info "Installed $BinaryName $version to $dest"

$pathDir = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($pathDir -notlike "*$InstallDir*") {
    Write-Warn "Add $InstallDir to your PATH:"
    Write-Host ""
    Write-Host "  [System.Environment]::SetEnvironmentVariable('Path', [System.Environment]::GetEnvironmentVariable('Path', 'User') + ';$InstallDir', 'User')"
    Write-Host ""
}

Write-Info "Run '$BinaryName setup global' to configure for opencode"
