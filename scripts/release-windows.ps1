param([string]$Destination = 'C:\gitroot\ut\vzeditor')

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$dest = [IO.Path]::GetFullPath($Destination)
$exe = Join-Path $dest 'vz.exe'
$id = [Guid]::NewGuid().ToString('N')

function Get-InstalledVz {
    Get-Process -Name vz -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
}

function Invoke-VzReleaseCommand([string]$Binary, [string[]]$Arguments) {
    $log = [IO.Path]::GetTempFileName()
    try {
        # 復元の子ウィンドウが閉じるまで待たない。コマンドを実行する親だけを待つ。
        $process = Start-Process -FilePath $Binary -ArgumentList $Arguments -WindowStyle Hidden -PassThru -RedirectStandardError $log
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) {
            $message = Get-Content -LiteralPath $log -Raw -Encoding UTF8
            throw "VZ のリリースコマンドが失敗しました ($($process.ExitCode)) $message"
        }
    } finally {
        Remove-Item -LiteralPath $log -ErrorAction SilentlyContinue
    }
}

Push-Location -LiteralPath $repo
try {
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    cargo build --locked --release --bin vz
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    $source = (Get-Item -LiteralPath 'target/release/vz.exe').FullName
    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    $lock = [IO.File]::Open((Join-Path $dest '.vz-release.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
    $pending = Join-Path $dest ".vz-$id.tmp.exe"
    try {
        Copy-Item -LiteralPath $source -Destination $pending
        $hash = (Get-FileHash -LiteralPath $source).Hash
        if ((Get-FileHash -LiteralPath $pending).Hash -ne $hash) { throw '一時ファイルの照合に失敗しました' }

        $running = @(Get-InstalledVz)
        if ($running.Count -gt 0) {
            Write-Host "リリース ID $id / 保存対象 $($running.Count) プロセス"
            [string[]]$arguments = @('--release-save-and-quit', $id) + @($running | ForEach-Object { $_.Id.ToString() })
            foreach ($process in $running) { $null = $process.Handle }
            Invoke-VzReleaseCommand $pending $arguments
            $deadline = [DateTime]::UtcNow.AddSeconds(60)
            foreach ($process in $running) {
                $remaining = [Math]::Max(0, ($deadline - [DateTime]::UtcNow).TotalMilliseconds)
                if (-not $process.WaitForExit([int]$remaining)) { throw "PID $($process.Id) が終了していません" }
            }
        }
        if (Get-InstalledVz) { throw '配置先から新しく起動された VZ があります' }
        if (Test-Path -LiteralPath $exe) {
            [IO.File]::Replace($pending, $exe, [NullString]::Value)
        } else {
            Move-Item -LiteralPath $pending -Destination $exe
        }
        if ((Get-FileHash -LiteralPath $exe).Hash -ne $hash) { throw '配置先の照合に失敗しました' }
        if ($running.Count -gt 0) {
            Invoke-VzReleaseCommand $exe @('--release-restore', $id)
        }

        foreach ($file in 'LICENSE', 'README.md') {
            $to = Join-Path $dest $file
            if (-not (Test-Path -LiteralPath $to) -or (Get-FileHash -LiteralPath $file).Hash -ne (Get-FileHash -LiteralPath $to).Hash) {
                Copy-Item -LiteralPath $file -Destination $to -Force
            }
        }
        Write-Output "配置済み: $exe"
    } finally {
        Remove-Item -LiteralPath $pending -ErrorAction SilentlyContinue
        $lock.Dispose()
    }
} finally {
    Pop-Location
}
