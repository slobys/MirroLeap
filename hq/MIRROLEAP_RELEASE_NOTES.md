# MirroLeap v5.2.8 · 延续原版的一体化轻量更新

这不是另一款叫 “MirroLeap HQ Preview” 的程序。本版本沿用自 v5.2.3 以来的 **MirroLeap** 程序名称、Windows 安装身份 `com.kb.scrcpy-gui-v3`、配置目录和原有的签名更新通道。该版本作为 GitHub 测试版（Pre-release）发布，正式 Latest 仍为 v5.1.4，直到明确验收后再升级正式版。

## 你可以直接从 MirroLeap v5.2.3 更新

- v5.2.3 原程序会检索仓库中的 `vX.Y.Z` 版本，并检查是否有完整的 NSIS 安装包、minisign 签名与 `latest.json`。本次使用正常的 `v5.2.8` 标签和 `MirroLeap_5.2.8_x64-setup.exe` 文件名。
- 已安装的 NSIS EXE：启动时检测新版，弹窗由用户确认后，下载并验证固定公钥签名再安装；不是静默升级。
- MSI、便携 ZIP：可以收到新版提示，但出于安装方式安全边界，需要自行下载安装对应的新版本，不能被 NSIS 自动覆盖。
- 注意仅存在于旧 “HQ Preview” 独立程序中的旧标识不自动合并或卸载；原来的 MirroLeap v5.2.3 更新链完全延续。
- Windows SmartScreen 或来源提示仍可能存在：项目的 minisign 更新签名不是微软认可的 Authenticode 发布证书，请不要关闭系统安全保护。

## 小安装包，高清功能可选

- 主安装包 **没有**打入实验性 scrcpy 4.0、FFmpeg/SDL3 或 ADB；EXE 目标回归约 5 MB。仍旧保持 USB、Wi-Fi、手机边框、画笔、截屏与创作者功能。
- 平常投屏仍按原方式使用已经安装的 scrcpy 引擎，不在软件启动和普通投屏时下载可选 HQ 核心。
- 只有在设置中明确勾选“可选 2.35× 高清放大”时，软件才下载单独的 `MirroLeap.HQ.Core_v1_x64.zip`，比对编译时固定的 SHA256，再校验 ZIP 结构和 scrcpy 4.0 补丁标志后保存到个人应用数据目录。中断、文件损坏或校验失败不会替换正常 scrcpy。
- 可选 HQ 会使用同一个 Android 视频流中的原始解码帧，经双槽共享内存与 2.35 倍局部采样显示独立 Windows CPU/GDI 圆形镜头；无帧时保留原有桌面实时放大作为回退。
- 增强的点击光圈、较长的可见时间和真实桌面像素 CI 验证，以及自适应抗混叠和画质来源提示，也包含在此版 MirroLeap 内。

## 当前验收边界

Windows Actions 会执行 Rust/前端测试、实验 scrcpy 4.0 真正编译、WebView2 启动、桌面点击特效截图、签名一致性和安装包体积门槛。但在没有连接真实 Android 的 CI 中，仍无法宣称 **D3D11 GPU 高清镜头**已完成，也不能承诺 1080P/2K 抖音视频放大在实际设备上无锯齿、60fps 或更低延迟。当前 HQ 是可选 CPU/GDI 实验路径。

## 下载内容

- `MirroLeap_5.2.8_x64-setup.exe`：同名、同安装标识的轻量 NSIS 更新版，推荐。
- `MirroLeap_5.2.8_x64_en-US.msi`：MSI 安装版，后续仍手动更新。
- `MirroLeap_5.2.8_x64_Portable.zip`：免安装、解压运行。
- `MirroLeap.HQ.Core_v1_x64.zip`：**单独的可选核心**，普通用户不需下载；开启 HQ 后由软件自动安装。
- `latest.json` / `MirroLeap_5.2.8_x64-setup.exe.sig` / `SHA256SUMS.txt`：签名自动更新清单与文件完整性校验。

实验 scrcpy 部件固定 Genymobile/scrcpy v4.0、提交 `2322868e9e256eb5fce0b3d659ab2a409f29bae1`。补丁源码及相关依赖许可详见仓库 `hq/`。本版本在本机内处理帧，不上传手机画面。
