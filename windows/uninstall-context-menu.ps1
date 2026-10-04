<#
.SYNOPSIS
    卸载 install-context-menu.ps1 注册的资源管理器右键菜单项。

.DESCRIPTION
    只删 HKCU\Software\Classes 下自己写的那四个键（连同各自的 command 子键），
    不碰别人的菜单项。不需要管理员权限。用 pwsh（PowerShell 7）运行。

    与安装脚本同理，这里用 .NET 的注册表 API 而不是 `HKCU:\...` 驱动器路径：
    键名里的 `*` 会被 PowerShell provider 当成通配符（会弹确认框，非交互环境
    下直接挂住）。
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$subKeys = @(
    '*\shell\NebulaLite'
    'Directory\shell\NebulaLite'
    'Directory\Background\shell\NebulaLite'
    'Drive\shell\NebulaLite'
)

$base = [Microsoft.Win32.Registry]::CurrentUser

foreach ($sub in $subKeys) {
    $path = "Software\Classes\$sub"
    try {
        $base.DeleteSubKeyTree($path, $false)
        Write-Host "已删除 HKCU\$path" -ForegroundColor Green
    }
    catch [System.ArgumentException] {
        # 本来就不存在：不是错误，只是没什么可删的。
        Write-Host "本来就没有：HKCU\$path" -ForegroundColor DarkGray
    }
}
