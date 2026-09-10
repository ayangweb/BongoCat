[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$InputDirectory,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputFile,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$')]
    [string]$ProductVersion,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$NsisSetupPath,

    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$MakeNsisPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$expectedNsisSetupName = 'nsis-3.11-setup.exe'
$expectedNsisSetupMd5 = '700dc40097d4cd226b13212dda1d33ac'
$expectedNsisVersion = 'v3.11'
$expectedTarget = 'x86_64-pc-windows-msvc'
$expectedExecutable = 'bongocat-app.exe'
$requiredModels = @('standard', 'keyboard', 'gamepad')

function Fail([string]$Message) {
    throw "Windows package validation failed: $Message"
}

function Resolve-ExistingDirectory([string]$Path, [string]$Name) {
    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    if (-not (Test-Path -LiteralPath $resolved -PathType Container)) {
        Fail "$Name must be a directory"
    }
    return $resolved.Path
}

function Resolve-ExistingFile([string]$Path, [string]$Name) {
    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
        Fail "$Name must be a file"
    }
    return $resolved.Path
}

if ($env:BONGOCAT_BUILD_ENV -notin @('development', 'production')) {
    Fail 'BONGOCAT_BUILD_ENV must be explicitly set to development or production'
}

$inputDirectoryItem = Get-Item -LiteralPath $InputDirectory -Force
if (($inputDirectoryItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
    Fail 'InputDirectory itself must not be a reparse point'
}
$payloadDirectory = Resolve-ExistingDirectory $InputDirectory 'InputDirectory'
$setupPath = Resolve-ExistingFile $NsisSetupPath 'NsisSetupPath'
$makeNsis = Resolve-ExistingFile $MakeNsisPath 'MakeNsisPath'
$outputParent = Split-Path -Parent $OutputFile
if ([string]::IsNullOrWhiteSpace($outputParent) -or -not (Test-Path -LiteralPath $outputParent -PathType Container)) {
    Fail 'OutputFile parent directory must already exist'
}
$outputPath = Join-Path (Resolve-Path -LiteralPath $outputParent).Path (Split-Path -Leaf $OutputFile)
if (Test-Path -LiteralPath $outputPath) {
    Fail 'OutputFile must not already exist'
}

if ((Split-Path -Leaf $setupPath) -ne $expectedNsisSetupName) {
    Fail "NsisSetupPath must be the official $expectedNsisSetupName artifact"
}
$setupMd5 = (Get-FileHash -LiteralPath $setupPath -Algorithm MD5).Hash.ToLowerInvariant()
if ($setupMd5 -ne $expectedNsisSetupMd5) {
    Fail 'NsisSetupPath does not match the pinned NSIS 3.11 MD5'
}
$actualNsisVersion = (& $makeNsis '/VERSION').Trim()
if ($LASTEXITCODE -ne 0 -or $actualNsisVersion -ne $expectedNsisVersion) {
    Fail "MakeNSIS must report $expectedNsisVersion"
}

$payloadItems = @(Get-Item -LiteralPath $payloadDirectory -Force)
$payloadItems += Get-ChildItem -LiteralPath $payloadDirectory -Force -Recurse
foreach ($item in $payloadItems) {
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        Fail "InputDirectory contains a reparse point: $($item.FullName)"
    }
}

$executablePath = Join-Path $payloadDirectory $expectedExecutable
if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
    Fail "InputDirectory is missing $expectedExecutable"
}
if (Test-Path -LiteralPath (Join-Path $payloadDirectory 'Uninstall.exe')) {
    Fail 'InputDirectory must not contain Uninstall.exe'
}
foreach ($model in $requiredModels) {
    if (-not (Test-Path -LiteralPath (Join-Path $payloadDirectory "resources\models\$model") -PathType Container)) {
        Fail "InputDirectory is missing preset model: $model"
    }
}

$provenancePath = Join-Path $payloadDirectory 'resources\build-provenance.json'
if (-not (Test-Path -LiteralPath $provenancePath -PathType Leaf)) {
    Fail 'InputDirectory is missing resources/build-provenance.json'
}
try {
    $provenance = Get-Content -LiteralPath $provenancePath -Raw | ConvertFrom-Json
} catch {
    Fail 'resources/build-provenance.json is not valid JSON'
}
if ($provenance.schema_version -ne 1 -or
    $provenance.target -ne $expectedTarget -or
    $provenance.profile -ne 'release' -or
    $provenance.build_environment -ne $env:BONGOCAT_BUILD_ENV) {
    Fail 'resources/build-provenance.json does not match the x64 release payload and build environment'
}

$signedFiles = Get-ChildItem -LiteralPath $payloadDirectory -File -Recurse |
    Where-Object { $_.Extension -in @('.exe', '.dll') }
if ($signedFiles.Count -eq 0) {
    Fail 'InputDirectory does not contain a signed Windows executable or DLL'
}
foreach ($signedFile in $signedFiles) {
    $signature = Get-AuthenticodeSignature -LiteralPath $signedFile.FullName
    if ($signature.Status -ne 'Valid') {
        Fail "product file does not have a valid Authenticode signature: $($signedFile.Name)"
    }
}

$scriptPath = Join-Path $PSScriptRoot '..\windows\installer\BongoCat.nsi'
$scriptPath = Resolve-ExistingFile $scriptPath 'BongoCat.nsi'
$makeNsisArguments = @(
    "/DINPUT_DIRECTORY=$payloadDirectory",
    "/DOUTPUT_FILE=$outputPath",
    "/DPRODUCT_VERSION=$ProductVersion",
    $scriptPath
)
& $makeNsis @makeNsisArguments
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $outputPath -PathType Leaf)) {
    Fail 'MakeNSIS did not create the requested installer artifact'
}

Write-Output $outputPath
