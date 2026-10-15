# MirroLeap v4.6.0 — 精简首页与原有功能归类

默认日常工作台只保留设备、分辨率、声音、真机边框、开始与停止投屏。其它选项通过右上角设置进入，包含连接配对、投屏参数、行为与快捷键、外观与引擎、工具与日志。保留原有 app identifier 和配置存储键，用于兼容旧版。

以下是历史版本开发记录。

---

# ScrcpyGUI v4.5.2 — 简化画笔与悬浮栏恢复

本轮针对 Windows 用户实测截图：

- 画笔展开为独立于移动、最小化和关闭按钮的紧凑面板。画笔、标准橡皮擦、撤销、清空、四色和细中粗笔触一次展示，不出现滚动条。
- 根据 Win32 实际 DPI 和剩余桌面边距自动选择左右展开或下方紧凑停靠；保留独立的“一键清除标注并恢复手机触控”按钮。
- 投屏最小化并从任务栏恢复后，重新跟踪 SDL 视频及两层 WebView2 的可见性、相对窗口层级和位置；临时 Win32 裁剪失败不会停止追踪。
- 新增对完整 HTML 工具条的交互测试、Rust 几何测试，并通过 Windows CI 构建。最终体验仍需在用户实际的 Windows 显示器、浏览器、不同 DPI 下验收。

---

# ScrcpyGUI v4.5 — 横屏、窗口恢复、悬浮栏与安装安全

本轮针对 Windows 用户实测的六个反馈进行修复。旧版和原生 scrcpy 功能继续保留。本项目是 Windows 桌面投屏 GUI，**不是 Android APK**。

## 功能变化

### 1. 安装安全与下载校验

GitHub Actions 生成的安装包过去没有 Windows 可验证的代码签名，容易触发 Defender SmartScreen 的「Windows 已保护你的电脑」/「未知发布者」警告。请注意，这和确实检测出恶意软件不是一回事，具体要看 Windows 弹窗内容。

**不采用**关闭 Defender、禁用 SmartScreen、忽略安全策略或伪造证书的办法。本版：
- 每次 Release 同时上传 `SHA256SUMS.txt`，可以在本地校验下载文件是否与 GitHub 构建一致。
- 增加条件签名与校验流程 `scripts/windows-sign-and-checksum.ps1`。只有仓库配置了**真实、受信任 CA 颁发的签名证书**，才会签名安装包并用 SignTool 验证后发布。
- 当前没有提供有效签名证书，安装包**仍是未签名**，SmartScreen 警告不会因本次代码修改自动消失。
- 正式分发优先考虑微软 Azure Artifact Signing（原 Trusted Signing）或微软商店。代码签名身份应与真实发布者一致，连续版本保持签名身份稳定。即使签名，也不保证新应用立即取得 SmartScreen 下载信誉。

如果你持有符合要求、可以安全导出的 PFX 证书，可把 `SCRCPY_SIGNING_PFX_BASE64`（PFX 文件的 Base64 内容）及 `SCRCPY_SIGNING_PFX_PASSWORD` 配置到 GitHub 仓库 **Settings → Secrets and variables → Actions**。工作流在 Runner 临时文件夹里完成签名，执行后删除临时证书，凭据不得写进代码或日志。现代证书常采用不可导出的硬件 / 云密钥；这类证书应按照发证机构流程，改用 Tauri 官方提供的 `bundle.windows.signCommand` / Azure Artifact Signing 集成，不应尝试导出私钥。

使用 PowerShell 核对文件哈希，例如：

```powershell
Get-FileHash .\ScrcpyGUI_4.5.0_x64-setup.exe -Algorithm SHA256
```

和 Release 里 `SHA256SUMS.txt` 的同名文件哈希比较。哈希一致证明文件与发布构建一致，**不等于数字签名或安全软件的独立安全认证**。

