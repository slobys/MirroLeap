<div align="center">

# MirroLeap

**轻盈投屏，专注当下**

基于 [scrcpy-gui](https://github.com/kil0bit-kb/scrcpy-gui) 二次开发的 Android 桌面投屏工具，使用 [scrcpy](https://github.com/Genymobile/scrcpy) 作为底层投屏引擎。

[下载最新版](https://github.com/slobys/MirroLeap/releases/latest) · [更新日志](https://github.com/slobys/MirroLeap/releases) · [安装问题指南](INSTALL_RECOVERY_CN.md)

</div>

## ✨ 功能特点

- **精简首页**：连接手机、选择分辨率、开关声音与真机边框、一键开始/停止投屏。
- **无线与 USB**：支持 USB 投屏、无线调试配对、设备记忆与重连。
- **手机外观与画笔**：竖屏/横屏真机边框、悬浮控制栏、标注画笔、橡皮擦、撤销、一键恢复。
- **悬浮栏防越界（v4.6.1）**：始终约束在手机投屏的可见上下范围、当前显示器工作区内，避开 Windows 任务栏；短横屏时自动切换双排按钮，避免画笔和关闭键被遮挡。
- **场景预设（v4.6.3）**：均衡、演示、流畅、高清四种一键预设，只改变下次投屏的分辨率、帧率与码率，保留设备、音频、录制和手机边框选项。
- **安全加固（v4.6.3）**：逐项更新有安全公告的前端依赖、添加持续审计；启动页/悬浮栏的 Tauri 权限与主窗口隔离，限制 CSP、报告文件名和终端命令，诊断数据默认排除设备地址与本地路径。
- **白屏修复（v4.6.4）**：恢复与 Windows WebView2 兼容的启动策略，增加不依赖 React 的故障提示，并在 CI 实际启动打包程序验证界面已渲染。v4.6.3 的 CSP 已临时回退，其他 IPC/导出/核心校验加固保留，详见 [安全说明](SECURITY_CN.md)。
- **界面防遮挡（v4.6.5）**：修复默认窗口和投屏运行后「开始/停止投屏」超出右侧卡片、覆盖「更多功能与快捷键」的问题。卡片高度按真实内容计算、底部保持独立可点，空间不足时整页滚动。Windows CI 使用 Edge 测量四种状态、四类窗口及 100%/150% 缩放的按钮边界，共 32 种场景。
- **专业模式放在设置里**：摄像头、虚拟显示器、编码器、帧率、图形渲染器、录屏、快捷键、日志等功能按类别整理，仍可使用。
- **兼容旧配置**：延续之前 ScrcpyGUI 的配置存储与内部应用标识，不要求重新配对已授权设备。

## 📥 下载与使用

适用于 **Windows 10/11 x64**，需要 WebView2 Runtime 与 Android 设备的 USB 调试/无线调试授权。

| 版本 | 文件名 | 适用场景 |
| --- | --- | --- |
| **免安装版（推荐）** | `MirroLeap_4.6.5_x64_Portable.zip` | 解压直接运行，绕开旧版卸载程序异常 |
| EXE 安装版 | `MirroLeap_4.6.5_x64-setup.exe` | 标准 Windows 安装 |
| MSI 安装版 | `MirroLeap_4.6.5_x64_en-US.msi` | 使用 Windows Installer 安装 |

从 **[MirroLeap GitHub Releases](https://github.com/slobys/MirroLeap/releases/latest)** 的 Assets 下载，不要从 Tags 页面下载源码 ZIP 当作安装包。

推荐直接下载 **Portable ZIP**，完整解压后双击 `Run-MirroLeap.cmd`（或 `mirroleap.exe`）。首次启动如提示缺少 scrcpy 引擎，可在软件里下载；已有引擎的用户可按 [免安装指引](INSTALL_RECOVERY_CN.md) 放置完整的 `scrcpy-bin`。

**基本操作：** 连接手机 → 授权 Android 调试 → 在首页选择设备 → 点击「开始投屏」。无线配对、码率/编码器和录屏等低频设置，从右上角的齿轮进入。

> 安全提示：发布文件提供 SHA-256 校验清单，但本项目目前尚未配置受信任的 Windows 代码签名证书，Windows SmartScreen 可能出现警告。哈希一致不等同于代码签名认证，请不要关闭 Windows 安全防护。

诊断报告与日志只保存到本地下载目录。导出内容会尽量过滤设备地址、私有路径和敏感选项，但分享前仍应自行检查。详见 [安全说明](SECURITY_CN.md)。

## 🧩 关于二次开发

MirroLeap 是基于 **[kil0bit-kb/scrcpy-gui](https://github.com/kil0bit-kb/scrcpy-gui)** 二次开发、现在以 **[slobys/MirroLeap](https://github.com/slobys/MirroLeap)** 独立仓库维护的开源项目。主要重新设计了日常交互、无线连接流程、投屏外框和标注操作，并完善 Windows 便携版与 Release 发布。独立仓库不改变原作者的署名或 MIT 授权。

**项目迁移说明：** 原先的 [slobys/scrcpy-gui](https://github.com/slobys/scrcpy-gui) fork 已完整迁移代码历史、分支、标签以及历史 Release 安装包；原仓库仍保留，便于查阅之前的 Pull Request 和开发记录。参见 [MIGRATION_CN.md](MIGRATION_CN.md)。

- 底层核心：[Genymobile/scrcpy](https://github.com/Genymobile/scrcpy)
- 桌面框架：[Tauri](https://tauri.app/) + React + Rust
- 技术与历史说明：[真机外框](DEVICE_FRAME_CN.md) · [标注/分辨率](ANNOTATION_RESOLUTION_CN.md) · [旧版安装恢复](INSTALL_RECOVERY_CN.md)

感谢原项目和 scrcpy 的作者及开源贡献者。遵循仓库 [MIT License](LICENSE)。MirroLeap 与 Genymobile、Android 设备厂商没有官方关联。

---
**Maintained as an independent community-developed derivative by slobys.** The original scrcpy-gui project and scrcpy engine retain their respective credits and licenses.
