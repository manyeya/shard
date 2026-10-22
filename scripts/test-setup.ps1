# Install Shard with its Setup.exe and `--silent`, check what it put where, then remove it with
# `--uninstall --silent` and check it's gone. CI runs it on Windows.
#   scripts/test-setup.ps1 <Shard-Setup-….exe>
param([Parameter(Mandatory)][string]$Setup)
$ErrorActionPreference = "Stop"
$env:SHARD_PORT = "47119"
$env:SHARD_DATA = Join-Path $env:RUNNER_TEMP "shard-setup-data"
$exe = Join-Path $env:LOCALAPPDATA "Programs\Shard\Shard.exe"
$menu = Join-Path ([Environment]::GetFolderPath("Programs")) "Shard.lnk"
$uninstall = "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Shard"
$run = "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"

function Run($file, $arguments) {
  # A windowed program: start it and wait, rather than letting the shell move on.
  $p = Start-Process -FilePath $file -ArgumentList $arguments -Wait -PassThru -NoNewWindow
  if ($p.ExitCode -ne 0) { throw "$file $arguments exited with $($p.ExitCode)" }
}
function Has-Key($key, $value) {
  $args = @("query", $key) + $(if ($value) { @("/v", $value) } else { @() })
  & reg @args *> $null
  return $LASTEXITCODE -eq 0
}

Run (Resolve-Path $Setup) "--silent"
if (-not (Test-Path $exe)) { throw "nothing installed at $exe" }
if (-not (Test-Path $menu)) { throw "no Start menu shortcut at $menu" }
if (-not (Has-Key $uninstall "UninstallString")) { throw "no entry in Settings > Apps" }
if (-not (Has-Key $run "Shard")) { throw "no login entry" }
# Again over the top, as an update would.
Run (Resolve-Path $Setup) "--silent"

Run $exe "--uninstall --silent"
Start-Sleep -Seconds 8 # Shard.exe is deleted once it has exited
if (Test-Path $exe) { throw "Shard.exe is still at $exe" }
if (Test-Path $menu) { throw "the Start menu shortcut is still there" }
if (Has-Key $uninstall $null) { throw "the entry in Settings > Apps is still there" }
if (Has-Key $run "Shard") { throw "the login entry is still there" }
Write-Output "ok: setup installed Shard, installed it again over the top, and removed it"