参考：[Microsoft Windows 代码签名选择](https://learn.microsoft.com/windows/apps/package-and-deploy/code-signing-options)、[微软 SmartScreen 信誉说明](https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation)、[Tauri Windows 签名](https://v2.tauri.app/distribute/sign/windows/)。

### 2. 画笔「一键恢复」

手机外框右侧有一个**独立的恢复按钮**（回转箭头），即使没有展开画笔工具，也可以点击。一次操作同时：
- 清空本次投屏画布上的全部笔迹。
- 退出标注模式。
- 恢复 scrcpy 原生鼠标点击、滑动、键盘输入。

原来的「撤销上一笔」和「只清空画笔但保留标注模式」继续保留。

### 3. 关闭与最小化放在一起

悬浮栏顶部固定为：**拖动、最小化、关闭**。下面另为「画笔、快速恢复」分组，最下方才是颜色、橡皮擦、粗细、撤销、清空工具。避免为了关闭投屏需要跨过整套画笔工具。

### 4. 最小化后恢复边框和悬浮栏

对 Android scrcpy SDL 窗口最小化后，从 Windows 任务栏或 `Alt+Tab` 恢复：
- 自动重新检测真实视频 HWND、透明外框 HWND 和工具栏 HWND 是否显示。
- 在恢复视频画面后，重新对齐外框并刷新 Windows 前后层级。
- 即使一次 Win32 定位调用发生瞬时错误，跟踪线程也会继续尝试，不直接把外框销毁。
- 鼠标移回手机外框边缘，悬浮控制栏应重新出现。

### 5. 横屏视频也显示手机外框

同一份窄黑钛 SVG 机身旋转 90° 作为 `iphone-black-landscape.svg`，对应 `2150×1000` 画板、`x18,y18,w2114,h964` 透明屏幕区域。手机视频由 scrcpy 实时渲染，安卓应用切换横竖屏后不用停止、重启或再次选择外框。

Rust 根据 SDL 当前客户区宽高自动判断：
- 竖屏使用 `1000×2150` 画板，横屏使用 `2150×1000`。
- 各自计算左右、上下的独立边距和圆角裁剪半径。
- 透明画笔 Canvas 同时调整绘制区域。旋转时为了避免原有笔迹位置错乱，会清空临时标注。
- 视频中的横屏播放，和用户主动将 scrcpy 配置为 Windows 独占全屏，属于不同机制。桌面窗口最大化时尽量跟随；**应用启动参数设置的 scrcpy 全屏模式仍不会启用装饰外框**，这是当前的已知限制。

### 6. 悬浮栏稍微放大

从约 **48px** 物理宽度增加到 **62px**，按键、图标及颜色点同步变大，普通悬浮栏高度约 258px，画笔工具展开后约 540px。使用当前 Windows 虚拟桌面范围进行定位，避免在右边缘或次屏越界。

## 必做真机验收

1. USB 或无线连接投屏，在竖屏下靠近左、右或顶部边框，确认悬浮栏正常出现、关闭/最小化在同一组。
2. 画几笔后单击「一键恢复」，确认笔迹立刻消失，之后能点击安卓应用。
3. 最小化后从任务栏恢复，确认黑钛外框显示、工具栏依然可以唤出和点击。
4. 在手机端打开横屏视频（包括应用的全屏视频），确认横屏外框出现、侧边按键位置自然，转回竖屏外框也恢复。
5. 使用 Windows 的 100%、125%、150% 显示缩放测试旋转后四角是否漏蓝线、圆角是否自然。
6. 下载 EXE/MSI 和 SHA256SUMS.txt 验证哈希；如仍出现安全警告，不要关闭 Windows 防护，请提供**警告窗口截图**以区分 SmartScreen、Smart App Control 或其它杀毒产品。
7. 运行前端测试、Rust 单元测试、Python SVG/界面检查，查看 Windows EXE/MSI 与 Release 上传结果。

本版本仍属于预览版。自动化验证无法替代真实 Windows 多显示器/DPI 和浏览器透明合成实验，不能宣称这些机器上的视觉问题已经全部解决。
