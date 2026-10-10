# Builds KrunkerRevival's browser client into client\dist, where
# config\server.toml points `client_dir`. Nothing it writes is committed.
#
#   scripts\build-client.ps1 [-Source URL_OR_PATH] [-ResZip FILE]
#
# -Source  KRP repository to clone: a URL, or the archive's mirror
#          (vertix-preservation\mirrors\KrunkerRevivalProject-vertix.git).
#          Default: the public KRP repository.
# -ResZip  A res.zip to serve instead of KRP's (for example the 2020
#          capture from the archive).
#
# Needs git, Node.js and pnpm. The commit is pinned so the client matches
# the server code ported from it (data\krp records the same commit).
param(
  [string]$Source = "https://github.com/KrunkerRevivalProject/vertix.git",
  [string]$ResZip = ""
)
$ErrorActionPreference = "Stop"

$KrpCommit = "1e302cbc3015a648aeffb90517ec683ae19ed4df"
$Root = Split-Path -Parent $PSScriptRoot
$Checkout = Join-Path $Root "client\krp"
$Dist = Join-Path $Root "client\dist"

foreach ($tool in "git", "node", "pnpm") {
  if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "$tool is required" }
}

if (-not (Test-Path (Join-Path $Checkout ".git"))) {
  git clone --no-checkout $Source $Checkout
  if ($LASTEXITCODE) { throw "git clone failed" }
}
git -C $Checkout fetch --quiet $Source $KrpCommit 2>$null
git -C $Checkout checkout --quiet --detach $KrpCommit
if ($LASTEXITCODE) { throw "commit $KrpCommit not found in $Source" }

# Our patches to the pinned source (scripts\client-patches\apply.mjs).
node (Join-Path $PSScriptRoot "client-patches\apply.mjs") $Checkout
if ($LASTEXITCODE) { throw "client patches failed" }

Push-Location $Checkout
try {
  pnpm install --frozen-lockfile
  if ($LASTEXITCODE) { throw "pnpm install failed" }
  pnpm --filter core build
  if ($LASTEXITCODE) { throw "client build failed" }
} finally {
  Pop-Location
}

if (Test-Path $Dist) { Remove-Item -Recurse -Force $Dist }
Copy-Item -Recurse (Join-Path $Checkout "core\dist") $Dist
# Hats, shirts and sprays are loaded by path at run time, not bundled.
$Assets = Join-Path $Dist "assets"
New-Item -ItemType Directory -Force $Assets | Out-Null
foreach ($d in "hats", "shirts", "sprays") {
  Copy-Item -Recurse -Force (Join-Path $Checkout "core\assets\$d") $Assets
}
if ($ResZip) { Copy-Item -Force $ResZip (Join-Path $Dist "res.zip") }
Write-Host "client built from KRP $KrpCommit into $Dist"
