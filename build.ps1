# Builds every plugin: each becomes a folder in dist/ (its manifest.json, and its .wasm when it is a module), the folder
# NADI installs from a release (`nadi-plugins.tar.gz` holds these folders) or that you drop in %APPDATA%/NADI/plugins.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Push-Location $here
try {
  rustup target add wasm32-unknown-unknown | Out-Null
  cargo build --release --target wasm32-unknown-unknown
  $out = Join-Path $here "target/wasm32-unknown-unknown/release"
  $dist = Join-Path $here "dist"
  if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
  # every folder with a manifest.json is a plugin; a module has a Cargo.toml and builds to <crate>.wasm
  foreach ($dir in Get-ChildItem $here -Directory | Where-Object { Test-Path (Join-Path $_.FullName "manifest.json") }) {
    $to = Join-Path $dist $dir.Name
    New-Item -ItemType Directory -Force $to | Out-Null
    Copy-Item (Join-Path $dir.FullName "manifest.json") $to
    # (the scripts of its faces)
    Get-ChildItem $dir.FullName -Filter *.js -File | ForEach-Object { Copy-Item $_.FullName $to }
    $manifest = Get-Content (Join-Path $dir.FullName "manifest.json") -Raw | ConvertFrom-Json
    if ($manifest.kind -eq "wasm") {
      $built = Join-Path $out (($dir.Name -replace "-", "_") + ".wasm")
      Copy-Item $built (Join-Path $to $manifest.module) -Force
    }
    # (a bundle: one plugin made of modules, each built from the crate its part names)
    if ($manifest.kind -eq "bundle") {
      foreach ($part in $manifest.parts) {
        $built = Join-Path $out (($part.crate -replace "-", "_") + ".wasm")
        Copy-Item $built (Join-Path $to $part.module) -Force
      }
    }
    Get-ChildItem $to | ForEach-Object { "dist/{0}/{1}  {2:N0} bytes" -f $dir.Name, $_.Name, $_.Length }
  }
  # the file a release carries
  # (the tar that comes with Windows: another one first on the PATH, Git's, cannot take a drive letter)
  & "$env:SystemRoot\System32\tar.exe" -czf (Join-Path $here "nadi-plugins.tar.gz") -C $dist .
  "nadi-plugins.tar.gz"
} finally { Pop-Location }
