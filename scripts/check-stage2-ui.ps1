param(
    [string]$Executable = 'target/x86_64-pc-windows-msvc/debug/seek.exe',
    [string]$Output = 'docs/benchmarks/stage2-ui.json'
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SeekHelperSheetKeys {
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
}
'@
$seekExe=(Resolve-Path -LiteralPath $Executable).Path
$seekData=[IO.Path]::GetFullPath('.bench-data/stage2-ui')
$seekAudit=Get-Content docs/benchmarks/real-world-check.json -Raw -Encoding UTF8|ConvertFrom-Json
$seekDownloads=($seekAudit.folders|Where-Object name -eq Downloads).path
$seekApp=Start-Process -FilePath $seekExe -ArgumentList @('--walker','--root',('"'+$seekDownloads+'"'),'--data',('"'+$seekData+'"')) -WindowStyle Normal -PassThru -RedirectStandardError .references/stage2-ui-final.log
$seekHandle=$seekApp.Handle # Retain a native process handle through shutdown.
$seekApp.Id|Set-Content .references/stage2-ui.pid
function Find-SeekButton([string]$name) {
    $seekApp.Refresh()
    if($seekApp.HasExited -or $seekApp.MainWindowHandle -eq 0){return $null}
    $root=[Windows.Automation.AutomationElement]::FromHandle($seekApp.MainWindowHandle)
    $condition=[Windows.Automation.AndCondition]::new(
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name),
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button))
    return $root.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)
}
function Wait-SeekButton([string]$name,[bool]$present=$true) {
    $clock=[Diagnostics.Stopwatch]::StartNew()
    do {
        $button=Find-SeekButton $name
        if(($null -ne $button) -eq $present){return $button}
        Start-Sleep -Milliseconds 100 # UI test only.
    }while($clock.Elapsed.TotalSeconds -lt 10)
    throw "UI did not reach expected state for '$name' (present=$present)"
}
function Click-SeekButton([string]$name) {
    # Kit Invoke exposure did not activate the callback in the first check.
    # Use a fresh semantic frame/native click and assert its actual readback.
    $state=& (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer get-app-state --app ('pid:'+$seekApp.Id) --restore-window --no-screenshot --json | ConvertFrom-Json
    if(-not $state.ok){throw $state.error.message}
    $pattern='(?m)^\s*(\d+) button '+[regex]::Escape($name)+'\s*$'
    if($state.result.snapshot.treeText -notmatch $pattern){throw "Missing observed button: $name"}
    $action=& (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer click --app ('pid:'+$seekApp.Id) --element-index $Matches[1] --no-screenshot --json | ConvertFrom-Json
    if(-not $action.ok){throw $action.error.message}
}
try {
    $seekSettings=Wait-SeekButton 'Settings'
    Click-SeekButton 'Settings'
    $seekInstalled=target/x86_64-pc-windows-msvc/debug/seek-helper.exe status
    $seekHelperLabel=if($seekInstalled -match 'Some\('){'Uninstall helper'}else{'Install optional helper'}
    $seekInstall=Wait-SeekButton $seekHelperLabel
    # Readback only: NEVER invoke the install button in this check.
    $seekInstallEnabled=$seekInstall.Current.IsEnabled
    & (Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe') computer get-app-state --app ('pid:'+$seekApp.Id) --restore-window --json > .references/stage2-ui-final-state.json
    $seekState=Get-Content .references/stage2-ui-final-state.json -Raw -Encoding UTF8|ConvertFrom-Json
    if(-not $seekState.ok){throw ('GUI observation failed: '+$seekState.error.message)}
    [void][SeekHelperSheetKeys]::PostMessage($seekApp.MainWindowHandle,0x100,[IntPtr]27,[IntPtr]0)
    [void][SeekHelperSheetKeys]::PostMessage($seekApp.MainWindowHandle,0x101,[IntPtr]27,[IntPtr]0)
    [void](Wait-SeekButton $seekHelperLabel $false)
    $seekSettings=Wait-SeekButton 'Settings'
    Click-SeekButton 'Settings'
    $seekCloseButton=Wait-SeekButton 'Close'
    Click-SeekButton 'Close'
    [void](Wait-SeekButton $seekHelperLabel $false)
    $seekMemory=Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($seekApp.Id)"
    $seekClose=[Diagnostics.Stopwatch]::StartNew()
    [void]$seekApp.CloseMainWindow()
    if(-not $seekApp.WaitForExit(10000)){throw 'Window close did not exit the app'}
    $seekApp.WaitForExit()
    $seekReport=[ordered]@{
        executable=$seekExe;settings_button_opened=$true;helper_button=$seekHelperLabel;helper_button_enabled=$seekInstallEnabled
        escape_closed_sheet=$true;close_button_closed_sheet=$true;installation_triggered=$false
        close_ms=$seekClose.Elapsed.TotalMilliseconds;exit_code=$seekApp.ExitCode
        private_working_set_bytes=[long]$seekMemory.WorkingSetPrivate
        total_working_set_bytes=[long]$seekMemory.WorkingSet
        private_commit_bytes=[long]$seekMemory.PrivateBytes
        screenshot=$seekState.result.screenshot.path
        note='Actual Downloads, isolated app data, --walker. New helper controls only; no UAC or installation action. Not an idle CPU/launch/latency benchmark.'
    }
    $seekReport|ConvertTo-Json|Set-Content $Output -Encoding UTF8
    if($seekApp.ExitCode -ne 0){throw 'App exit code was not zero'}
    Write-Output 'Stage 2 helper sheet and process-close checks passed.'
}finally{
    if(-not $seekApp.HasExited){[void]$seekApp.CloseMainWindow();if(-not $seekApp.WaitForExit(5000)){Stop-Process -InputObject $seekApp}}
}
