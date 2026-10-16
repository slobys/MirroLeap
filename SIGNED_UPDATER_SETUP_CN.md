# MirroLeap 安全一键更新：发布配置与限制

目前 v5.0.2 正式版只具备安全的手动更新检查。它没有预装更新签名公钥，因此不能通过远程检查自己升级成签名自动更新客户端。下一次包含签名验证公钥的过渡版本必须先手动安装。

维护者在自己可信的设备上生成 Tauri updater minisign 密钥对，并在安全目录备份私钥和密码；严禁把私钥、密码上传到仓库、聊天、日志或发布附件。给 GitHub Actions 配置 TAURI_SIGNING_PRIVATE_KEY、TAURI_SIGNING_PRIVATE_KEY_PASSWORD 两个受保护 Secrets，以及 TAURI_UPDATER_PUBLIC_KEY 公钥变量。

发布管线必须在带固定公钥和固定 GitHub HTTPS 更新源的 signed-updates 构建中生成签名 NSIS updater 包、.sig 文件与最新版本的 latest.json；通过 Tauri 更新签名验证和实际 Windows 安装测试后，才可以启用 signed-updater capability。只下载 GitHub EXE 或验证 SHA256SUMS.txt 不构成签名更新。

当前仓库仍未完成密钥配置和签名产物流水线，所以默认编译严格关闭 signed-updates；用户仍可打开官方 Release 自行下载。免安装版只提供手动更新，避免 NSIS 覆盖 Portable 目录。完整自动安装必须经用户点击并确认，禁止静默提权或关闭 SmartScreen。

Windows Authenticode 发布者证书和 Tauri minisign 更新签名是两套不同机制。首次安装、未签名安装包和旧版本迁移仍可能触发 SmartScreen；不可声称启用 updater 就能消除 Windows 安全提示。
