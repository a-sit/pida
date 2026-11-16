# PIDA – Installationstest für das Windows-Setup (NSIS): installiert still, prüft die Verknüpfungen und deinstalliert.
#
#   pwsh -File tools/test-windows-installer.ps1 -Installer target\debug\bundle\nsis\PIDA_…-setup.exe
#
# Geprüft wird, dass die Verknüpfungen im Startmenü und auf dem Desktop auf eine vorhandene pida.exe zeigen.
# Anlass (0.8.3): Auf einem Rechner lag nach der Installation eine Desktop-Verknüpfung mit leerem Ziel.
#
# -LadenSperren: Verbietet dem Benutzer für die Dauer der Installation, neue Dateien auf dem Desktop zu LESEN
# (vererbter Deny-Eintrag; Anlegen und Überschreiben bleiben erlaubt). Damit scheitert das Nachladen der frisch
# angelegten Verknüpfung so, wie es bei belegter Datei (Virenscanner, OneDrive) oder einem umgeleiteten Desktop
# (Netzlaufwerk, geteilter Ordner einer VM) scheitern kann. Der Installer darf die Verknüpfung dann nicht durch eine
# leere ersetzen.
param(
  [Parameter(Mandatory = $true)][string]$Installer,
  [switch]$LadenSperren
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

$deny = "$([Security.Principal.WindowsIdentity]::GetCurrent().Name):(OI)(IO)(RD)"
if ($LadenSperren) {
  Remove-Item (Join-Path $desktops[0] 'PIDA.lnk') -ErrorAction SilentlyContinue
  icacls $desktops[0] /deny $deny | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'icacls /deny fehlgeschlagen' }
  Write-Host "Lesen neuer Dateien gesperrt: $($desktops[0])"
}

Write-Host "Installiere $Installer (still)"
$p = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
$code = $p.ExitCode
if ($LadenSperren) {
  icacls $desktops[0] /remove:d ([Security.Principal.WindowsIdentity]::GetCurrent().Name) | Out-Null
  # der vererbte Eintrag an der schon angelegten Verknüpfung
  icacls $desktops[0] /reset /T /Q | Out-Null
}

if ($code -ne 0) { throw "Setup endete mit Code $code" }
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
