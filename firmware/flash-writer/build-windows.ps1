param(
    [Parameter(Mandatory)]
    [string] $GnuArmRoot,

    [string] $LlvmRoot = "${env:ProgramFiles}/LLVM"
)

$ErrorActionPreference = 'Stop'
$LlvmRoot = (Resolve-Path -LiteralPath $LlvmRoot).Path.Replace('\', '/')
$GnuArmRoot = (Resolve-Path -LiteralPath $GnuArmRoot).Path.Replace('\', '/')
$clang = Join-Path $LlvmRoot 'bin/clang.exe'
if ((& $clang --version | Select-Object -First 1) -notmatch '^clang version 22\.') {
    throw 'The Windows fallback requires Clang 22.'
}

$sysroot = "$GnuArmRoot/arm-none-eabi"
$gccRuntimes = @(Get-ChildItem -LiteralPath "$GnuArmRoot/lib/gcc/arm-none-eabi" -Directory |
    Where-Object { Test-Path -LiteralPath "$($_.FullName)/thumb/v6-m/nofp/libgcc.a" })
if ($gccRuntimes.Count -ne 1 -or -not (Test-Path -LiteralPath "$sysroot/lib/thumb/v6-m/nofp/libc.a")) {
    throw 'GNU Arm Toolchain must provide one ARMv6-M libgcc and a matching newlib.'
}

$version = $gccRuntimes[0].Name
$gccLib = $gccRuntimes[0].FullName.Replace('\', '/') + '/thumb/v6-m/nofp'
$cxxInclude = "$sysroot/include/c++/$version"
$commonFlags = "--target=armv6m-none-eabi -march=armv6m -mfloat-abi=soft --sysroot `"$sysroot`""
$cxxFlags = "$commonFlags -isystem `"$cxxInclude`" -isystem `"$cxxInclude/arm-none-eabi/thumb/v6-m/nofp`" -isystem `"$cxxInclude/arm-none-eabi`""
$linkerFlags = "-nostdlib -L`"$sysroot/lib/thumb/v6-m/nofp`" -L`"$gccLib`""
$buildDir = Join-Path $PSScriptRoot 'build/windows-clang'

$previousPath = $env:Path
$previousToolchainPath = $env:PICO_TOOLCHAIN_PATH
try {
    $env:Path = "$LlvmRoot/bin;$env:Path"
    $env:PICO_TOOLCHAIN_PATH = $LlvmRoot
    $configuration = @(
        '-S', $PSScriptRoot, '-B', $buildDir, '-G', 'Ninja',
        '-DCMAKE_BUILD_TYPE=Release',
        '-DPICO_COMPILER=pico_arm_cortex_m0plus_clang',
        "-DPICO_COMPILER_SYSROOT=$sysroot",
        '-DFRWR_CLANG22_COMPAT=ON',
        '-DCMAKE_TRY_COMPILE_PLATFORM_VARIABLES=PICO_COMPILER_SYSROOT',
        '-DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY',
        "-DCMAKE_C_FLAGS=$commonFlags",
        "-DCMAKE_CXX_FLAGS=$cxxFlags",
        "-DCMAKE_EXE_LINKER_FLAGS=$linkerFlags",
        '-DCMAKE_CXX_STANDARD_LIBRARIES=-Wl,--start-group -lc -lm -lgcc -lnosys -Wl,--end-group'
    )
    & cmake @configuration
    if ($LASTEXITCODE -ne 0) { throw 'CMake configuration failed.' }

    & cmake --build $buildDir
    if ($LASTEXITCODE -ne 0) { throw 'Firmware build failed.' }

    $uf2 = Join-Path $buildDir 'frwr.uf2'
    if (-not (Test-Path -LiteralPath $uf2 -PathType Leaf)) { throw 'Build did not produce frwr.uf2.' }
    Get-Item -LiteralPath $uf2
} finally {
    $env:Path = $previousPath
    $env:PICO_TOOLCHAIN_PATH = $previousToolchainPath
}
