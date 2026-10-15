# MirroLeap 独立仓库迁移说明

## 新的官方开发地址

**https://github.com/slobys/MirroLeap**

MirroLeap 现作为一个独立的公开 GitHub 仓库维护，不再作为 GitHub 平台上的 fork 展示。它仍然是基于 [kil0bit-kb/scrcpy-gui](https://github.com/kil0bit-kb/scrcpy-gui) 二次开发的衍生开源软件，并采用 [Genymobile/scrcpy](https://github.com/Genymobile/scrcpy) 作为投屏核心；取消 GitHub fork 关联**不代表取消原项目版权、署名或许可证要求**。

## 迁移内容

- 迁移了原 [slobys/scrcpy-gui](https://github.com/slobys/scrcpy-gui) 仓库的完整 Git 分支、标签与其可达提交历史，迁移后原提交哈希不变。
- 将历史 14 个 GitHub Release 的安装包等 37 个附件、Release 名称、说明和预发布标记复制到新仓库，文件内容 SHA-256 与原发布一致；旧版文件仍保留其原有 ScrcpyGUI 命名，用于正确区分历史版本。
- 所有旧版均保留原有标签和文件，新的第一个独立仓库构建版本为 **MirroLeap v4.6.2**，从 [MirroLeap Releases](https://github.com/slobys/MirroLeap/releases/latest) 下载。
- 更新了 README、安装指南、应用内的项目链接、Nix 项目主页和 GitHub Actions 发布相关配置。

## 不随 Git 自动迁移的内容

GitHub 的 Pull Request 编号、审查讨论、Issue 评论、Stars、Watchers、原 Release 的上传/发布时间、Action 历史日志与仓库 Secrets 不是 Git 提交的一部分，无法通过 `git push` 原样复制。原 [slobys/scrcpy-gui](https://github.com/slobys/scrcpy-gui) 保留，供查询这些历史记录。新仓库的 Release 在 GitHub 显示的是迁移后的发布时间。

软件仍使用原有的 Tauri 内部应用标识与本地配置存储键，以便兼容已安装用户；仓库地址的调整**不会自动修改用户电脑上的 Git remote 或旧版安装路径**。

## 更新你电脑上的本地代码仓库

如果此前已从老仓库克隆：

```bash
git remote set-url origin https://github.com/slobys/MirroLeap.git
git fetch --prune origin
git remote -v
```

或者重新克隆：

```bash
git clone https://github.com/slobys/MirroLeap.git
```

## 维护与授权

本仓库独立维护，但保留原项目和 scrcpy 的版权及开源协议声明。详细信息参见 [README](README.md) 与 [MIT License](LICENSE)。
