# Updates the Game Host Bros server to a new Enshrouded build WITHOUT losing items.
#
# Why this is needed: a server that starts with the vanilla data files of a new build
# shrinks every stack above the vanilla limit and deletes the rest (stacks x10, bigger
# chests). So the world must never be loaded before the patched files are in place:
#
#   1. you stop the server                      4. the server updates with NO world loaded
#   2. the savegame is downloaded (backup)      5. the world comes back
#   3. the patched files are built locally      6. patched files + mod DLL are uploaded
#                                               7. you start the server
#
# The script stops and tells you whenever something has to be done in the panel.
# Nothing on the server is deleted: replaced files and folders are renamed.
param(
    # SFTP access from the hosting panel; asked for when not given
    [string]$HostName,
    [int]$Port = 2022,
    [string]$User,
    # local copy of the dedicated server used for patching (~8 GB, reused next time)
    [string]$ServerDir = (Join-Path $PSScriptRoot 'repatch\server')
)
$ErrorActionPreference = 'Stop'
$repatch = Join-Path $PSScriptRoot 'repatch'
$stamp = Get-Date -Format 'yyyyMMdd_HHmmss'

function Step($text) { Write-Host ''; Write-Host "== $text" -ForegroundColor Cyan }
function Info($text) { Write-Host "   $text" }
function Wait-User($text) {
    Write-Host ''
    Write-Host ">> $text" -ForegroundColor Yellow
    Read-Host '   Press Enter when done'
}

# EML's patcher (github.com/Brabb3l/kfc-parser, GPL-3.0) is downloaded, not bundled
function Get-Emm($dir) {
    $emm = Join-Path $dir 'emm.exe'
    $sha = 'B1133F6B8EA7000596CC7710901BD8307B48165B0E650C0487A611D90EA49305'
    if (-not (Test-Path $emm) -or (Get-FileHash $emm).Hash -ne $sha) {
        Write-Host 'Downloading the EML patcher (emm.exe v0.2.0)...'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest 'https://github.com/Brabb3l/kfc-parser/releases/download/v0.2.0/emm.exe' -OutFile $emm
        if ((Get-FileHash $emm).Hash -ne $sha) { Remove-Item $emm; throw 'emm.exe download does not match the expected file' }
    }
    $emm
}

function Get-BuildId($manifestText) {
    [regex]::Match($manifestText, '"buildid"\s+"(\d+)"').Groups[1].Value
}

# ---- SFTP (Posh-SSH) -----------------------------------------------------------------
if (-not (Get-Module -ListAvailable Posh-SSH)) {
    Step 'Installing the Posh-SSH module (SFTP support, once)'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Install-PackageProvider -Name NuGet -MinimumVersion 2.8.5.201 -Scope CurrentUser -Force | Out-Null
    Install-Module Posh-SSH -Scope CurrentUser -Force -AllowClobber
}
Import-Module Posh-SSH

if (-not $HostName) { $HostName = Read-Host '   SFTP host (from the panel, e.g. xyz.myserverfiles.com)' }
if (-not $User) { $User = Read-Host '   SFTP user name' }
Step "Connecting to $HostName"
$password = Read-Host "   SFTP password for $User" -AsSecureString
$credential = New-Object System.Management.Automation.PSCredential($User, $password)
$session = New-SFTPSession -ComputerName $HostName -Port $Port -Credential $credential -AcceptKey
$sid = $session.SessionId

function Remote($path) { '/' + $path.TrimStart('/') }
function Exists($path) { Test-SFTPPath -SessionId $sid -Path (Remote $path) }
function RemoteSize($path) { (Get-SFTPPathAttribute -SessionId $sid -Path (Remote $path)).Size }
function RemoteText($path) { Get-SFTPContent -SessionId $sid -Path (Remote $path) }
function RemoteRename($path, $newName) { Rename-SFTPFile -SessionId $sid -Path (Remote $path) -NewName $newName }
function HostBuild { Get-BuildId (RemoteText 'steamapps/appmanifest_2278520.acf') }

# Uploads $local as remote file $name (a path like "shroudlink_mods/x.dll"): first as
# $name.new, then the old file is renamed to $name.$keepSuffix and the new one takes its place.
function Upload($local, $name, $keepSuffix) {
    $leaf = Split-Path $name -Leaf
    $dir = Remote (Split-Path $name -Parent)
    $tmp = Join-Path $env:TEMP "$leaf.new"
    Copy-Item $local $tmp -Force
    Set-SFTPItem -SessionId $sid -Path $tmp -Destination $dir -Force
    Remove-Item $tmp
    if ((RemoteSize "$name.new") -ne (Get-Item $local).Length) { throw "upload of $name is incomplete" }
    if (Exists $name) { RemoteRename $name "$leaf.$keepSuffix" }
    RemoteRename "$name.new" $leaf
    Info "$name uploaded ($((Get-Item $local).Length) bytes)"
}

