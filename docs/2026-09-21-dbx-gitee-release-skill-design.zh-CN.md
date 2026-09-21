# DBX Gitee Release 打包与 Cursor Skill 设计

> 状态：已评审（对话确认 2026-09-21）  
> 范围：Windows NSIS 安装包 → Gitee 本仓库 Release；客户端改从 Gitee 解析更新清单。  
> 副本（gitignore）：`docs/superpowers/specs/2026-09-21-dbx-gitee-release-skill-design.md`

## 1. 背景与目标

### 1.1 现状

- 发版入口：`update.txt` 最后一行 `版本号 更新说明` → `scripts/update.ps1` / `update.bat`。
- 脚本同步 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 版本号，执行 `pnpm tauri build --bundles nsis`，复制安装包到 `release/`，经 SSH 上传到 nginx（`111.230.247.111/dbx/`）。
- Windows 客户端读取固定 URL：
  - `src-tauri/src/commands/update_installer.rs`：`UPDATE_FEED_URL = http://111.230.247.111/dbx/latest.json`
  - `apps/desktop/src/composables/useAppUpdater.ts`：`latestReleaseUrl = http://111.230.247.111/dbx/`

### 1.2 目标

1. **打包**并发布到 **Gitee 本 fork**（`zeki_baoruixin/dbx`）的 **Release**，上传 NSIS 安装包与 `latest.json`。
2. **不再依赖 nginx SSH 上传**作为 Windows 更新主路径（legacy 脚本可保留但标记废弃）。
3. 提供 **Cursor Agent Skill**，指导 Agent 安全、可重复地执行发版与校验。
4. **更新链接**：客户端与文档指向 Gitee Release / API，发版后无需每版修改 Rust 里的 manifest 直链常量（采用 Gitee API 解析最新 Release 附件）。

### 1.3 非目标

- macOS / Linux 安装包与上游 GitHub Release 工作流。
- 自动 `git commit` / `push` 源码（版本号同步后的提交由开发者自行完成）。
- Agent/JDBC 等其它产物的 Gitee 发布。

## 2. 决策摘要

| 项 | 决策 |
| --- | --- |
| Release 仓库 | Gitee `zeki_baoruixin/dbx`（与源码同仓） |
| 发布通道 | 仅 Gitee Release（不用 nginx） |
| 清单交付 | 每个 semver Release 上传附件 `latest.json` + 安装包 |
| 客户端拉清单 | **方案 A**：调用 Gitee API `releases/latest`，在 `attach_files` 中找 `latest.json` 再下载 |
| Git 操作 | 本地同步版本号、构建、打 tag、`git push origin <tag>`；不自动 commit/push 分支 |
| Tag 格式 | `v{semver}`，例如 `v0.6.3` |
| 认证 | 优先环境变量 **`GITEE_ACCESS_TOKEN`**；可选 `scripts/.gitee-release.env`（gitignore）作备用 |

## 3. 架构

```text
update.txt (最后一行)
       │
       ▼
publish-gitee-release.ps1  ──► 同步版本号 → NSIS 构建 → 生成 latest.json
       │                              │
       │                              ▼
       └── Gitee API v5 ◄──── 上传 exe + latest.json
                │
                ▼
       tag vX.Y.Z + git push tag
                │
                ▼
       客户端：GET releases/latest → 找 latest.json 附件 → 解析 url/sha256 → 下载 exe
```

### 3.1 Gitee 约束

- Gitee **无** GitHub 式稳定 URL：`/releases/latest/download/latest.json`。
- 因此客户端必须 **一次改造**：通过 `GET /api/v5/repos/{owner}/{repo}/releases/latest` 获取最新 Release，再在返回的附件列表中匹配文件名 `latest.json`（或约定字段），使用附件下载 URL 拉取 manifest。
- 公开仓库下该 API 通常可匿名读；若遇权限问题，文档中说明仓库 Release 可见性要求。

## 4. 发版脚本 `scripts/publish-gitee-release.ps1`

### 4.1 职责

复用 `update.ps1` 中已有逻辑（读 `update.txt`、`Sync-ProjectVersion`、`Find-Installer`、`Copy-LocalInstaller`），替换 `Publish-Installer`（SSH）为 Gitee Release 流程。

