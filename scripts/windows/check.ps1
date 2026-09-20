param([Parameter(Mandatory=$true)][string]$Directory)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Set-Location $Directory
Write-Output "HOST=$env:COMPUTERNAME"
rustc -Vv
Get-FileHash Cargo.toml,Cargo.lock,rust-toolchain.toml,src/main.rs,src/lib.rs,src/windows_terminal.rs
cargo fmt -- --check
if($LASTEXITCODE){exit $LASTEXITCODE}
cargo check --locked --offline
if($LASTEXITCODE){exit $LASTEXITCODE}
cargo test --locked --offline
if($LASTEXITCODE){exit $LASTEXITCODE}
cargo clippy --all-targets --locked --offline -- -D warnings
if($LASTEXITCODE){exit $LASTEXITCODE}
cargo build --release --locked --offline
if($LASTEXITCODE){exit $LASTEXITCODE}
Get-FileHash target/release/hush.exe
