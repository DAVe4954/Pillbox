# Builds dist\Pillbox-Setup-<version>.exe. Run from anywhere; needs Inno Setup 6 (winget install JRSoftware.InnoSetup).
# Not 'Stop': cargo and Inno Setup write progress to stderr, which Stop would treat as a failure.
# Failures are caught through the exit codes below.
$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $root
try {
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    # The page the installer asks people to accept: the licence, then the terms of use.
    $eula = (Get-Content -Raw -Encoding UTF8 "$root\LICENSE") + "`r`n`r`n" + (Get-Content -Raw -Encoding UTF8 "$root\TERMS.md")
    $eula = $eula -replace "(?<!`r)`n", "`r`n"
    [IO.File]::WriteAllText("$PSScriptRoot\EULA.txt", $eula, (New-Object Text.UTF8Encoding $true))
    $iscc = @(
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $iscc) { throw 'Inno Setup 6 not found: winget install JRSoftware.InnoSetup' }
    & $iscc "$PSScriptRoot\pillbox.iss"
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
    Get-Item "$root\dist\Pillbox-Setup-*.exe" | Select-Object Name, Length
} finally {
    Pop-Location
}
