#!/usr/bin/env pwsh
# CI test signing for Windows artifacts (Authenticode).
# Uses a self-signed certificate generated during CI for smoke testing only.
#
# This script is for CI smoke tests. It does NOT sign release artifacts.
# Production signing is BLOCKED pending human decision on key storage/CA.
#
# Usage: pwsh signing/ci/test-sign.ps1 <artifact-path>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ArtifactPath
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $ArtifactPath)) {
    Write-Error "Artifact not found: $ArtifactPath"
    exit 1
}

# Check if SIGN_RELEASE is set — hard stop if someone tries to sign for real
if ($env:SIGN_RELEASE -eq 'true') {
    Write-Error "SIGN_RELEASE=true is not allowed in CI test signing. Production signing is BLOCKED."
    exit 1
}

$certDir = Join-Path $env:RUNNER_TEMP 'test-certs'
if (-not (Test-Path $certDir)) {
    New-Item -ItemType Directory -Path $certDir -Force | Out-Null
}

$certPath = Join-Path $certDir 'openstream-test.pfx'

if (-not (Test-Path $certPath)) {
    Write-Host "Generating self-signed test certificate..."
    $cert = New-SelfSignedCertificate `
        -Subject 'CN=OpenStream Test Signing' `
        -Type CodeSigningCert `
        -CertStoreLocation Cert:\CurrentUser\My `
        -NotAfter (Get-Date).AddHours(24) `
        -HashAlgorithm SHA256

    $password = ConvertTo-SecureString -String 'test-password' -Force -AsPlainText
    Export-PfxCertificate -Cert $cert -FilePath $certPath -Password $password | Out-Null
    Write-Host "Test certificate generated: $certPath"
}

# Import osslsigncode if available, otherwise use signtool
if (Get-Command osslsigncode -ErrorAction SilentlyContinue) {
    Write-Host "Signing with osslsigncode..."
    $certPassword = 'test-password'
    & osslsigncode sign `
        -certs $certPath `
        -pass "$certPassword" `
        -h 256 `
        -in $ArtifactPath `
        -out "$ArtifactPath.signed"

    Move-Item "$ArtifactPath.signed" $ArtifactPath -Force
    Write-Host "Signed: $ArtifactPath"
} elseif (Get-Command signtool -ErrorAction SilentlyContinue) {
    Write-Host "Signing with signtool..."
    & signtool sign `
        /f $certPath `
        /p 'test-password' `
        /fd SHA256 `
        /tr http://timestamp.digicert.com `
        /td SHA256 `
        $ArtifactPath
    Write-Host "Signed: $ArtifactPath"
} else {
    Write-Warning "No signing tool available (osslsigncode or signtool). Skipping sign."
    Write-Warning "Artifact is unsigned: $ArtifactPath"
    exit 0
}

Write-Host "Test signing complete for: $ArtifactPath"
Write-Host "NOTE: This is a CI test signature, NOT a production signature."
