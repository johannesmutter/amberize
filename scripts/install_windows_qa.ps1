$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'This installer test requires a fresh hosted CI profile' }
$qa = Join-Path $env:RUNNER_TEMP 'amberize-qa'
$installer = Get-ChildItem 'target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi' | Select-Object -First 1
if (-not $installer) { throw 'MSI installer missing' }
Get-FileHash $installer.FullName -Algorithm SHA256 | ConvertTo-Json | Set-Content (Join-Path $qa 'installer-hash.json')
$installLog = Join-Path $qa 'install.log'
$install = Start-Process msiexec.exe -ArgumentList @('/i', ('"' + $installer.FullName + '"'), '/qn', '/norestart', '/l*v', ('"' + $installLog + '"')) -Wait -PassThru
if ($install.ExitCode -notin @(0, 3010)) { throw "Installer exited $($install.ExitCode)" }
$candidates = @((Join-Path $env:ProgramFiles 'Amberize/Amberize.exe'), (Join-Path $env:LOCALAPPDATA 'Amberize/Amberize.exe'))
$binary = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $binary) { throw 'Installed Amberize executable not found' }
python scripts/native_release_smoke.py --binary $binary --fixture (Join-Path $qa 'archive.sqlite3') --config-dir (Join-Path $env:APPDATA 'com.amberize.app') --report (Join-Path $qa 'native.json')
if ($LASTEXITCODE -ne 0) { throw 'Native release smoke test failed' }
