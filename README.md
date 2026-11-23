# MirroLeap

MirroLeap 可以把安卓手机、iPhone 和 iPad 的画面投到 Windows 电脑。投屏时可以用画笔标注、打开聚光灯或放大画面。安卓投屏还支持用电脑键鼠操作手机、截图和录屏。

[下载 Windows 版](https://github.com/slobys/MirroLeap/releases/latest) · [查看版本更新](https://github.com/slobys/MirroLeap/releases)

## 下载和打开

适用于 Windows 10/11 x64，需要 WebView2 运行时。在发布页的 **Assets**（下载文件）里选一种即可。以下以 v5.1.4 为例。

| 下载方式 | 文件名示例 | 怎么打开 |
| --- | --- | --- |
| **免安装版（推荐）** | `MirroLeap_5.1.4_x64_Portable.zip` | 完整解压，再双击 `Run-MirroLeap.cmd` |
| EXE 安装版 | `MirroLeap_5.1.4_x64-setup.exe` | 双击后按提示安装 |
| MSI 安装版 | `MirroLeap_5.1.4_x64_en-US.msi` | 用 Windows Installer 安装 |

安卓投屏首次使用时，如果软件提示缺少 scrcpy，按界面提示下载。发布页的 `SHA256SUMS.txt` 可以用来核对下载文件。

## 连接设备

### 安卓

- **USB 有线**。在手机的「开发者选项」里打开「USB 调试」，用数据线连接电脑，并在手机上确认授权。在 MirroLeap 首页选择设备，再点「开始投屏」。
- **Wi-Fi 无线**。手机和电脑连到同一个局域网。在手机上开启「无线调试」，然后在 MirroLeap 点击「无线连接 / 配对设备」，按提示完成配对。

在首页选择「分辨率」即可调整画质。旧版的「场景预设」已并入这个入口。

### iPhone 和 iPad

使用 v5.4.1 或更新版本，在首页找到「iPhone / iPad 投屏」。

- **USB 有线**。用数据线连接电脑，解锁设备。看到「信任此电脑」时点「信任」。在 MirroLeap 点「USB投屏」，如需 Apple Devices 或 USB 驱动，按界面提示安装。
- **Wi-Fi 无线**。设备和电脑连到同一个局域网。在 MirroLeap 安装并启动 iOS 接收器，再在设备的「控制中心 → 屏幕镜像」里选择首页显示的接收器名称。

[查看 iPhone / iPad 投屏步骤](https://github.com/slobys/MirroLeap/blob/714bae65d62b63c0e5800ade2f5f41ece5ae918a/IOS_AIRPLAY_CN.md)

## 常用功能和帮助

- **标注和演示**。在投屏窗口旁的悬浮栏打开画笔、聚光灯或放大镜。scrcpy 原始录像记录的是安卓设备画面。录制演示时，请先确认画笔和遮挡效果是否出现在录屏里。 [画笔和演示工具](IMMERSIVE_CONTROLS_CN.md)
- **安装、升级或卸载遇到问题**。可先用免安装版启动。 [安装恢复指南](INSTALL_RECOVERY_CN.md)
- **查看日志或分享诊断报告**。打开「设置 → 工具与日志」。 [安全与隐私说明](SECURITY_CN.md)

## 关于项目

MirroLeap 由 slobys 独立维护，基于 scrcpy-gui 二次开发，安卓投屏使用 Genymobile/scrcpy。 [原始项目](https://github.com/kil0bit-kb/scrcpy-gui) · [投屏核心](https://github.com/Genymobile/scrcpy) · [项目迁移说明](MIGRATION_CN.md)

主项目采用 MIT 许可证。iOS 接收和捕获组件的许可及源码出处见投屏指南。 [MIT 许可证](LICENSE) · [iOS 组件说明](https://github.com/slobys/MirroLeap/blob/714bae65d62b63c0e5800ade2f5f41ece5ae918a/IOS_AIRPLAY_CN.md)
