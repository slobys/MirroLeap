# MirroCreator HQ：scrcpy 原始解码帧旁路 · Stage 1

**状态：实验性 C/Rust 传输原型已实现；Windows GPU 高画质镜头尚未接入，当前软件不会自动启用 HQ。**

本阶段保留默认 **2.35×**。当前 Windows Magnification.dll 仍放大桌面 SDL 投屏画面，尚不能等同于真正的高分辨率解码帧放大。不要将本分支打包宣传为“高清 2.35 倍已经实现”。

## 为什么要补丁而不是改设置

MirroLeap 以外部 scrcpy.exe 投屏。用户选择 2K 只表示手机编码输出上限；当 SDL 把该视频压缩显示在一个较小的窗口上后，Windows Magnification 无法恢复已经丢失的像素。

scrcpy 客户端在 libavcodec 解码后使用 frame_source 向 screen.frame_sink 发送 AVFrame。补丁只给 **同一条视频解码流**增加第二个可选 sink，数据在 SDL 缩放前取得，手机不需要第二个录屏进程。

固定研究基线：Genymobile/scrcpy **v4.0**、commit `2322868e9e256eb5fce0b3d659ab2a409f29bae1`，原仓库 Apache-2.0。新增的 MirroLeap sink/协议文件 MIT；分发修改后的 scrcpy 客户端需同时遵守上游 Apache-2.0 版权/声明、二进制依赖许可证和源码提供义务。

## 当前真实代码与测试

- `include/mirroleap_hq_protocol.h`：64-byte 全局头、两个 80-byte 槽头、固定最多 3840×2160 的紧凑 YUV420P/NV12，提交序列号和最新槽位索引。
- `scrcpy/mirroleap_hq_sink.{h,c}`：可选视频帧 sink、Windows OpenFileMapping、轻量 av_frame_clone、独立工作线程复制最新帧；覆写未处理的旧帧以控制延迟。遇到无效帧、找不到映射、版本不符等情况不会让正常 scrcpy 解码/显示退出。
- `../src-tauri/src/hq_protocol.rs`：跨平台协议与像素尺寸校验；拒绝未提交、超界、错误格式或不规范平面布局。
- `../src-tauri/src/hq_bridge_windows.rs`：Windows BCrypt 随机会话 token 与映射 owner、按原子序列号检查读取最新帧；未与默认投屏生命周期相连。
- `apply_scrcpy_v4.py`：仅接受固定 git 提交，安全改动 app/meson.build 和 app/src/scrcpy.c，再安装新增 sink 源文件。重复执行不插入两次，拒绝未知上游版本。
- `tests/test_sink.c`：通过仅供 CI 使用的 Windows/FFmpeg API stubs 编译并实际执行 YUV420P/NV12 打包、填充 stride、PTS、双槽提交、无效格式回退；**这些 stubs 不能证明 Windows FFmpeg/Sdl3 官方构建可通过**。

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

## 共享内存安全与生命周期

必须由 MirroLeap **先**在 Windows 当前登录会话中创建 `Local\\MirroLeapHQ_<32位随机hex>`，然后显式传给经过版本验证的自定义 scrcpy 子进程：

```text
MIRROLEAP_HQ_TOKEN=<128-bit CSPRNG hex>
```

**绝对不能**把 token 放进 UI、设置文件、共享日志或 CLI 参数。映射所有权随着用户会话消亡。其他 scrcpy/手机进程没有此 token 时，HQ sink 不启用。用户没有选择高清镜头时，不应分配 25MB 映射，不应更改现有 SDL 视频处理。

两个槽最多约 24.9MB。读端先 Acquire 读取槽序列号（偶数非零），复制头部和相应像素，再 Acquire 重读序列号；若不一致直接丢弃这张帧。此机制支持 worker 同时更新下一个槽，不占用 SDL 绘制锁。

限制：只支持 CPU 可读的 YUV420P 和 NV12，硬解码 GPU surface、其他 pixel format、奇数尺寸或超过 4K 的帧均**跳过 HQ 旁路**。生产者不转换颜色空间、不做 GPU→CPU 下载；后续需要独立验证 D3D11 共享纹理方案。

## 下一阶段不可跳过的验收

1. **Windows scrcpy 客户端原生编译**：在固定 scrcpy 4.0 MSYS2/FFmpeg/SDL3 环境中构建修改后 EXE，独立于当前官方核心存放，核验 Windows 版权/签名及是否能正常退出。当前只完成 Linux stub C 语法及生产者打包测试，不能宣称已得到可安装的自定义客户端。
2. **端到端 IPC**：Tauri 按需创建映射并传 env，验证真正自定义客户端从一台手机提供 1080P/2K 实时解码帧、PTS 和会话隔离，不允许影响任何未开启 HQ 的设备。
3. **GPU HQ 镜头**：独立 Windows D3D11 GPU 渲染源帧采样区域，保持 2.35 倍/滚轮大小、旋转与 DPI 映射，和旧桌面放大镜作 A/B 对比。无有效 HQ 帧时回退 Windows 原生镜头，但不谎称源像素无损。
4. **长测**：USB/Wi-Fi 30/60fps、H264/H265、双设备、视频运动、旋转、最小化、DRM、Windows 100/125/150% DPI，记录实际 CPU/GPU、掉帧、延迟；完全通过后再考虑预览包。

更多设计及安全边界见 [`MIRROCREATOR_HQ_RENDERING_CN.md`](../MIRROCREATOR_HQ_RENDERING_CN.md)。
