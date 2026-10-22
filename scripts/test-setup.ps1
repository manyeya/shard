# Install Shard with its Setup.exe and `--silent`, check what it put where, then remove it with
# `--uninstall --silent` and check it's gone. CI runs it on Windows.
# It installs into the real places (your Start menu, Settings > Apps, %LOCALAPPDATA%\Programs), so
# it only runs in CI, or with -Force on a machine you don't mind that on.
#   scripts/test-setup.ps1 <Shard-Setup-….exe> [-Force]
param([Parameter(Mandatory)][string]$Setup, [switch]$Force)
$ErrorActionPreference = "Stop"
if (-not $env:CI -and -not $Force) { throw "This installs Shard for real and removes it again; pass -Force to run it here" }
$env:SHARD_PORT = "47119"
$env:SHARD_DATA = Join-Path ($env:RUNNER_TEMP ?? $env:TEMP) "shard-setup-data"
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
exit 0 # not the last `reg query`'s code, which is 1 when the key is gone, as it should be
