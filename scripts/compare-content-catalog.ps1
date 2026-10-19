param(
 [Parameter(Mandatory=$true)][string]$Plan,
 [Parameter(Mandatory=$true)][string]$Output
)
# Read-only catalog comparison. Exact, scanner-approved paths restrict every
# query; protected files never enter an indexed-content query. Output is private.
$ErrorActionPreference='Stop'
$cases=Get-Content -LiteralPath $Plan -Raw -Encoding UTF8|ConvertFrom-Json
$connection=New-Object -ComObject ADODB.Connection
$report=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');service=(Get-Service WSearch).Status.ToString();cases=@()}
try {
 $connection.ConnectionTimeout=3;$connection.CommandTimeout=5
 $opening=[Diagnostics.Stopwatch]::StartNew()
 $connection.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")
 $report.open_ms=$opening.Elapsed.TotalMilliseconds
 foreach($case in $cases) {
  $all=@($case.eligible|ForEach-Object {([string]$_).Replace('\\?\','')})
  $queries=@(@{label='coverage';term=''},@{label='content';term=" AND CONTAINS(System.Search.Contents, '`"$($case.query)*`"')"})
  $sample=[ordered]@{query=$case.query;eligible=$all.Count;samples=@()}
  foreach($query in $queries) {
   $timer=[Diagnostics.Stopwatch]::StartNew();$paths=@();$batches=0
   for($start=0;$start -lt $all.Count;$start+=32) {
    $end=[Math]::Min($start+31,$all.Count-1)
    $in=($all[$start..$end]|ForEach-Object {"System.ItemPathDisplay='"+$_.Replace("'","''")+"'"})-join ' OR '
    $rs=$connection.Execute("SELECT System.ItemPathDisplay FROM SystemIndex WHERE ($in)$($query.term)")
    try {while(-not $rs.EOF){$paths+=([string]$rs.Fields.Item(0).Value);$rs.MoveNext()}} finally {$rs.Close()}
    $batches++
   }
   $sample.samples+=@{label=$query.label;elapsed_ms=$timer.Elapsed.TotalMilliseconds;batches=$batches;count=$paths.Count;paths=$paths}
  }
  $report.cases+=,$sample
 }
} catch {$report.error=$_.Exception.Message} finally {if($connection.State -ne 0){$connection.Close()}}
$report|ConvertTo-Json -Depth 10|Set-Content -LiteralPath $Output -Encoding UTF8
$report|Select-Object service,open_ms,error|Format-List
foreach($case in $report.cases){$case.samples|Select-Object label,count,elapsed_ms,batches|Format-Table}
if($report.error){exit 1}
