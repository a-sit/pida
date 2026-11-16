# PIDA – Installationstest für das Windows-Setup (NSIS): installiert still, prüft die Verknüpfungen und deinstalliert.
#
#   pwsh -File tools/test-windows-installer.ps1 -Installer target\debug\bundle\nsis\PIDA_…-setup.exe
#
# Geprüft wird, dass die Verknüpfungen im Startmenü und auf dem Desktop auf eine vorhandene pida.exe zeigen.
# Anlass (0.8.3): Auf einem Rechner lag nach der Installation eine Desktop-Verknüpfung mit leerem Ziel.
#
# -Belegen: Ein zweiter Prozess öffnet die Desktop-Verknüpfung exklusiv, sobald es sie gibt, und hält sie kurz –
# so wie es Virenscanner oder die OneDrive-Sicherung des Desktops tun. Der Installer muss damit zurechtkommen.
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [switch]$Belegen
)
$ErrorActionPreference = 'Stop'
$Installer = (Resolve-Path $Installer).Path
$desktops = @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('CommonDesktopDirectory'))
$menus = @([Environment]::GetFolderPath('Programs'), [Environment]::GetFolderPath('CommonPrograms'))

function Find-Link([string[]]$dirs) {
  foreach ($d in $dirs) {
    $hit = Get-ChildItem -Path $d -Filter 'PIDA.lnk' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($hit) { return $hit.FullName }
  }
  return $null
}

$locker = $null
if ($Belegen) {
  $script = @'
param($path)
$end = (Get-Date).AddSeconds(120)
while ((Get-Date) -lt $end) {
  try {
    $f = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
    Start-Sleep -Milliseconds 1500
    $f.Close()
    Write-Output "belegt: $path"
    break
  } catch { }
}
'@
  $file = Join-Path ([IO.Path]::GetTempPath()) 'pida-belegen.ps1'
  Set-Content -Path $file -Value $script -Encoding utf8
  $target = Join-Path $desktops[0] 'PIDA.lnk'
  Remove-Item $target -ErrorAction SilentlyContinue
  $locker = Start-Process pwsh -ArgumentList '-NoProfile', '-File', $file, $target -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput (Join-Path ([IO.Path]::GetTempPath()) 'pida-belegen.txt')
  Start-Sleep -Seconds 2
}

Write-Host "Installiere $Installer (still)"
$p = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "Setup endete mit Code $($p.ExitCode)" }
if ($locker) {
  $locker.WaitForExit(10000) | Out-Null
  if (-not $locker.HasExited) { $locker.Kill() }
  Get-Content (Join-Path ([IO.Path]::GetTempPath()) 'pida-belegen.txt') -ErrorAction SilentlyContinue | Write-Host
}

$shell = New-Object -ComObject WScript.Shell
$failed = @()
$uninstall = $null
foreach ($pair in @(@('Startmenü', $menus), @('Desktop', $desktops))) {
  $name = $pair[0]
  $link = Find-Link $pair[1]
  if (-not $link) { $failed += "${name}: keine Verknüpfung PIDA.lnk"; continue }
  $target = $shell.CreateShortcut($link).TargetPath
  Write-Host "${name}: $link -> '$target'"
  if (-not $target) { $failed += "${name}: Verknüpfung ohne Ziel"; continue }
  if (-not (Test-Path $target)) { $failed += "${name}: Ziel fehlt ($target)"; continue }
  if ((Split-Path $target -Leaf) -ne 'pida.exe') { $failed += "${name}: Ziel ist nicht pida.exe ($target)" }
  $uninstall = Join-Path (Split-Path $target -Parent) 'uninstall.exe'
}

if ($uninstall -and (Test-Path $uninstall)) {
  Write-Host "Deinstalliere ($uninstall)"
  Start-Process -FilePath $uninstall -ArgumentList '/S' -Wait | Out-Null
}
if ($failed.Count) { $failed | ForEach-Object { Write-Host "FEHLER: $_" }; exit 1 }
Write-Host 'Installationstest: ok'