try {
    # ---- 1. stop -------------------------------------------------------------------
    Step '1. Stop the server'
    Wait-User 'In the Game Host Bros panel: STOP the server and wait until it shows "offline".'

    # ---- 2. backup -----------------------------------------------------------------
    Step '2. Backing up the savegame'
    $backup = Join-Path $PSScriptRoot "backups\savegame_$stamp"
    New-Item -ItemType Directory -Force $backup | Out-Null
    Get-SFTPItem -SessionId $sid -Path '/savegame' -Destination $backup -Force
    $remoteFiles = Get-SFTPChildItem -SessionId $sid -Path '/savegame' -Recursive | Where-Object { -not $_.IsDirectory }
    $local = Get-ChildItem (Join-Path $backup 'savegame') -Recurse -File
    $remoteBytes = ($remoteFiles | Measure-Object Length -Sum).Sum
    $localBytes = ($local | Measure-Object Length -Sum).Sum
    if ($local.Count -ne $remoteFiles.Count -or $localBytes -ne $remoteBytes) {
        throw "backup incomplete: $($local.Count)/$($remoteFiles.Count) files, $localBytes/$remoteBytes bytes"
    }
    Info "$($local.Count) files, $([math]::Round($localBytes / 1MB, 1)) MB -> $backup"

    # ---- 3. build the patched files ------------------------------------------------
    Step '3. Building the patched data files for the newest build'
    $steamcmd = Join-Path $repatch 'steamcmd\steamcmd.exe'
    if (-not (Test-Path $steamcmd)) {
        Info 'downloading SteamCMD...'
        $zip = Join-Path $repatch 'steamcmd.zip'
        Invoke-WebRequest 'https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip' -OutFile $zip
        Expand-Archive $zip (Join-Path $repatch 'steamcmd') -Force
        Remove-Item $zip
    }
    Info 'downloading / updating the dedicated server (first run ~8 GB)...'
    & $steamcmd +force_install_dir $ServerDir +login anonymous +app_update 2278520 validate +quit | Out-Host
    $manifest = Join-Path $ServerDir 'steamapps\appmanifest_2278520.acf'
    if (-not (Test-Path $manifest)) { throw 'server download failed, run the script again' }
    $build = Get-BuildId (Get-Content $manifest -Raw)
    # validate restored the vanilla files; a backup from an older build must not be the patch source
    Remove-Item (Join-Path $ServerDir 'enshrouded_server.kfc.bak') -ErrorAction SilentlyContinue
    Copy-Item (Join-Path $repatch 'mods') $ServerDir -Recurse -Force
    & (Get-Emm $repatch) run -g $ServerDir --patch | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'patching failed (see the messages above)' }
    Info "patched files ready for build $build"

    # ---- 4. update the server without a world --------------------------------------
    Step '4. Updating the server'
    $hostBuild = HostBuild
    Info "server build: $hostBuild, newest build: $build"
    if ($hostBuild -ne $build) {
        $parked = "savegame.parked-$stamp"
        RemoteRename 'savegame' $parked
        Info "world parked as ${parked}: the update start cannot touch it"
        while ($true) {
            Wait-User ("Panel: turn AUTO UPDATE ON, START the server, wait until the console says it is running " +
                       "(the update runs first, a new empty world is created), then STOP it again.")
            $hostBuild = HostBuild
            if ($hostBuild -eq $build) { break }
            Write-Host "   The server is still on build $hostBuild (expected $build). Did the update run? Try again." -ForegroundColor Red
        }
        # ---- 5. world back ---------------------------------------------------------
        Step '5. Putting the world back'
        if (Exists 'savegame') { RemoteRename 'savegame' "savegame.empty-world-$stamp" }
        RemoteRename $parked 'savegame'
        Info 'world restored (the empty world from the update start is kept as savegame.empty-world-*)'
    } else {
        Info 'the server already runs the newest build'
    }

    # ---- 6. upload -----------------------------------------------------------------
    Step '6. Uploading patched files and the mod'
    Upload (Join-Path $ServerDir 'enshrouded_server.kfc') 'enshrouded_server.kfc' "vanilla-$build"
    Upload (Join-Path $ServerDir 'enshrouded_server.kfc_resources') 'enshrouded_server.kfc_resources' "vanilla-$build"
    # the shroudlink core (as Steam's DLL, which it passes every call on to) and the mods
    Upload (Join-Path $repatch 'steam_api64.dll') 'steam_api64.dll' "before-$stamp"
    if (-not (Exists 'shroudlink_mods')) { New-SFTPItem -SessionId $sid -Path '/shroudlink_mods' -ItemType Directory | Out-Null }
    foreach ($mod in Get-ChildItem (Join-Path $PSScriptRoot 'shroudlink_mods') -Filter '*.dll') {
        Upload $mod.FullName "shroudlink_mods/$($mod.Name)" "before-$stamp"
    }
    if (-not (Exists 'steam_api64_o.dll')) {
        Write-Host '   WARNING: steam_api64_o.dll (the original Steam DLL) is missing on the server!' -ForegroundColor Red
        Write-Host '   The mod cannot start without it. Ask for help before starting the server.' -ForegroundColor Red
    }
    # the patched data expects these (empty) data containers
    $empty = Join-Path $env:TEMP 'empty.dat'
    Set-Content $empty -Value $null -NoNewline
    foreach ($i in 10..15) {
        $name = "enshrouded_server_0$i.dat"
        if (-not (Exists $name)) {
            Copy-Item $empty (Join-Path $env:TEMP $name) -Force
            Set-SFTPItem -SessionId $sid -Path (Join-Path $env:TEMP $name) -Destination '/' -Force
            Info "$name created"
        }
    }

    # ---- 7. start ------------------------------------------------------------------
    Step '7. Start the server'
    Wait-User ('Panel: turn AUTO UPDATE OFF (otherwise the next start can undo the patch), then START the server. ' +
               'Watch the console: if it prints "Deleted ... items from stack", STOP it immediately - ' +
               "your backup is in $backup.")
    $logName = if (Exists 'shroudlink.log') { 'shroudlink.log' } else { 'chestsort.log' }
    $log = (RemoteText $logName) -split "`n" | Select-Object -Last 8
    Write-Host ''
    Info 'end of the mod log (should show "inventory hooks installed" and "mod ''chestsort'' registered"):'
    $log | ForEach-Object { Info $_ }
    Write-Host ''
    Write-Host 'Done.' -ForegroundColor Green
}
finally {
    Remove-SFTPSession -SessionId $sid | Out-Null
}
