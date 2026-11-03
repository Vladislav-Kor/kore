<#
  Registers the task "KORE guard": kore-guard.ps1 elevated in the user's session at every logon, restarted if it
  fails, and starts it now. The user's session because the guard must see the active window, session 0 cannot.
  Needs administrator rights: the KORE app asks for them through Windows.
  -Remove stops and deletes the task.
#>
param([switch]$Remove)

$name = 'KORE guard'
$stop = Join-Path $PSScriptRoot 'guard.stop'

if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
    # Ask the running guard to remove its caps and exit; killing it would leave the caps on.
    New-Item -ItemType File -Path $stop -Force | Out-Null
    for ($i = 0; $i -lt 20 -and (Test-Path $stop); $i++) { Start-Sleep -Milliseconds 500 }
    Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $name -Confirm:$false
    Remove-Item $stop -ErrorAction SilentlyContinue
}
if ($Remove) { Write-Host 'Сторож CPU выключен, все потолки сняты.'; return }

$act = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument ('-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + (Join-Path $PSScriptRoot 'kore-guard.ps1') + '"')
$trg = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$set = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew -StartWhenAvailable -Priority 4
$pri = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Highest
Register-ScheduledTask -TaskName $name -Action $act -Trigger $trg -Settings $set -Principal $pri -Force | Out-Null
Start-ScheduledTask -TaskName $name
Start-Sleep -Seconds 8
Write-Host ('Задача: ' + (Get-ScheduledTask -TaskName $name).State)
Write-Host 'Сторож CPU включён, предел загрузки задан в guard.conf. Журнал:'
Get-Content (Join-Path $PSScriptRoot 'guard.log') -Tail 5 -Encoding UTF8 -ErrorAction SilentlyContinue
