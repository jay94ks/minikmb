param([switch]$Sanitize)
$ErrorActionPreference='Stop'
$taskRoot=(& wsl.exe --exec wslpath -a -u (Split-Path $PSScriptRoot)).Trim()
$taskSanitize=if($Sanitize){'ON'}else{'OFF'}
wsl.exe --exec cmake -S "$taskRoot/common/tests" -B "$taskRoot/build/host-tests" -DCMAKE_BUILD_TYPE=Debug "-DKBM_SANITIZE=$taskSanitize" "-DPICO_SDK_PATH=$((& wsl.exe --exec wslpath -a -u $env:PICO_SDK_PATH).Trim())"
if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
wsl.exe --exec cmake --build "$taskRoot/build/host-tests" --parallel
if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
wsl.exe --exec ctest --test-dir "$taskRoot/build/host-tests" --output-on-failure
exit $LASTEXITCODE
