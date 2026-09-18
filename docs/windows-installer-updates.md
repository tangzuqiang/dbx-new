# Windows 安装版更新

DBX 的 Windows NSIS 安装版采用与 Rebased 相同的 `update.txt` → 构建 → 上传 `latest.json` 和安装包 → 客户端静默安装的流程，不使用 Tauri 更新签名。

共用服务器主机 `111.230.247.111`，但 DBX 独立使用 `http://111.230.247.111/dbx/latest.json` 及 `/usr/share/nginx/html/dbx`，避免误装 Rebased。安装包 URL 必须属于这一主机的 `/dbx/` 路径。发布脚本计算 SHA-256，客户端在下载后核验。HTTP 链路不提供传输层身份验证，生产环境建议给同一目录配置 HTTPS。

每次发布前，在根目录 `update.txt` 末尾追加一行 `版本号 更新内容`，例如 `0.6.1 修复连接问题`。脚本只读取最后一条有效记录，并同步根目录 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 的版本号。

复制 `scripts/.update-server.env.example` 为 `scripts/.update-server.env`，填入 `UPDATE_PASS`。该文件被 Git 忽略。先运行 `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/update.ps1 -SkipBuild -SkipPublish -NoPause` 检查版本同步；正式发布运行根目录 `update.bat` 或 `scripts/update.ps1 -NoPause`。脚本不会清理 Cargo 编译缓存；NSIS 安装包会复制保留在 `release/`，再上传到服务器。

DBX 的自定义 NSIS 安装器需使用 `/S /UPDATE /R`，因此与 Rebased 的 `/S /R` 略有不同。安装前辅助脚本等待旧进程退出，安装后启动 DBX。便携版、Windows 7 专用版、macOS 和 Linux 仍只提供手动更新入口。现有 GitHub Release 工作流保留非 Windows 平台与旧版客户端所需的产物，但新版 Windows 客户端只读取上述 DBX 服务器清单.
