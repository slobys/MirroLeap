# MirroLeap HQ Preview · Windows 预览构建（非稳定版）

本预览安装包使用独立应用 ID `com.slobys.mirroleap-hq-preview` 和产品名 `MirroLeap HQ Preview`。不覆盖 MirroLeap 正式安装、不参与正式版一键更新；原正式版可继续使用。

## 本轮实际提供的能力

- 选择“实验性 2.35× 原始帧高清放大”后，下次启动普通 Windows 带边框投屏时，使用随安装包附带、以 scrcpy v4.0 固定源码补丁编译的实验性客户端。
- 通过客户端的 `--version` 标识先验证补丁，随后仅向该已验证子进程传递随机 128-bit 映射 token；默认投屏仍使用原来的 scrcpy，不会分配 HQ 共享内存。
- 点击投屏悬浮工具栏的“实时放大”后，如果同一投屏会话产生有效解码帧，使用原始 YUV420P/NV12 源像素通过 CPU 双线性取样，提交到原生、鼠标穿透的圆形 GDI layered window。
- 默认 2.35 倍，鼠标滚轮扩大/缩小镜头直径，Ctrl + 滚轮调整倍率；原有 Windows 实时放大镜作为无法获取原始帧时的回退。
- 停止投屏、切换会话、解码器/音频重试时清理或复位帧发布状态，隔离不同设备的映射，不上传/保存投屏帧。

## 重要限制

**这还不是最终 D3D11 GPU 高清镜头。** GDI/CPU 预览版只验证“同一视频流的解码帧→内存→Windows 可见圆形窗口”的架构连接，不承诺达到 60fps、低延迟、无模糊、低 CPU 占用或完美颜色空间表现。当前 Windows CI 不连接真实 Android，2K/1080P 抖音视频、旋转、多 DPI、USB/Wi-Fi 场景仍待真机验收；受 DRM/硬解像素格式等限制，可能自动使用原有放大镜。

这是 **GitHub Actions 社区构建**。提供 SHA256 文件校验，但不代表已取得受信任的 Windows Authenticode 证书或 SmartScreen 信誉，Windows 可能弹出来源警告；请自行确认下载来源，勿关闭系统安全功能。

## 文件和安装

- **EXE**：NSIS 预览安装器，单独安装。
- **MSI**：独立预览 MSI，二选一安装，避免重复安装。
- **SHA256SUMS.txt**：两份最终预览安装包的 SHA-256 校验。
- 安装后在“GPU 解码与性能”开启实验选项，然后开始 Windows 带边框普通投屏，并从手机悬浮栏打开“实时放大”。
- 如发现性能回退或不稳定，关闭 HQ 选项、停止并重新启动投屏即可使用原生实时放大镜；正式版完全不受影响。

修改的 scrcpy 客户端基于 Genymobile/scrcpy v4.0 固定 commit `2322868e9e256eb5fce0b3d659ab2a409f29bae1`。原项目及 FFmpeg/SDL3/ADB 等许可证、源代码见其发行项目；MirroLeap 旁路新增文件依照仓库许可发布。修改后的源文件和重现流程：`hq/apply_scrcpy_v4.py`、`hq/scrcpy/*`。
