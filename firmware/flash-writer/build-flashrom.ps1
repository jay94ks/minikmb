param()

$ErrorActionPreference = 'Stop'
$flashromSha = '90b6b4376c7480a223e7bb5642e7116315ac6d09'
$linuxRoot = (& wsl.exe --exec wslpath -u $PSScriptRoot).Trim()
if ($LASTEXITCODE -ne 0) { throw 'WSL path conversion failed.' }
$sourceDir = "$linuxRoot/build/tools/flashrom-1.8.0"
$buildDir = "$linuxRoot/build/tools/flashrom-windows"
if (-not (Test-Path -LiteralPath "$PSScriptRoot/build/tools/flashrom-1.8.0/.git")) {
    & wsl.exe --exec git clone --depth 1 --branch v1.8.0 https://github.com/flashrom/flashrom.git $sourceDir
    if ($LASTEXITCODE -ne 0) { throw 'Official flashrom clone failed.' }
}
$actualSha = (& wsl.exe --exec git -C $sourceDir rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $actualSha -ne $flashromSha) { throw 'Unexpected flashrom revision.' }
$sourceChanges = & wsl.exe --exec git -C $sourceDir status --porcelain --untracked-files=no
if ($LASTEXITCODE -ne 0 -or $sourceChanges) { throw 'flashrom source has local changes.' }
$configuration = @('setup', $buildDir, $sourceDir, '--cross-file', "$linuxRoot/cmake/flashrom-mingw.ini",
    '-Dprogrammer=serprog', '-Dtests=disabled', '-Ddocumentation=disabled', '-Dman-pages=disabled',
    '-Drpmc=disabled', '-Ddefault_library=static', '-Dich_descriptors_tool=disabled',
    '-Dbash_completion=disabled', '-Dni845x_search_path=/nonexistent')
if (Test-Path -LiteralPath "$PSScriptRoot/build/tools/flashrom-windows/meson-private/coredata.dat") {
    $configuration += '--reconfigure'
}
& wsl.exe --exec meson @configuration
if ($LASTEXITCODE -ne 0) { throw 'flashrom Meson configuration failed.' }
& wsl.exe --exec meson compile -C $buildDir -j 6
if ($LASTEXITCODE -ne 0) { throw 'flashrom build failed.' }
& "$PSScriptRoot/build/tools/flashrom-windows/flashrom.exe" --version
if ($LASTEXITCODE -ne 0) { throw 'Windows flashrom failed to run.' }
Get-FileHash -LiteralPath "$PSScriptRoot/build/tools/flashrom-windows/flashrom.exe" -Algorithm SHA256
