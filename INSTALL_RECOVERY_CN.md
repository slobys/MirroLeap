# MirroLeap / 原 ScrcpyGUI Windows 安装恢复指南

此说明针对 ScrcpyGUI v4.5.x 安装时遇到：

- **NSIS Error — Error launching installer**
- **Already Installed → Unable to uninstall!**

这两种错误不等于同一种原因。后者说明新 NSIS 安装器在升级时尝试调用 Windows 中的**旧版卸载程序**，但没有成功。可能是旧版卸载程序被删除、注册记录指向不存在的路径、安装程序正在运行或系统安全机制阻止执行。只有在获得安装日志前，不能断定是哪一种。

## 方案一：优先尝试保留旧版直接覆盖（无需卸载）

1. 完全关闭 ScrcpyGUI 和 scrcpy 投屏窗口；如有需要，重启 Windows 后再安装。
2. 只从 [MirroLeap Releases](https://github.com/slobys/MirroLeap/releases) 下载可信的安装包，并核对同一 Release 的 `SHA256SUMS.txt`。
3. 在出现 **Already Installed** 页面时，选择 **Do not uninstall**（不要先卸载），而非默认的 **Uninstall before installing**。
4. 继续安装到原目录，完成后打开 ScrcpyGUI 检查版本与已有配置。
5. 如果仍显示 `Error launching installer`，停止尝试，不要添加 `/NCRC` 参数跳过校验。

此方法对许多升级问题有效，但旧的 `uninstall.exe` 若真的损坏，仍可能出现其它错误。

## 方案二：免安装 ZIP（原 ScrcpyGUI v4.5.1 起提供，建议优先采用）

MirroLeap v4.6.4 Release 选择 **`MirroLeap_4.6.4_x64_Portable.zip`**，而不是 NSIS 安装用的 setup.exe：

1. 整个 ZIP 解压到可写目录，例如 `D:\Apps\MirroLeap-Portable\`。
2. 双击 `Run-MirroLeap.cmd` 或 `mirroleap.exe`。
3. 此方法**不执行安装器、不触碰旧版卸载记录，也不会自动删除旧软件**。
4. 如果提示没找到 scrcpy 核心，点击 GUI 内的核心下载；也可以把之前安装目录里完整且可信的 `scrcpy-bin` 文件夹复制到便携版 EXE 同级目录，例如：
   ```text
   D:\Apps\MirroLeap-Portable\
     mirroleap.exe
     Run-MirroLeap.cmd
     scrcpy-bin\
       scrcpy.exe
       adb.exe
       scrcpy-server
       其他 DLL...
   ```
5. **不要从 ZIP 压缩包的预览窗口直接运行 EXE**，必须先全部解压。尽量不要放在 `C:\Program Files` 等需要管理员权限的目录。

原有设置和核心通常位于原来的安装目录/应用数据文件夹中。请在任何卸载、清理操作之前备份，例如：

```text
%LOCALAPPDATA%\ScrcpyGUI\scrcpy-bin
```

只有在确定该目录真实存在且是你需要备份的文件后才进行复制，**不要删除整个 `%LOCALAPPDATA%\ScrcpyGUI`**。

## 如果希望彻底修复“旧版无法卸载”

按照微软官方支持渠道：

1. Windows **设置 → 应用 → 已安装的应用 → ScrcpyGUI → 卸载**；
2. 如果依旧出错，尝试 Windows **控制面板 → 程序和功能**；
3. 对损坏的 NSIS 卸载记录，参考微软官方 [无法安装或删除程序的修复指引](https://support.microsoft.com/en-us/windows/deployment/install-upgrade/fix-problems-that-block-programs-from-being-installed-or-removed)。不要使用来源不明的“强制卸载”注册表脚本。

请不要删除不熟悉的注册表项或其它应用数据；特别不要关闭 Windows Defender、SmartScreen、Smart App Control 来绕过安装失败。

## 校验安装包是否完整

v4.6.4 的 Release 同时包含 `SHA256SUMS.txt`，它列出 NSIS EXE、MSI、免安装 ZIP 的 SHA-256。Windows PowerShell 中运行：

```powershell
Get-FileHash -LiteralPath '.\MirroLeap_4.6.4_x64_Portable.zip' -Algorithm SHA256
```

将输出与下载自同一个 Release 的 `SHA256SUMS.txt` 对应文件名的哈希对比。如果不匹配，删除文件重新从官方 GitHub 仓库下载，不要尝试跳过校验。

**文件哈希一致不代表软件已经通过 Microsoft 数字签名认证。** 仓库如果尚未配置有效 CA 证书，便携版内部 EXE 和安装包仍可能触发 SmartScreen。真正减少此类安全警告，需要使用受信任的代码签名与稳定的发布者身份。

## 构建验证与已知限制

GitHub Actions 会先用 Tauri 的 `--no-bundle` 构建独立原生 EXE，复制到便携包 staging，再构建 MSI/NSIS。只有在校验三种文件和 SHA256 后，才上传发布。自动化也会检查 ZIP 中至少有主程序、启动脚本和说明文档。

这能规避依赖 **旧版卸载程序** 的升级失败，但不能从远程直接修复你电脑上损坏的卸载注册记录，也不能替代真实电脑安装、启动与 WebView2 环境测试。
