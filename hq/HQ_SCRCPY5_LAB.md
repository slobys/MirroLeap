# MirroLeap · scrcpy 5.0.1 + D3D11VA 原始帧实验（独立研发分支）

**状态：开发实验；不替换已安装的 MirroLeap，不使用主应用更新通道。**

## 为什么更换底层方案

用户已实测：普通 scrcpy 5.0.1、Windows D3D11VA 投屏正常；勾选原始帧高清后切换到实验 scrcpy 4.0，整段视频明显卡顿且无法报告实际硬解。继续降低 scrcpy 4.0 的 CPU 拷贝不能修复底层解码引擎退化。

本实验固定上游 `Genymobile/scrcpy v5.0.1` commit
`a60891aea193d92e7e5c3942700eca63f9d19a5f`，**保留原来的
D3D11VA 解码器、FFmpeg 流、SDL 视频输出和 Android 服务器**，再附加一个只在随机共享内存 token 有效时运行的旁路 FrameSink。

## 现阶段的真实能力和限制

- 视频仍由 scrcpy 5.0.1 的原有 FFmpeg 硬解码器处理；D3D11VA 解码帧的 `AVFrame.data[0]` 是 GPU texture，不允许按普通 CPU YUV 指针访问。
- 视频主线程最多只是有间隔地 `av_frame_clone()` 做引用计数，不调用会造成等待的 `av_hwframe_transfer_data()`。
- 在 **后台** worker 中，只有放大镜明确处于活动状态时才接受最新帧，至多约每 125ms 对一个 GPU frame 执行 FFmpeg GPU→CPU 传输，然后输出和旧协议兼容的 NV12 原始帧到双槽共享内存。
- 代码同时支持软件 YUV420P/NV12；未启用 HQ 时不复制高分辨率图像。无法读取的 P010、非 NV12 或异常 GPU 帧跳过，不影响正常 scrcpy 的视频解码与显示。
- 目前**并非 GPU ROI 局部取样、跨进程共享 D3D11 纹理或零拷贝**。GPU 下载还是整帧、只是放在独立 worker 且限频；它可能造成 GPU 同步开销，不能保证视频 60FPS，也尚未直接接入 MirroLeap 的高清圆形窗口。
- **固定默认放大倍率 2.35×** 及原版 MirroLeap 均未修改。真正的硬件 GPU 高清镜头需后续以局部 ROI staging/readback 或共享资源进一步优化，避免 CPU 画圆圈。
- 没有实际 Android 设备的 GitHub CI 只能测试 C 协议、fake D3D11 AVFrame、Windows 原生编译与 `scrcpy --version`。不能由此宣称已解决用户手机上的卡顿。

## 代码

- `hq/apply_scrcpy_v5.py`：仅对固定 scrcpy 5.0.1 源码打可重复补丁，并校验上游精确 SHA。
- `hq/scrcpy/mirroleap_hq_sink_v5.c`：使用新枚举接口 `SC_SINK_OK=0` 的编译桥接；复用旧双槽协议，但 D3D11AVFrame 只在 worker 下载。
- `hq/tests/test_sink_v5.c`：验证 fake GPU 纹理不被当成 CPU YUV 访问、解码线程仅克隆少量帧、后台像素提交、未知格式跳过。
- `.github/workflows/hq-scrcpy5-lab.yml`：独立编译和测试，提供开发者测试 ZIP（GitHub Actions artifact），**不发布 MirroLeap 安装包**。

## 有真实安卓设备的 Windows 测试方法（人工验收）

在上面的 Windows CI 确认成功后，从本项目 GitHub Actions 对应运行的 Artifacts 取得 `scrcpy5-HQ-D3D11-lab-NOT-APP-INSTALLER`，校验 `SHA256SUMS.txt`，解压后在 PowerShell 执行：

```powershell
pwsh -NoProfile -File .\hq\windows-frame-probe.ps1 -SelfTest
pwsh -NoProfile -File .\hq\windows-frame-probe.ps1 `
  -PatchedScrcpy "C:\test\scrcpy5-hq\scrcpy.exe" `
  -Serial "YOUR_DEVICE_SERIAL" -D3D11 -Seconds 30
```

只统计视频帧宽高/PTS 与发布次数，不记录、保存或上传手机画面。测试时保持与普通 MirroLeap 相同分辨率和 Wi-Fi，在同一段持续播放的视频上比较：普通 FPS、CPU、GPU、延迟，以及添加旁路后是否仍可顺畅操作。若 D3D11 失败，记录日志并停止实验，默认 MirroLeap 不受影响。

**验收门槛：** (1) 真机 `Video decoding: d3d11va`；(2) video 不因旁路卡顿；(3) 原始 NV12 帧实际发布并正确映射；(4) 退出后无悬挂进程/映射；(5) 最终 GPU 局部取样、圆形合成画面实测清晰与顺滑。完成之前不得标注为可用的 GPU 高清版本。
