param(
    [string]$Cli = 'target/x86_64-pc-windows-msvc/debug/seek-cli.exe',
    [string]$Output = 'docs/benchmarks/real-world-check.json',
    [switch]$UseSavedVerification
)
$ErrorActionPreference = 'Stop'
# Only reads user folders. All reports and indexes stay inside the repository.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SeekRealKnownFolders {
 [DllImport("shell32.dll")] static extern int SHGetKnownFolderPath([MarshalAs(UnmanagedType.LPStruct)] Guid id, uint flags, IntPtr token, out IntPtr path);
 public static string Get(string id) { IntPtr p; int hr=SHGetKnownFolderPath(new Guid(id),0,IntPtr.Zero,out p); if(hr<0) Marshal.ThrowExceptionForHR(hr); try{return Marshal.PtrToStringUni(p);}finally{Marshal.FreeCoTaskMem(p);} }
}
'@
$roots = [ordered]@{
    Downloads = [SeekRealKnownFolders]::Get('374DE290-123F-4565-9164-39C4925E467B')
    Documents = [SeekRealKnownFolders]::Get('FDD39AD0-238F-46AF-ADB4-6C85480369C7')
    Desktop = [SeekRealKnownFolders]::Get('B4BFCC3A-DB2C-424C-B029-7FE99A87C641')
}
$folders = @()
$listing = @()
foreach ($label in $roots.Keys) {
    $reportPath = 'docs/benchmarks/real-' + $label.ToLowerInvariant() + '-verify.json'
    if (-not $UseSavedVerification) {
        & $Cli verify --root $roots[$label] --data ('.bench-data/real-world/' + $label) --output $reportPath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "verify failed for $label; inspect $reportPath" }
    }
    $verification = Get-Content -Raw -Encoding UTF8 $reportPath | ConvertFrom-Json
    if ($verification.report.missing.Count -or $verification.report.extra.Count -or $verification.report.stale.Count) { throw "verify discrepancies in $label" }
    # Plain PowerShell directory listing, independent of the Rust index/walker.
    # PowerShell traverses cloud directories but does not follow junctions/symlinks.
    $entries = @(Get-ChildItem -LiteralPath $roots[$label] -Force -Recurse)
    foreach ($e in $entries) {
        $listing += [pscustomobject]@{ root=$label; path=$e.FullName; name=$e.Name; parent=$e.DirectoryName; directory=$e.PSIsContainer; extension=$e.Extension.ToLowerInvariant(); mtime=$e.LastWriteTimeUtc; depth=$e.FullName.Substring($roots[$label].Length).Split('\').Count-1 }
    }
    if ($entries.Count + 1 -ne $verification.report.ground_truth) { throw "Independent listing count differs for $label" }
    $folders += [pscustomobject]@{ name=$label; path=$roots[$label]; entries=$verification.report.ground_truth; files=@($entries | Where-Object { -not $_.PSIsContainer }).Count; directories=@($entries | Where-Object PSIsContainer).Count; missing=$verification.report.missing.Count; extra=$verification.report.extra.Count; stale=$verification.report.stale.Count; denied=$verification.report.denied.Count; catch_up_ms=$verification.catch_up_ms; verify_ms=$verification.verify_ms }
}
$cases = [System.Collections.Generic.List[object]]::new()
$chosen = @{}
function Add-Case($label, $query, $expected, $mode) {
    $cases.Add([pscustomobject]@{root=$label;query=$query;expected=@($expected);mode=$mode})
}
# Choose words from the directory listing BEFORE invoking searches. Prefer
# distinct meaningful words; never select expected files from engine results.
foreach ($spec in @(@('Downloads',4),@('Documents',4),@('Desktop',2))) {
    $label=$spec[0]; $wanted=[int]$spec[1]; $count=0
    foreach ($file in @($listing | Where-Object { $_.root -eq $label -and -not $_.directory } | Sort-Object mtime -Descending)) {
        $words=@([regex]::Matches([IO.Path]::GetFileNameWithoutExtension($file.name),'[a-zA-Z]{6,}') | ForEach-Object { $_.Value.ToLowerInvariant() })
        foreach ($word in $words) {
            if ($chosen.ContainsKey($word)) { continue }
            $matches=@($listing | Where-Object { $_.root -eq $label -and $_.name -match [regex]::Escape($word) })
            if ($matches.Count -gt 12) { continue }
            $chosen[$word]=$file
            Add-Case $label $word.Substring(0,$word.Length-1) @($file.path) 'partial-name'
            $count++; break
        }
        if ($count -eq $wanted) { break }
    }
    if ($count -ne $wanted) { throw "Insufficient diverse filename words in $label" }
}
$typos=0
foreach ($word in @($chosen.Keys | Sort-Object)) {
    $file=$chosen[$word]
    if ($file.root -eq 'Desktop') { continue }
    if ($typos % 2 -eq 0) {
        $misspelled=$word.Substring(0,2)+$word.Substring(3)
        $mode='one-missing-letter'
    } else {
        if ($word[2] -eq $word[3]) { continue }
        $misspelled=$word.Substring(0,2)+$word[3]+$word[2]+$word.Substring(4)
        $mode='adjacent-swapped-letters'
    }
    Add-Case $file.root $misspelled @($file.path) $mode
    $typos++
    if ($typos -eq 4) { break }
}
foreach ($spec in @(@('Downloads','.pdf'),@('Documents','.pdf'),@('Documents','.docx'))) {
    $label=$spec[0]; $ext=$spec[1]
    $expected=@($listing | Where-Object { $_.root -eq $label -and -not $_.directory -and $_.extension -eq $ext } | ForEach-Object path)
    Add-Case $label ('ext:'+$ext.TrimStart('.')) $expected 'complete-extension-filter'
}
$recentBoundary=[DateTime]::UtcNow.AddDays(-7)
$recent=@($listing | Where-Object { $_.root -eq 'Downloads' -and -not $_.directory -and $_.mtime -ge $recentBoundary } | ForEach-Object path)
Add-Case 'Downloads' 'kind:file mtime:<7d' $recent 'complete-recent-downloads-filter'
$deep=$listing | Where-Object { $_.root -eq 'Downloads' -and -not $_.directory -and $_.depth -ge 3 -and $_.extension -eq '.exe' } | Select-Object -First 1
if (-not $deep) { throw 'No deep executable for scoped search' }
Add-Case 'Downloads' ('ext:exe in:"'+$deep.parent+'"') @($deep.path) 'deep-folder-scope'
$latest=$listing | Where-Object { $_.root -eq 'Downloads' -and -not $_.directory } | Sort-Object mtime -Descending | Select-Object -First 1
Add-Case 'Downloads' 'kind:file' @($latest.path) 'empty-filename-query-newest-first'
if ($cases.Count -ne 20) { throw "Expected 20 cases; got $($cases.Count)" }
$searches=@()
$failed=0
foreach ($case in $cases) {
    $destination='.bench-data/real-world-search.json'
    $query=$case.query+' limit:2000'
    & $Cli search --root $roots[$case.root] --data ('.bench-data/real-world/'+$case.root) --query $query --output $destination | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Search failed: $query" }
    $results=@(Get-Content -Raw -Encoding UTF8 $destination | ConvertFrom-Json | ForEach-Object { $_ } | Where-Object { $null -ne $_ -and $null -ne $_.path })
    $paths=@($results | ForEach-Object path)
    $missing=@($case.expected | Where-Object { $_ -notin $paths })
    $notListed=@($paths | Where-Object { $_ -notin @($listing | Where-Object root -eq $case.root | ForEach-Object path) })
    $nonexistent=@($paths | Where-Object { -not (Test-Path -LiteralPath $_) })
    $wrongFilter=@()
    if ($case.mode.StartsWith('complete-')) { $wrongFilter=@($paths | Where-Object { $_ -notin $case.expected }) }
    $rank=if ($case.expected.Count -eq 1) { [array]::IndexOf($paths,$case.expected[0])+1 } else { $null }
    $ordering=$true
    if ($case.mode -eq 'empty-filename-query-newest-first') {
        for($i=1;$i -lt $results.Count;$i++) { if($results[$i].mtime -gt $results[$i-1].mtime) { $ordering=$false } }
        if($rank -ne 1) { $ordering=$false }
    }
    $passed=($missing.Count -eq 0 -and $notListed.Count -eq 0 -and $nonexistent.Count -eq 0 -and $wrongFilter.Count -eq 0 -and $ordering)
    if(-not $passed) { $failed++ }
    $searches += [pscustomobject]@{ root=$case.root; query=$case.query; mode=$case.mode; limit=2000; expected=$case.expected; returned=$paths.Count; expected_rank=$rank; missing=$missing; not_in_plain_listing=$notListed; nonexistent=$nonexistent; wrong_filter=$wrongFilter; newest_first=$ordering; passed=$passed; results=$results }
}
$report=[ordered]@{timestamp_utc=[DateTime]::UtcNow.ToString('o'); executable=$Cli; folders=$folders; searches=$searches; queries_passed=20-$failed; queries_failed=$failed; limitations=@('Read-only snapshot checks: user files were not changed or opened.', 'The 2,000-result audit limit exceeds every folder population; this checks completeness, not top-200 truncation or UI latency.', 'Cloud directory metadata is listed without downloading file contents. Junctions and symbolic links are not traversed.', 'Known Folders API resolves redirected OneDrive locations. Memory and query performance use the retained Stage 1 benchmark; not measured by this audit.')}
$report | ConvertTo-Json -Depth 12 | Set-Content -Encoding UTF8 $Output
[pscustomobject]@{report=$Output;folders=$folders;passed=20-$failed;failed=$failed;failures=@($searches | Where-Object { -not $_.passed } | Select-Object root,query,mode,missing,not_in_plain_listing,nonexistent,wrong_filter)} | ConvertTo-Json -Depth 8
if ($failed) { exit 1 }
