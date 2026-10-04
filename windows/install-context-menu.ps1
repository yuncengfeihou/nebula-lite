<#
.SYNOPSIS
    在资源管理器的右键菜单里加上「用 nebula-lite 打开」。

.DESCRIPTION
    只写 **HKCU\Software\Classes**（当前用户），不需要管理员权限，也不碰任何
    需要提权的键。注册四类目标：

      *                      文件          → nebula-lite.exe "%1"
      Directory              文件夹        → nebula-lite.exe "%1"
      Directory\Background   文件夹空白处  → nebula-lite.exe "%V"
      Drive                  盘符          → nebula-lite.exe "%1"

    **Windows 11 的说明**：这些是"经典"式动词，会出现在右键菜单的「显示更多
    选项」里（或直接按 Shift+F10）。要让它出现在一级菜单上，得走打包成 MSIX
    稀疏包 / IExplorerCommand 那条路，本脚本不做。

    必须用 **pwsh（PowerShell 7）** 运行：本文件是 UTF-8 无 BOM，Windows
    PowerShell 5.1 会按 ANSI 读它，菜单里的中文会变成乱码。同目录的 .cmd
    已经帮你调好 pwsh 了。

.EXAMPLE
    pwsh -File .\install-context-menu.ps1
    pwsh -File .\install-context-menu.ps1 -ExePath "D:\tools\nebula-lite.exe"
#>
[CmdletBinding()]
param(
    # nebula-lite.exe 的路径。默认取本机 release 产物的位置。
    [string]$ExePath = (Join-Path $env:USERPROFILE '.nebula-lite\target\release\nebula-lite.exe')
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $ExePath -PathType Leaf)) {
    Write-Host "找不到 nebula-lite.exe：$ExePath" -ForegroundColor Red
    Write-Host '用 -ExePath 指定实际路径，例如：' -ForegroundColor Yellow
    Write-Host '  pwsh -File .\install-context-menu.ps1 -ExePath "D:\somewhere\nebula-lite.exe"'
    exit 1
}
# canonicalize：注册表里存的是**绝对**路径，将来 exe 挪了要重跑这个脚本。
$ExePath = (Resolve-Path -LiteralPath $ExePath).ProviderPath

# 每个条目：注册表子键（相对 HKCU\Software\Classes）、菜单标题、命令里的参数占位符。
$entries = @(
    @{ Sub = '*\shell\NebulaLite';                   Placeholder = '%1'; Label = '用 nebula-lite 打开' }
    @{ Sub = 'Directory\shell\NebulaLite';           Placeholder = '%1'; Label = '用 nebula-lite 打开' }
    @{ Sub = 'Directory\Background\shell\NebulaLite'; Placeholder = '%V'; Label = '用 nebula-lite 打开此文件夹' }
    @{ Sub = 'Drive\shell\NebulaLite';               Placeholder = '%1'; Label = '用 nebula-lite 打开' }
)

# 用 .NET 的注册表 API 而不是 `HKCU:\...` 驱动器路径：路径里那个 `*`（"所有文件
# 类型"那个键名）会被 PowerShell 的 provider 当成**通配符**，`Set-Item` 于是发现
# 多个匹配项、弹出"是否继续"的确认提示——非交互环境下就永久挂在那里（踩过：
# 脚本卡住、只建了第一个键的一半）。.NET API 没有通配符语义，键名就是字面量。
$base = [Microsoft.Win32.Registry]::CurrentUser

foreach ($entry in $entries) {
    $key = $base.CreateSubKey("Software\Classes\$($entry.Sub)")
    # 菜单标题 = 这个键的默认值（名字是空串）。
    $key.SetValue('', $entry.Label)
    # 图标取 exe 内嵌的那一枚（build.rs 编进去的资源 ID 1）。
    $key.SetValue('Icon', "`"$ExePath`",0")
    # 命令 = 子键 command 的默认值。
    $command = $key.CreateSubKey('command')
    $command.SetValue('', "`"$ExePath`" `"$($entry.Placeholder)`"")
    # 上面两个 LocalMachine/CurrentUser 句柄是托管对象，用完即弃；显式关掉免得
    # 注册表视图一直挂着（PowerShell 不保证及时回收）。
    $command.Close()
    $key.Close()
}

Write-Host '已注册（HKCU\Software\Classes）：' -ForegroundColor Green
foreach ($entry in $entries) {
    Write-Host ("  {0}  ->  `"{1}`" `"{2}`"" -f $entry.Sub, $ExePath, $entry.Placeholder)
}
Write-Host ''
Write-Host '在文件 / 文件夹上点右键即可看到「用 nebula-lite 打开」。' -ForegroundColor Gray
Write-Host 'Windows 11 上它在「显示更多选项」里（或按 Shift+F10）。' -ForegroundColor Gray
