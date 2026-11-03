# Verify the actual PE bytes, including timestamps, on a clean Windows runner.
# No certificate is installed or trusted by this script. Hosted Windows CI is
# a packaging/signature check; it does not establish SAC-On enforcement.
param([Parameter(Mandatory = $true)][string]$Root)

$ErrorActionPreference = "Stop"
$nativeFiles = @(Get-ChildItem -LiteralPath $Root -Recurse -File |
    Where-Object { $_.Extension.ToLowerInvariant() -in @(".pyd", ".dll", ".exe") })
if ($nativeFiles.Count -eq 0) {
    throw "No native files found under $Root"
}
foreach ($file in $nativeFiles) {
    $signature = Get-AuthenticodeSignature -LiteralPath $file.FullName
    if ($signature.Status -ne "Valid" -or $signature.SignatureType -ne "Authenticode") {
        throw "Invalid embedded Authenticode signature on $($file.FullName): $($signature.Status)"
    }
    if ($null -eq $signature.SignerCertificate -or $null -eq $signature.TimeStamperCertificate) {
        throw "Missing signer or timestamp on $($file.FullName)"
    }
    Write-Output "$($file.Name): Valid; publisher=$($signature.SignerCertificate.Subject); timestamped"
}
