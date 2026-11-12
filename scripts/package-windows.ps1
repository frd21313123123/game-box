# Collects the Windows package into dist/game-box and zips it as
# dist/game-box-windows-x64.zip. Used by CI, which publishes it on the
# Releases page.
#
# The zip layout matters: the in-app updater (launcher/src/update.rs) expects
# a `game-box\` folder at the top level of the archive.
param([string]$Target = 'target/release')

$ErrorActionPreference = 'Stop'
$pkg = 'dist/game-box'

Remove-Item -LiteralPath 'dist' -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $pkg | Out-Null

Copy-Item "$Target/game-box.exe" $pkg
$dll = Get-ChildItem -Path "$Target/build" -Recurse -Filter steam_api64.dll | Select-Object -First 1
if (-not $dll) { throw 'steam_api64.dll was not found in the build output' }
Copy-Item $dll.FullName $pkg
Copy-Item README.md $pkg
Copy-Item assets/fonts/LICENSE-Inter.txt $pkg

Compress-Archive -Path $pkg -DestinationPath 'dist/game-box-windows-x64.zip' -Force
