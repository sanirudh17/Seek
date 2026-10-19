param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$root=(Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
try{
 if(git status --porcelain --untracked-files=normal){throw 'Release builds require a clean source checkout. Commit changes or build in a clean clone.'}
 if((Get-Content Cargo.toml -Raw) -notmatch '(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"'){throw 'Expected a stable workspace release version'}
 $productVersion=$Matches[1]
 $oldFlags=$env:CARGO_ENCODED_RUSTFLAGS
 try{
  $env:CARGO_ENCODED_RUSTFLAGS=@('-C','target-feature=+crt-static',('--remap-path-prefix='+$env:USERPROFILE+'=C:/build/user'),('--remap-path-prefix='+$root+'=C:/build/Seek')) -join [char]31
  if(-not $SkipBuild){
   & cargo build --release --locked -p seek -p seek-helper
   if($LASTEXITCODE -ne 0){throw 'Optimized build failed'}
  }
 }finally{$env:CARGO_ENCODED_RUSTFLAGS=$oldFlags}
 $target=if($env:CARGO_TARGET_DIR){$env:CARGO_TARGET_DIR}else{Join-Path $root 'target'}
 $release=Join-Path $target 'x86_64-pc-windows-msvc\release'
 $output=Join-Path $target 'release-package';$folder=Join-Path $output ('Seek-'+$productVersion+'-windows-x64')
 if(Test-Path -LiteralPath $folder){throw 'Package staging directory already exists; choose a new CARGO_TARGET_DIR for a fresh release'}
 New-Item -ItemType Directory -Path $folder -Force|Out-Null
 foreach($name in @('seek.exe','seek-helper.exe')){Copy-Item -LiteralPath (Join-Path $release $name) -Destination $folder}
 $info=[Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $folder 'seek.exe'))
 if($info.FileVersion -ne $productVersion -or $info.ProductName -ne 'Seek'){throw 'Executable metadata does not match release'}
 foreach($name in @('README.md','LICENSE','THIRD-PARTY-NOTICES.txt')){Copy-Item -LiteralPath (Join-Path $root $name) -Destination $folder}
 Copy-Item -LiteralPath (Join-Path $root 'app/assets/seek.ico') -Destination $folder
 foreach($name in @('Install.cmd','Install.ps1','Uninstall.cmd','Uninstall.ps1','Update.ps1')){Copy-Item -LiteralPath (Join-Path $root ('packaging/'+$name)) -Destination $folder}
 $zip=Join-Path $output ('Seek-'+$productVersion+'-windows-x64.zip')
 Add-Type -AssemblyName System.IO.Compression.FileSystem
 [IO.Compression.ZipFile]::CreateFromDirectory($folder,$zip,[IO.Compression.CompressionLevel]::Optimal,$true)
 $hash=(Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
 [IO.File]::WriteAllText((Join-Path $output 'SHA256SUMS.txt'),($hash+'  '+(Split-Path $zip -Leaf)+"`n"),[Text.UTF8Encoding]::new($false))
 Write-Host ('Package: '+$zip)
}finally{Pop-Location}
