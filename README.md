# <img src="src-tauri/icons/128x128.png" width="44" height="44" alt="MirroLeap 客户端图标"> MirroLeap

把安卓、iPhone 和 iPad 的画面投到 Windows。投屏时可以用画笔、聚光灯和放大镜。

**[下载 Windows 版](https://github.com/slobys/MirroLeap/releases/latest)** · Windows 10/11（64 位）

下载文件名以 `Portable.zip` 结尾的免安装版，解压后双击 `Run-MirroLeap.cmd`。

## 安卓投屏

- **USB**。手机开启「USB 调试」，插线后在手机上确认授权。
- **Wi-Fi**。手机和电脑连同一个网络。手机开启「无线调试」，电脑点「无线连接 / 配对设备」。

连好后，在电脑上选中手机，点「开始投屏」。

## iPhone / iPad 投屏

需要 MirroLeap v5.4.1 或更新版本。

- **USB**。插线并解锁，弹出提示时点「信任」，然后在电脑上点「USB投屏」。
- **Wi-Fi**。设备和电脑连同一个网络。电脑点「启动 iOS 投屏」，iPhone 或 iPad 打开「控制中心 → 屏幕镜像」，选电脑上显示的名称。

<details>
<summary>首次连接提示安装组件</summary>

- **USB**。按提示安装 Apple Devices 或 USB 驱动。装好驱动后重新插一次数据线。
- **Wi-Fi**。先点「安装 iOS 接收器」，装好后再点「启动 iOS 投屏」。

</details>

<details>
<summary>其他下载和帮助</summary>

- **下载文件示例（v5.1.4）**。免安装版 `MirroLeap_5.1.4_x64_Portable.zip`，EXE 安装版 `MirroLeap_5.1.4_x64-setup.exe`，MSI 安装版 `MirroLeap_5.1.4_x64_en-US.msi`。 [版本列表](https://github.com/slobys/MirroLeap/releases)
- **打开软件**。需要 WebView2。安卓投屏提示缺少 scrcpy 时，按软件提示下载。 [安装恢复指南](INSTALL_RECOVERY_CN.md)
- **画质和录屏**。安卓首页的「分辨率」包含旧版「场景预设」。scrcpy 原始录像记录安卓设备画面。 [画笔和演示工具](IMMERSIVE_CONTROLS_CN.md)
- **日志和连接问题**。在「设置 → 工具与日志」查看日志。 [安全与隐私说明](SECURITY_CN.md) · [iPhone / iPad 投屏排查和组件说明](https://github.com/slobys/MirroLeap/blob/714bae65d62b63c0e5800ade2f5f41ece5ae918a/IOS_AIRPLAY_CN.md)

</details>

MirroLeap 由 slobys 维护，基于 scrcpy-gui，安卓投屏采用 Genymobile/scrcpy。 [原始项目](https://github.com/kil0bit-kb/scrcpy-gui) · [投屏核心](https://github.com/Genymobile/scrcpy) · [项目迁移说明](MIGRATION_CN.md) · [MIT 许可证](LICENSE)
