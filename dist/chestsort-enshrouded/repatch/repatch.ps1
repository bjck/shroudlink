# Builds the chest/stack data files for the CURRENT Enshrouded server build.
# Downloads the dedicated server (~8 GB) with SteamCMD, applies the two EML mods
# and puts everything to upload into ..\upload-build-<build id>.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$server = Join-Path $here 'server'
$steamcmd = Join-Path $here 'steamcmd\steamcmd.exe'

if (-not (Test-Path $steamcmd)) {
    Write-Host 'Downloading SteamCMD...'
    $zip = Join-Path $here 'steamcmd.zip'
    Invoke-WebRequest 'https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip' -OutFile $zip
    Expand-Archive $zip (Join-Path $here 'steamcmd') -Force
    Remove-Item $zip
}

Write-Host 'Downloading / updating the Enshrouded dedicated server (first run ~8 GB)...'
& $steamcmd +force_install_dir $server +login anonymous +app_update 2278520 validate +quit
if (-not (Test-Path (Join-Path $server 'enshrouded_server.kfc'))) { throw 'server download failed, run again' }

# a backup from an older build must not be used as the patch source
Remove-Item (Join-Path $server 'enshrouded_server.kfc.bak') -ErrorAction SilentlyContinue
Copy-Item (Join-Path $here 'mods') $server -Recurse -Force

Write-Host 'Applying Bigger Storage + Bigger Stacks...'
& (Join-Path $here 'emm.exe') run -g $server --patch
if ($LASTEXITCODE -ne 0) { throw 'patching failed (see the messages above)' }

$manifest = Get-Content (Join-Path $server 'steamapps\appmanifest_2278520.acf') -Raw
$build = [regex]::Match($manifest, '"buildid"\s+"(\d+)"').Groups[1].Value
$out = Join-Path (Split-Path $here) "upload-build-$build"
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item (Join-Path $server 'enshrouded_server.kfc'), (Join-Path $server 'enshrouded_server.kfc_resources') $out -Force
# empty data containers the patched file expects
Get-ChildItem $server -Filter 'enshrouded_server_*.dat' | Where-Object Length -eq 0 | Copy-Item -Destination $out -Force
Copy-Item (Join-Path $here 'steam_api64.dll') $out -Force

Write-Host ''
Write-Host "Done. Upload the files in $out (see README.txt)."