### 4.2 参数（建议）

| 参数 | 说明 |
| --- | --- |
| `-SkipBuild` | 跳过 `pnpm tauri build --bundles nsis` |
| `-SkipPublish` | 仅同步版本号/构建，不上传 Release |
| `-SkipTag` | 不上传 tag（仅本地构建与上传，用于调试） |
| `-NoPause` | 非交互结束时不等待 Enter |

### 4.3 环境变量

| 变量 | 必填 | 说明 |
| --- | --- | --- |
| `GITEE_ACCESS_TOKEN` | 是* | Gitee 私人令牌（创建 Release、上传附件） |
| `GITEE_OWNER` | 否 | 默认 `zeki_baoruixin` |
| `GITEE_REPO` | 否 | 默认 `dbx` |

\* 若未设置，脚本可读 `scripts/.gitee-release.env`（若存在）。

### 4.4 Gitee API 流程

1. **解析版本**：从 `update.txt` 得到 `version`、`notes`；`tag_name = "v$version"`。
2. **构建**（除非 `-SkipBuild`）：`pnpm tauri build --bundles nsis`。
3. **安装包路径**：`target/release/bundle/nsis/DBX_{version}_x64-setup.exe`（与现逻辑一致）。
4. **SHA256**：对 exe 计算小写 hex。
5. **创建或复用 Release**：
   - `POST /api/v5/repos/{owner}/{repo}/releases`，body 含 `tag_name`、`name`（如 `DBX v{version}`）、`body`（notes）、`target_commitish`（当前 HEAD）。
   - 若 tag/Release 已存在：默认 **失败退出**；可选后续增加 `-RecreateRelease`（非 v1 范围）。
6. **上传 exe**：`POST .../releases/{id}/attach_files`，multipart `file`；`access_token` 按 Gitee 要求（查询参数或 form，以实现时 Gitee 文档为准）。
7. **生成 `latest.json`**（与现 manifest 字段一致）：

   ```json
   {
     "version": "0.6.3",
     "notes": "...",
     "url": "<exe 的 browser_download_url>",
     "sha256": "<hex>",
     "silentArgs": "/S /UPDATE"
   }
   ```

   `url` 必须使用上传 exe 后 API 返回的 **附件直链**，不可预填占位 URL。

8. **上传 `latest.json`** 为第二个附件（同名覆盖遵循 Gitee 行为）。
9. **打 tag**：`git tag -a v{version} -m "..."`（若 tag 已存在则失败）；`git push origin v{version}`。
10. **校验**：
    - `GET releases/latest`：`tag_name` 或附件版本与本次一致；
    - 可下载 `latest.json` 且 `version`/`sha256`/`url` 与本地一致；
    - 对 `url` 做 HEAD/GET，Content-Length 与本地 exe 一致。

### 4.5 错误处理

- Token 缺失 → 明确提示设置 `GITEE_ACCESS_TOKEN`。
- 构建失败、找不到安装包 → 非零退出。
- 上传超时 → 有限重试（如 3 次，间隔递增）；大文件上传超时建议 ≥ 120s。
- 校验失败 → 非零退出，日志提示检查 Gitee Release 页面是否半成品。

### 4.6 与 `update.ps1` 关系

- `update.ps1` 在文档与注释中标记 **legacy（nginx）**；新发版默认文档指向 `publish-gitee-release.ps1` 或根目录包装 bat。
- 不删除 `upload-update.py` / `.update-server.env.example`，避免影响仍用 nginx 的旧环境。

## 5. 客户端与文档变更（方案 A）

### 5.1 Rust：`update_installer.rs`

- 移除对固定 `http://111.230.247.111/dbx/latest.json` 的单一 GET。
- 新增常量：`GITEE_OWNER`、`GITEE_REPO`（或单一 API URL 模板）。
- 步骤：
  1. `GET https://gitee.com/api/v5/repos/zeki_baoruixin/dbx/releases/latest`
  2. 在 JSON 的附件列表中查找 `name == "latest.json"`（字段名以实现时 API 响应为准）。
  3. 使用该附件的下载 URL GET manifest。
  4. 后续下载 exe、校验 sha256、静默安装逻辑不变。
- 更新/新增单元测试：mock API JSON，覆盖「无 latest.json 附件」「版本解析失败」等。

