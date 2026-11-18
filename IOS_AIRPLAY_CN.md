# MirroLeap iOS AirPlay 投屏指南（v5.4.0）

本版本保留原来的 Android scrcpy 投屏。另在首页提供「iOS AirPlay」入口，按需启动独立 Windows 接收器，让 iPhone/iPad 屏幕镜像到电脑。

## 首次使用

1. 在 MirroLeap 首页点击 **iOS AirPlay**。只有 Windows 10/11 x64 支持当前接收组件。
2. 点击 **下载安装 AirPlay 组件**。MirroLeap 此时才会访问 GitHub，下载固定版本 `moieric11/AirPlay-Windows v0.4.1`（约 12 MB），校验整个 ZIP 的 SHA-256，并在当前 Windows 用户数据目录安装。以后启动无需重复下载。
3. 点击 **启动 iOS 接收器**。它会打开自己的 Windows 窗口。首次使用时如 Windows 防火墙弹窗，允许信任的**专用网络**（不要在公共 Wi-Fi 中开放），必要时检查路由器是否启用 AP/访客隔离。
4. 将 iPhone/iPad 和电脑连接到同一个局域网。iOS 控制中心 → **屏幕镜像** → 选择 **AirPlay-Windows**。如果曾在接收器 OPTIONS 里修改名称，以实际显示的名称为准。
5. 停止镜像时在 iOS 控制中心选择「停止镜像」，也可以在 MirroLeap 中点击 **停止 iOS 接收器**。

## 支持范围与限制

- iOS 使用 AirPlay，不通过 ADB。**这不是安卓手机的远程触控功能**，不能用鼠标控制 iOS 系统。
- 初版 iOS 画面和音频由第三方接收器单独渲染；尚未与 MirroLeap 的 Android 真机边框、画笔、HQ 放大、GPU 性能面板和 scrcpy 录屏共享渲染管线。
- 接收器进程启动并不代表 iPhone 已连接；只有 iOS 侧选中设备并建立镜像后才会有画面。
- 需要局域网 mDNS 发现与设备互访。代理/VPN/访客网络、Windows 防火墙、AP 隔离可能导致看不到接收器。该版本不支持 iOS 系统的原生 USB 画面镜像。
- AirPlay 屏幕镜像不保证受版权保护视频可播放；不提供 FairPlay/Netflix/Apple TV+ 等 DRM 绕过。
- 不在 MirroLeap 启动时自动下载第三方组件、自动开启接收端口或启动镜像。

## 开源许可与安全

独立接收组件：[AirPlay-Windows](https://github.com/moieric11/AirPlay-Windows)（GPL-3.0），基于 UxPlay。该项目的 LICENSE、THIRD_PARTY_NOTICES.md 和 README.md 会从官方 ZIP 一并保存。MirroLeap 不重新打包这个组件到自己的 EXE/MSI 内，下载前由用户明确点击确认，主程序使用锁定的公开发布 SHA-256：

`5f1792a96efffe84b220edfa7286fa7670806226aa4d0b4b6c7e33ddd7747bf9`

固定下载地址：`https://github.com/moieric11/AirPlay-Windows/releases/download/v0.4.1/airplay-windows-v0.4.1-x64.zip`

该独立项目不是 Apple 官方软件。不要把接收器开放给公共/不可信网络。本版尚未完成全部 iOS 设备与显卡组合的实机验证。

## 后续

下一阶段考虑让 iOS 视频直接进入 MirroLeap 统一投屏窗口，再评估竖屏、横屏、分辨率、截图、录制和一致的悬浮工具栏。此阶段不将未验证的功能标记为已完成。
