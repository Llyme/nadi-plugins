# Builds every plugin: each becomes a folder in dist/ (its manifest.json, its Luau scripts, its pictures; a face is its manifest), the folder NADI installs
# from a release (`nadi-plugins.tar.gz` holds these folders) or that you drop in %APPDATA%/NADI/plugins.
#
# A plugin with a test.luau is tested first, when `nadi-plugin` (the test runner that comes with NADI: native/crates/nadi-core) is
# on the PATH or named by $env:NADI_PLUGIN.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $here
try {
  $runner = $env:NADI_PLUGIN
  if (-not $runner) { $found = Get-Command nadi-plugin -ErrorAction SilentlyContinue; if ($found) { $runner = $found.Source } }
  $dist = Join-Path $here "dist"
  if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
  foreach ($dir in Get-ChildItem $here -Directory | Where-Object { Test-Path (Join-Path $_.FullName "manifest.json") }) {
    $manifest = Get-Content (Join-Path $dir.FullName "manifest.json") -Raw | ConvertFrom-Json
    # (only what this NADI runs: Luau plugins)
    if ($manifest.kind -ne "luau" -and $manifest.kind -ne "face") { "skip {0} ({1})" -f $dir.Name, $manifest.kind; continue }
    if ($runner -and (Test-Path (Join-Path $dir.FullName "test.luau"))) {
      & $runner test $dir.FullName
      if ($LASTEXITCODE -ne 0) { throw "$($dir.Name): tests failed" }
    }
    $to = Join-Path $dist $dir.Name
    New-Item -ItemType Directory -Force $to | Out-Null
    Copy-Item (Join-Path $dir.FullName "manifest.json") $to
    # (its scripts, not its tests; and its pictures)
    Get-ChildItem $dir.FullName -File | Where-Object { ($_.Extension -eq ".luau" -and $_.Name -ne "test.luau") -or $_.Extension -in ".gif", ".png", ".webp", ".jpg", ".jpeg", ".svg" } | ForEach-Object { Copy-Item $_.FullName $to }
    Get-ChildItem $to | ForEach-Object { "dist/{0}/{1}  {2:N0} bytes" -f $dir.Name, $_.Name, $_.Length }
  }
  # the file a release carries
  # (the tar that comes with Windows: another one first on the PATH, Git's, cannot take a drive letter)
  & "$env:SystemRoot\System32\tar.exe" -czf (Join-Path $here "nadi-plugins.tar.gz") -C $dist .
  "nadi-plugins.tar.gz"
} finally { Pop-Location }
