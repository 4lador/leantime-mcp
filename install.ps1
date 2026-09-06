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
        { $_ -in "arm64", "aarch64" } {
            Write-Err "Windows ARM64 build not published yet — open an issue if you need it: https://github.com/4lador/leantime-mcp/issues"
        }
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
# Download to a temp file first: writing directly over a running executable
# fails on Windows (the file is locked while the MCP server is running).
$tmpDest = "$dest.tmp"
try {
    Invoke-WebRequest -Uri $url -OutFile $tmpDest
} catch {
    if (Test-Path $tmpDest) { Remove-Item -Force $tmpDest }
    Write-Err "Download failed: $_"
}

# Integrity: verify the published SHA-256 before installing anything.
$tmpSha = "$dest.sha256.tmp"
try {
    Invoke-WebRequest -Uri "$url.sha256" -OutFile $tmpSha
} catch {
    if (Test-Path $tmpDest) { Remove-Item -Force $tmpDest }
    if (Test-Path $tmpSha) { Remove-Item -Force $tmpSha }
    Write-Err "Could not download the checksum ($url.sha256)"
}
$expected = (Get-Content $tmpSha | Select-Object -First 1) -split '\s+' | Select-Object -First 1
Remove-Item -Force $tmpSha
if (-not $expected) {
    if (Test-Path $tmpDest) { Remove-Item -Force $tmpDest }
    Write-Err "Checksum file is empty"
}
$actual = (Get-FileHash -Algorithm SHA256 $tmpDest).Hash.ToLower()
if ($actual -ne $expected.ToLower()) {
    Remove-Item -Force $tmpDest
    Write-Err "Checksum mismatch (expected $expected, got $actual) - download corrupted, aborted"
}

try {
    Move-Item -Force -Path $tmpDest -Destination $dest
} catch {
    if (Test-Path $tmpDest) { Remove-Item -Force $tmpDest }
    Write-Err "Install failed: $_"
}

Write-Info "Installed $BinaryName $version to $dest"

$pathDir = [System.Environment]::GetEnvironmentVariable("Path", "User")
if ($pathDir -notlike "*$InstallDir*") {
    Write-Warn "Add $InstallDir to your PATH:"
    Write-Host ""
    Write-Host "  [System.Environment]::SetEnvironmentVariable('Path', [System.Environment]::GetEnvironmentVariable('Path', 'User') + ';$InstallDir', 'User')"
    Write-Host ""
}

Write-Info "Run '$BinaryName setup global' to configure for opencode"
