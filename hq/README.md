# MirroCreator HQ：原始帧按需减负 · Stage 5（CPU/GDI，协议 v2）

**状态：feature 分支加入按需启停 v2 原始解码帧传输、CPU/GDI 绘制限频，以及实际解码方式的诚实诊断。默认放大 2.35×，放大圈未启用时不持续复制高分辨率 YUV 帧；仍能回退旧镜头。D3D11 GPU 镜头与真实 Android 视频性能验收尚未完成。**

默认 **2.35×**：接到有效解码帧时预览使用原始 YUV 像素，而不是已缩小的 SDL 桌面画面；无帧时回退 Windows Magnification.dll。当前不能宣传为 GPU 高清放大成品，真实 1080P/2K 视频的流畅性和画质尚待验收。

Windows 轻量级 MirroLeap 更新工作流：[`mirroleap-continuity.yml`](../.github/workflows/mirroleap-continuity.yml)，版本与安装包说明见 [`MIRROLEAP_RELEASE_NOTES.md`](MIRROLEAP_RELEASE_NOTES.md)。从 v5.2.8 起继续沿用 v5.2.3 的 MirroLeap 安装标识与签名更新通道；可选 HQ 实验核心单独下载，不再建立另一款软件。

## 为什么要补丁而不是改设置

MirroLeap 以外部 scrcpy.exe 投屏。用户选择 2K 只表示手机编码输出上限；当 SDL 把该视频压缩显示在一个较小的窗口上后，Windows Magnification 无法恢复已经丢失的像素。

scrcpy 客户端在 libavcodec 解码后使用 frame_source 向 screen.frame_sink 发送 AVFrame。补丁只给 **同一条视频解码流**增加第二个可选 sink，数据在 SDL 缩放前取得，手机不需要第二个录屏进程。

固定研究基线：Genymobile/scrcpy **v4.0**、commit `2322868e9e256eb5fce0b3d659ab2a409f29bae1`，原仓库 Apache-2.0。新增的 MirroLeap sink/协议文件 MIT；分发修改后的 scrcpy 客户端需同时遵守上游 Apache-2.0 版权/声明、二进制依赖许可证和源码提供义务。

## 当前真实代码与测试

- `include/mirroleap_hq_protocol.h`：64-byte 全局头、两个 80-byte 槽头、横竖方向均不超过 3840、总像素不超过 3840×2160 的紧凑 YUV420P/NV12，提交序列号和最新槽位索引。
- `scrcpy/mirroleap_hq_sink.{h,c}`：可选视频帧 sink、Windows OpenFileMapping、轻量 av_frame_clone、独立工作线程复制最新帧；覆写未处理的旧帧以控制延迟。遇到无效帧、找不到映射、版本不符等情况不会让正常 scrcpy 解码/显示退出。
- `../src-tauri/src/hq_protocol.rs`：跨平台协议与像素尺寸校验；拒绝未提交、超界、错误格式或不规范平面布局。
- `../src-tauri/src/hq_bridge_windows.rs`：Windows BCrypt 随机会话 token 与映射 owner、按原子序列号检查读取最新帧；仅与明确开启 HQ 的 Windows 带边框投屏会话连接；设备隔离、重试复位与发布代数避免误用旧帧。
- `../src-tauri/src/hq_geometry.rs`：根据真实 Windows 视频矩形（物理像素）、鼠标坐标与源 AVFrame 分辨率，以 **2.35×** 映射竖屏、横屏和 90°/180°/270° 旋转；圆圈在靠边 App 处不会选错源像素。
- `../src-tauri/src/hq_reference_render.rs`：采用 YUV420P/NV12 原始解码像素、CPU 双线性插值输出限定尺寸 BGRA 局部镜头，作为将来 GPU 镜头的**对照基准**，仅明确启用的实验模式会调用。颜色转换暂按 BT.709 limited-range 估算，不能在尚未获取真实 frame colorspace 元数据时当作颜色精准渲染。
- `../src-tauri/src/hq_pipeline.rs`：贯通“校验过的解码帧 → 源帧位置映射 → 2.35× 原始像素取样 → 限定尺寸 BGRA 画面”的**实验性参考管线**；从 C 生产者实际写出的 NV12 二进制帧进行跨语言测试，不通过已经被 SDL 缩放的桌面像素。尚未产生 Windows GPU 窗口，只在实验模式中运行。
- `../src-tauri/src/hq_cpu_lens_windows.rs`：**独立、默认关闭的实验性 Windows 显示窗口**，使用 GDI 32-bit DIB + `UpdateLayeredWindow` 显示来自原始解码帧的 BGRA 像素，圆形抗锯齿边缘、透明/鼠标穿透及退出释放资源。只在用户明确开启 HQ、启用实时放大且验证实验客户端后显示；不是 GPU 镜头，CPU 插值、逐帧缓存与颜色空间仍存在性能和画质限制。
- Windows mapping reader 现在同时读取**全局帧发布编号**和槽内原子提交序号。两个槽位的提交序号可以相同，过去仅靠槽序号识别新帧会漏掉每隔一帧。窗口也会在暂停帧时根据鼠标、倍率、圆圈大小的变化重新绘制。源端元数据损坏会隐藏旧画面而不是继续显示不可信内容。


