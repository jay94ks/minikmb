param()
$ErrorActionPreference = 'Stop'
$guiRoot = Split-Path $PSScriptRoot
$original = Join-Path $guiRoot '../build/tools/flashrom-1.8.0'
if (Test-Path -LiteralPath "$original/.git") {
    $sha = (& wsl.exe --exec git -C ((& wsl.exe --exec wslpath -u $original).Trim()) rev-parse HEAD).Trim()
    if ($sha -ne '90b6b4376c7480a223e7bb5642e7116315ac6d09') { throw 'Unexpected flashrom source revision' }
}
& 'C:/Program Files/Python312/python.exe' "$PSScriptRoot/prepare.py"
if ($LASTEXITCODE -ne 0) { throw 'Native source preparation failed' }
$linuxGui = (& wsl.exe --exec wslpath -u $guiRoot).Trim()
$buildDir = "$linuxGui/native/build/windows"
$configuration = @('setup', $buildDir, "$linuxGui/native/build/flashrom-src", '--cross-file', "$linuxGui/../cmake/flashrom-mingw.ini",
    '-Dprogrammer=serprog', '-Dclassic_cli=disabled', '-Dtests=disabled', '-Ddocumentation=disabled',
    '-Dman-pages=disabled', '-Drpmc=disabled', '-Ddefault_library=static', '-Dich_descriptors_tool=disabled',
    '-Dbash_completion=disabled', '-Dni845x_search_path=/nonexistent', '-Duse_git_version=disabled', '-Dlog_message_length_limit=1024')
if (Test-Path -LiteralPath "$PSScriptRoot/build/windows/meson-private/coredata.dat") { $configuration += '--reconfigure' }
& wsl.exe --exec meson @configuration
if ($LASTEXITCODE -ne 0) { throw 'Native Meson configuration failed' }
& wsl.exe --exec meson compile -C $buildDir -j 6
if ($LASTEXITCODE -ne 0) { throw 'Native library build failed' }
New-Item -ItemType Directory -Force -Path "$PSScriptRoot/lib" | Out-Null
Copy-Item -LiteralPath "$PSScriptRoot/build/windows/libflashrom.a" -Destination "$PSScriptRoot/lib/libflashrom.a"
New-Item -ItemType Directory -Force -Path "$PSScriptRoot/runtime" | Out-Null
$linuxRuntime = "$linuxGui/native/runtime"
$gccLibrary = (& wsl.exe --exec x86_64-w64-mingw32-gcc '-print-libgcc-file-name').Trim()
$gccEhLibrary = (& wsl.exe --exec x86_64-w64-mingw32-gcc '-print-file-name=libgcc_eh.a').Trim()
& wsl.exe --exec cp $gccLibrary "$linuxRuntime/libgcc.a"
if ($LASTEXITCODE -ne 0) { throw 'SEH runtime copy failed' }
& wsl.exe --exec cp $gccEhLibrary "$linuxRuntime/libgcc_eh.a"
if ($LASTEXITCODE -ne 0) { throw 'SEH unwinder copy failed' }
& wsl.exe --exec cp /usr/share/doc/gcc-13-base/copyright "$linuxRuntime/GCC-COPYRIGHT.txt"
if ($LASTEXITCODE -ne 0) { throw 'GCC runtime notices copy failed' }
Get-FileHash -LiteralPath "$PSScriptRoot/lib/libflashrom.a" -Algorithm SHA256
