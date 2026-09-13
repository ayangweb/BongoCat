[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
    throw "Windows build failed: $Message"
}

$scriptDirectory = $PSScriptRoot
$repositoryRoot = (Resolve-Path (Join-Path $scriptDirectory '..')).Path
$workspaceManifest = Join-Path $repositoryRoot 'Cargo.toml'
$target = 'x86_64-pc-windows-msvc'
$workspaceManifestContent = Get-Content -LiteralPath $workspaceManifest -Raw
$workspacePackageMatch = [regex]::Match(
    $workspaceManifestContent,
    '(?ms)^\[workspace\.package\]\s*\r?\n(?<body>.*?)(?=^\[|\z)')
$versionMatch = $null
if ($workspacePackageMatch.Success) {
    $versionMatch = [regex]::Match(
        $workspacePackageMatch.Groups['body'].Value,
        '(?m)^version\s*=\s*"([^"]+)"\s*$')
}
if (-not $workspacePackageMatch.Success -or -not $versionMatch.Success) {
    Fail 'could not read [workspace.package].version from Cargo.toml'
}
$version = $versionMatch.Groups[1].Value

$payloadDirectory = Join-Path $repositoryRoot 'target\package\windows-x64'
$outputFile = Join-Path $repositoryRoot "target\package\BongoCat-$version-x64-setup.exe"
$executable = Join-Path $repositoryRoot "target\$target\release\bongocat-app.exe"
$resourcesDirectory = Join-Path $payloadDirectory 'resources'

$nsisSetupPath = $env:BONGOCAT_NSIS_SETUP_PATH
$makeNsisPath = $env:BONGOCAT_MAKENSIS_PATH
if ([string]::IsNullOrWhiteSpace($nsisSetupPath) -or [string]::IsNullOrWhiteSpace($makeNsisPath)) {
    Fail 'set BONGOCAT_NSIS_SETUP_PATH and BONGOCAT_MAKENSIS_PATH to the pinned NSIS 3.11 artifacts'
}

Write-Host "Building $target release payload..."
& cargo build --locked --manifest-path $workspaceManifest --target $target -p bongocat-app --release
if ($LASTEXITCODE -ne 0) {
    Fail 'Cargo release build failed'
}
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    Fail "release executable was not created: $executable"
}

Remove-Item -LiteralPath $payloadDirectory -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $outputFile -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $resourcesDirectory -Force | Out-Null
Copy-Item -LiteralPath $executable -Destination (Join-Path $payloadDirectory 'bongocat-app.exe')
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'resources\models') -Destination $resourcesDirectory -Recurse

& python (Join-Path $repositoryRoot 'tools\record-native-provenance.py') `
    --workspace $repositoryRoot `
    --output (Join-Path $resourcesDirectory 'build-provenance.json') `
    --target $target `
    --profile release `
    --features default `
    --environment $env:BONGOCAT_BUILD_ENV
if ($LASTEXITCODE -ne 0) {
    Fail 'could not write build provenance'
}

$installer = & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $scriptDirectory 'package-windows.ps1') `
    -InputDirectory $payloadDirectory `
    -OutputFile $outputFile `
    -NsisSetupPath $nsisSetupPath `
    -MakeNsisPath $makeNsisPath
if ($LASTEXITCODE -ne 0) {
    Fail 'NSIS packaging failed'
}

$installer = ($installer | Select-Object -Last 1).Trim()
if ([string]::IsNullOrWhiteSpace($installer) -or -not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    Fail 'NSIS did not return an installer path'
}

Write-Host ''
Write-Host 'Build completed successfully!'
Write-Host ''
Write-Host 'Installer:'
Write-Host "  $((Resolve-Path -LiteralPath $installer).Path)"