- `apply_scrcpy_v4.py`：仅接受固定 git 提交，修改 app/meson.build、app/src/scrcpy.c 和 app/src/main.c 的 --version 实验标识，再安装 sink 源文件。重复执行不插入两次，拒绝未知上游版本。
- `tests/test_sink.c`：Linux CI 利用 Windows/FFmpeg API stubs 运行 YUV420P/NV12 打包、stride、PTS、双槽提交、无效格式回退，并把 C 写出的二进制帧交给 Rust 跨语言解析；**stubs 不能代替 Windows 真机渲染测试**。上游 scrcpy v4.0 已在 [Windows MSYS2 CI](https://github.com/slobys/MirroLeap/actions/runs/39473209338) 完成 FFmpeg/SDL3 真正编译及 `scrcpy.exe --version` 启动验证，但尚未连接真实 Android 设备传帧。

## 在本地复现补丁

```bash
git clone --depth 1 --branch v4.0 https://github.com/Genymobile/scrcpy.git /tmp/scrcpy-hq-experimental
python3 hq/apply_scrcpy_v4.py /tmp/scrcpy-hq-experimental
# 用该版官方要求的 FFmpeg + SDL3 + Meson Windows 依赖另行编译客户端。
# 普通用户继续使用官方 scrcpy.exe；不要替换 MirroLeap 正式版核心。
python3 scripts/check_hq_bridge.py
gcc -std=c11 -D_WIN32 -Wall -Wextra -Werror -Ihq/tests/stubs -Ihq/include \
  hq/tests/test_sink.c -o /tmp/hq-sink-test
/tmp/hq-sink-test
```

## 开发者可选端到端数据链诊断（不会接管正常 MirroLeap）

本次新增 `windows-frame-probe.ps1`，它只在开发者指定实验性 scrcpy 路径后：

1. 用 Windows CSPRNG 创建不可猜测的当前用户专用映射；
2. 只向当前测试 scrcpy **子进程环境变量**传递会话 token，不输出到日志；
3. 持续约 15 秒读取全局发布序号、槽位元数据和解码帧宽高/PTS，输出观察到的更新频率，**不记录图像、音频或账号内容**；
4. 结束后关闭测试客户端并销毁映射。未发现有效帧则提示失败，正常 MirroLeap 不受影响。

只可在有真实 Android 设备的 Windows 测试机器、使用已验证的实验性构建时运行：

```powershell
# 无需连接手机的 IPC 自检（GitHub Windows CI 自动执行）
pwsh -NoProfile -File hq/windows-frame-probe.ps1 -SelfTest

# 经独立 scrcpy HQ workflow 构建并确认来源的测试版 scrcpy.exe
pwsh -NoProfile -File hq/windows-frame-probe.ps1 `
  -PatchedScrcpy "C:\path\to\hq-test\scrcpy.exe" -Serial "DEVICE_SERIAL" -Seconds 15
```

上述输出仅能证明**原始解码帧确实抵达共享内存**，还不能证明 GPU 高清放大镜已经完成。

## 共享内存安全与生命周期

必须由 MirroLeap **先**在 Windows 当前登录会话中创建 `Local\\MirroLeapHQ_<32位随机hex>`，然后显式传给经过版本验证的自定义 scrcpy 子进程：

```text
MIRROLEAP_HQ_TOKEN=<128-bit CSPRNG hex>
```

**绝对不能**把 token 放进 UI、设置文件、共享日志或 CLI 参数。映射所有权随着用户会话消亡。其他 scrcpy/手机进程没有此 token 时，HQ sink 不启用。用户没有选择高清镜头时，不应分配 25MB 映射，不应更改现有 SDL 视频处理。

两个槽最多约 24.9MB。读端先 Acquire 读取槽序列号（偶数非零），复制头部和相应像素，再 Acquire 重读序列号；若不一致直接丢弃这张帧。此机制支持 worker 同时更新下一个槽，不占用 SDL 绘制锁。

限制：只支持 CPU 可读的 YUV420P 和 NV12，硬解码 GPU surface、其他 pixel format、奇数尺寸或超过 4K 的帧均**跳过 HQ 旁路**。生产者不转换颜色空间、不做 GPU→CPU 下载；后续需要独立验证 D3D11 共享纹理方案。

## 下一阶段不可跳过的验收

1. **Windows scrcpy 客户端原生编译**：在固定 scrcpy 4.0 MSYS2/FFmpeg/SDL3 环境中构建修改后 EXE，独立于当前官方核心存放，核验 Windows 版权/签名及是否能正常退出。目前已通过独立 [MSYS2 Windows 构建和客户端版本启动](https://github.com/slobys/MirroLeap/actions/runs/39473209338)，仍不能宣称完成手机实机解码流端到端验证，也不得自动替换用户的 scrcpy-bin。
2. **端到端 IPC**：Tauri 按需创建映射并传 env，验证真正自定义客户端从一台手机提供 1080P/2K 实时解码帧、PTS 和会话隔离，不允许影响任何未开启 HQ 的设备。代码现已接入显式投屏会话，但仍只有 C 测试帧→Rust 消费端跨语言验证，不能算 Android 实机验收。
3. **GPU HQ 镜头**：现有 GDI 圆形窗口已用于显式启用的在线 CPU 预览；下一步独立实现 Windows D3D11 GPU 渲染源帧采样区域，保持 2.35 倍/滚轮大小、旋转与 DPI 映射，和旧桌面放大镜作 A/B 对比。无有效 HQ 帧时回退 Windows 原生镜头，但不谎称源像素无损。
4. **长测**：USB/Wi-Fi 30/60fps、H264/H265、双设备、视频运动、旋转、最小化、DRM、Windows 100/125/150% DPI，记录实际 CPU/GPU、掉帧、延迟；完全通过后再考虑预览包。

更多设计及安全边界见 [`MIRROCREATOR_HQ_RENDERING_CN.md`](../MIRROCREATOR_HQ_RENDERING_CN.md)。