### 5.2 前端：`useAppUpdater.ts`

- `latestReleaseUrl` 改为 `https://gitee.com/zeki_baoruixin/dbx/releases`（手动打开 Release 列表页）。

### 5.3 文档与示例

- `docs/windows-installer-updates.md`：重写为 Gitee Release 流程；nginx 路径注明已废弃。
- `update-server/latest.json`：示例 URL 改为 Gitee 说明或删除误导性 IP（可选改为注释性 README）。

## 6. Cursor Skill

### 6.1 位置

```
.cursor/skills/dbx-gitee-release/
├── SKILL.md
└── (可选) reference.md — Gitee API 片段、故障排查
```

仓库内 Skill，与 fork 定制发版绑定；description 含触发词：Gitee 发版、打包 Release、update.txt 发布等。

### 6.2 Skill 工作流（Agent 必须遵守）

1. **前置检查**
   - 确认 `update.txt` 最后一行格式正确。
   - 确认 `GITEE_ACCESS_TOKEN` 已设置（可 `if (-not $env:GITEE_ACCESS_TOKEN) { ... }` 探测）。
   - 确认当前 branch 与 HEAD 为预期 commit（Skill 提醒：版本号同步后需用户自行 commit，或发版前已 commit）。
   - 检查远程 tag `v{version}` 是否已存在。

2. **执行**
   - 从仓库根目录运行：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/publish-gitee-release.ps1 -NoPause`
   - 根据用户意图附加 `-SkipBuild` / `-SkipPublish`。

3. **发版后**
   - 输出 Gitee Release 页面 URL、`releases/latest` 校验结果。
   - 提醒用户：若尚未 commit 版本号变更，请 commit 并 push 源码（tag 已指向当前 HEAD）。
   - 若本次包含客户端 Rust/TS 改造，提醒跑 `pnpm vitest` 相关 spec 与 Rust 测试。

4. **禁止**
   - 在日志、commit、Skill 正文写入 token。
   - 未经用户明确要求不执行 `git commit` / `push master`。
   - 不调用 legacy nginx 上传作为默认路径。

### 6.3 Skill frontmatter（草案）

```yaml
name: dbx-gitee-release
description: >-
  Pack DBX Windows NSIS installer and publish to Gitee Release (zeki_baoruixin/dbx),
  upload latest.json and installer, push version tag. Use when the user asks to
  release, publish update, or push installer to Gitee. Requires GITEE_ACCESS_TOKEN.
```

## 7. 测试与验证

| 层级 | 内容 |
| --- | --- |
| 脚本 | 本地 `-SkipPublish` dry-run 版本同步；mock API 单元测试（若抽 Python/Node 辅助） |
| Rust | `update_installer` 解析 Gitee latest + manifest 单测 |
| 前端 | 若有 `UpdateDialog` / updater 相关 spec，更新断言 URL |
| 手工 | 发版后在已安装客户端检查更新；确认 sha256 失败路径仍有效 |

发版前检查清单（Skill 引用）：`pnpm typecheck`（若改 TS）；`cargo test` 针对 `update_installer` 模块。

## 8. 安全

- Token 仅存环境变量或 gitignore env 文件。
- 安装包大小上限保持现有 `MAX_INSTALLER_BYTES`（512 MiB）。
- HTTP 下载仍无 TLS pinning；Gitee HTTPS 为常规范畴。

## 9. 实现顺序建议

1. `publish-gitee-release.ps1` + `.gitee-release.env.example`
2. 客户端 Gitee API 拉清单（Rust + 前端 URL）
3. 文档更新
4. `.cursor/skills/dbx-gitee-release/SKILL.md`
5. 可选：根目录 `release-gitee.bat` 薄包装

## 10. 验收标准

- [ ] 在 `update.txt` 追加新版本行后，一条命令完成构建、Gitee Release、附件、tag push。
- [ ] Gitee Release 页可见 exe 与 `latest.json`；`latest.json.url` 可下载且 sha256 正确。
- [ ] Windows 客户端能从 Gitee 检测新版本并下载安装（非 nginx）。
- [ ] Skill 能在 Agent 对话中复现上述流程且无 token 泄漏。
- [ ] legacy nginx 文档不再作为默认路径。
