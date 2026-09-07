[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Fail([string]$Message) {
    throw "Windows build failed: $Message"
}

$scriptDirectory = $PSScriptRoot
$nativeDirectory = (Resolve-Path (Join-Path $scriptDirectory '..')).Path
$workspaceManifest = Join-Path $nativeDirectory 'Cargo.toml'
$target = 'x86_64-pc-windows-msvc'
$version = ((Select-String -LiteralPath $workspaceManifest -Pattern '^version = "([^"]+)"$' | Select-Object -First 1).Matches.Groups[1].Value)
if ([string]::IsNullOrWhiteSpace($version)) {
    Fail 'could not read the workspace product version'
}

$payloadDirectory = Join-Path $nativeDirectory 'target\package\windows-x64'
$outputFile = Join-Path $nativeDirectory "target\package\BongoCat-$version-x64-setup.exe"
$executable = Join-Path $nativeDirectory "target\$target\release\bongocat-app.exe"
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
Copy-Item -LiteralPath (Join-Path $nativeDirectory 'resources\models') -Destination $resourcesDirectory -Recurse

& python (Join-Path $nativeDirectory '..\tools\record-native-provenance.py') `
    --workspace $nativeDirectory `
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
    -ProductVersion $version `
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