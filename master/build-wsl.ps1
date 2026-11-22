param([ValidateSet(115200,230400,460800,921600,1000000)][int]$Baud=921600)
$ErrorActionPreference='Stop'
if (-not $env:PICO_SDK_PATH) { throw 'Set PICO_SDK_PATH to Pico SDK 2.2.0 (Windows path)' }
$taskProject=(& wsl.exe --exec wslpath -a -u $PSScriptRoot).Trim()
$taskSdk=(& wsl.exe --exec wslpath -a -u $env:PICO_SDK_PATH).Trim()
wsl.exe --exec cmake -S $taskProject -B "$taskProject/build-wsl" -G Ninja -DCMAKE_BUILD_TYPE=Release "-DPICO_SDK_PATH=$taskSdk" "-DKBM_UART_BAUD=$Baud"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
wsl.exe --exec cmake --build "$taskProject/build-wsl"
exit $LASTEXITCODE
