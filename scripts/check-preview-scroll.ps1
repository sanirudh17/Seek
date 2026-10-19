param([string]$Executable='target/x86_64-pc-windows-msvc/release/seek.exe', [string]$Output='docs/benchmarks/c-polish-preview.json')
# Own fixture/profile only. Native checks for selection changes and shared scroll styling.
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$driver=Join-Path $env:LOCALAPPDATA 'Programs/orca/resources/bin/orca.exe'
$base=Join-Path $env:TEMP ('seek-preview-scroll-'+[guid]::NewGuid().ToString('N'))
$root=Join-Path $base 'files';$data=Join-Path $base 'data';$trace=Join-Path $base 'trace.ndjson'
New-Item -ItemType Directory -Path $root,$data|Out-Null
foreach($name in @('A-red.png','B-blue.png')){
 $bitmap=[Drawing.Bitmap]::new(1200,900);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{
  $g.Clear($(if($name -like 'A-*'){[Drawing.Color]::Crimson}else{[Drawing.Color]::RoyalBlue}))
  for($n=0;$n -lt 40;$n++){$g.DrawLine([Drawing.Pens]::White,0,$n*22,1200,$n*22)}
  $bitmap.Save((Join-Path $root $name),[Drawing.Imaging.ImageFormat]::Png)
 }finally{$g.Dispose();$bitmap.Dispose()}
}
for($n=0;$n -lt 100;$n++){[IO.File]::WriteAllText((Join-Path $root ('C-{0:d3}.txt' -f $n)),('Owned preview fixture '+$n))}
[IO.File]::WriteAllText((Join-Path $data 'settings.json'),'{"theme":"dark","glass":true,"default_view":"list"}')
$exe=(Resolve-Path -LiteralPath $Executable).Path
$app=Start-Process $exe -ArgumentList @('--walker','--root',('"'+$root+'"'),'--data',('"'+$data+'"'),'--metrics',('"'+$trace+'"')) -PassThru
$checks=@();$captures=@()
function State([switch]$Capture){
 $a=@('computer','get-app-state','--app',('pid:'+$app.Id),'--restore-window','--json');if(-not $Capture){$a+='--no-screenshot'}
 $s=& $driver @a|ConvertFrom-Json;if(-not $s.ok){throw $s.error.message};$s
}
function Records {Get-Content -LiteralPath $trace -ErrorAction SilentlyContinue|ForEach-Object{try{$_|ConvertFrom-Json}catch{}}}
function WaitFor([scriptblock]$condition){
 $clock=[Diagnostics.Stopwatch]::StartNew()
 do{if(&$condition){return};Start-Sleep -Milliseconds 80}while($clock.Elapsed.TotalSeconds -lt 20)
 throw 'Preview check timed out'
}
function Click([string]$name){
 $null=State
 $w=[Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
 $e=$w.FindFirst([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name))
 if(-not $e){throw ('Missing '+$name)}
 $r=$e.Current.BoundingRectangle;$b=$w.Current.BoundingRectangle
 $s=& $driver computer click --app ('pid:'+$app.Id) --x ([int]($r.Left-$b.Left+[Math]::Min(70,$r.Width/2))) --y ([int]($r.Top-$b.Top+$r.Height/2)) --no-screenshot --json|ConvertFrom-Json
 if(-not $s.ok){throw $s.error.message}
}
function Capture([string]$name){
 $s=State -Capture;$path=Join-Path $base ($name+'.png')
 Copy-Item -LiteralPath $s.result.screenshot.path -Destination $path;$script:captures+=@{name=$name;path=$path}
}
try{
 WaitFor {(Records|Where-Object{$_.status.ready -and -not $_.status.catching_up -and $_.rows.Count -eq 102})}
 foreach($name in @('A-red.png','B-blue.png','C-000.txt','A-red.png')){
  $before=@(Records).Count;Click $name
  WaitFor {@(Records|Select-Object -Skip $before|Where-Object{$_.interaction -eq 'preview_ready' -and $_.detail.name -eq $name -and $_.detail.edge -eq 768 -and $_.detail.image}).Count -gt 0}
  WaitFor {(State).result.snapshot.treeText -notmatch 'Loading preview'}
  $checks+=@{name=('Selected high-resolution preview: '+$name);passed=$true}
 }
 Capture 'dark-preview-scroll'
 Click ('Info panel '+[char]0xb7+' Space')
 WaitFor {$s=Records|Where-Object{$_.interaction -eq 'preview_cache'}|Select-Object -Last 1;$s.detail.stats[2] -eq 0 -and $s.detail.stats[3] -eq 0}
 $checks+=@{name='Closing info panel releases large preview and details';passed=$true}
 Click ('Info panel '+[char]0xb7+' Space');Click 'Settings';Click 'Light';Click 'Done';Click 'B-blue.png'
 WaitFor {(State).result.snapshot.treeText -notmatch 'Loading preview'}
 Capture 'light-preview-scroll'
 $checks+=@{name='Light theme preview and scrollbar captured';passed=$true}
 if(-not $app.CloseMainWindow() -or -not $app.WaitForExit(5000) -or $app.ExitCode -ne 0){throw 'Close did not quit'}
 @{checks=$checks;captures=$captures;fixture=$base;sha256=(Get-FileHash $exe).Hash;limitations='Screenshots inspect settled quality and thumb geometry, not a high-speed recording of the transition.'}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $Output -Encoding UTF8
 'PASS '+$checks.Count+' preview/scroll checks'
}finally{if(-not $app.HasExited){[void]$app.CloseMainWindow();[void]$app.WaitForExit(5000)}}
