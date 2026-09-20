param([Parameter(Mandatory=$true)][string]$Source,[Parameter(Mandatory=$true)][ValidatePattern('^[A-Fa-f0-9]{64}$')][string]$Sha256)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
if((Get-FileHash $Source).Hash -ine $Sha256){throw 'hush.exe SHA256 mismatch'}
$bin=Join-Path $env:USERPROFILE '.local\bin'
[void][IO.Directory]::CreateDirectory($bin)
$dest=Join-Path $bin 'hush.exe'
if(Test-Path $dest){Copy-Item $dest ($dest+'.previous') -Force}
Copy-Item $Source ($dest+'.new') -Force
Move-Item ($dest+'.new') $dest -Force
$path=[string][Environment]::GetEnvironmentVariable('Path','User')
if($bin -notin ($path -split ';')){[Environment]::SetEnvironmentVariable('Path',($path.TrimEnd(';')+';'+$bin),'User')}
@{host_name=$env:COMPUTERNAME;binary=$dest;sha256=(Get-FileHash $dest).Hash;user_path_registered=$true} | ConvertTo-Json
& $dest sessions
exit $LASTEXITCODE
