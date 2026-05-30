# autodns

一个基于 Rust + Tauri 的桌面 DNS 管理工具。

autodns 提供图形化界面来管理本地 DNS 服务、上游解析器、分流规则、缓存、查询历史和系统 DNS 接管。它适合需要在桌面环境里快速切换 DNS 策略、观察解析状态、调试规则命中的用户。

> 项目仍在早期迭代中，接口和配置结构可能继续调整。欢迎试用、反馈和提交 PR。

## 功能特性

- 本地 DNS 服务：支持启动、停止、重启和运行状态查看。
- 多协议上游：支持 UDP、TCP、DoT、DoQ、DoH HTTP/HTTPS 等上游类型。
- 分流规则：通过图形界面维护域名匹配规则，并将请求路由到指定上游。
- 代理支持：支持为上游配置 SOCKS5 代理。
- DNS 缓存：可配置缓存容量、TTL 范围和负缓存时间。
- 查询工具：在应用内发起 DNS 查询并查看响应记录。
- 查询历史：记录请求来源、上游、响应码、耗时和错误信息。
- 健康状态：展示上游健康状态、失败次数、延迟和最近错误。
- 系统 DNS 接管：可选接管系统网络接口 DNS，默认关闭。
- 桌面体验：托盘菜单、关闭行为偏好、开机启动、窗口大小与位置记忆。

## 截图

欢迎在发布稳定版本前补充截图或录屏。建议包含：

- 总览页
- 上游与代理配置
- 规则工作台
- DNS 查询历史
- 系统 DNS 接管

## 技术栈

- Rust
- Tauri 2
- React
- Vite
- Ant Design
- SQLite

## 环境要求

- Rust stable
- Node.js 与 npm
- Tauri CLI
- 平台相关依赖请参考 Tauri 官方文档

Windows 打包 MSI 还需要安装 WiX Toolset v3。

## 快速开始

安装前端依赖：

```bash
make install
```

启动开发版桌面应用：

```bash
make dev
```

也可以直接运行 Tauri 命令：

```bash
cd src-tauri
cargo tauri dev --config tauri.dev.conf.json
```

开发版和正式版使用不同的应用标识与本地数据目录，避免互相污染：

```text
dev:  com.autodns.desktop.dev  -> autodns-dev/autodns.sqlite3
prod: com.autodns.desktop      -> autodns/autodns.sqlite3
test: com.autodns.desktop.test -> autodns-test/autodns.sqlite3
```

## 常用命令

```bash
make install         # 安装前端依赖
make dev             # 启动开发版桌面应用
make build           # 构建当前平台桌面包
make build-frontend  # 仅构建前端
make build-tauri     # 仅构建 Rust/Tauri 应用
make check           # 运行前端构建和 Rust check
make test            # 运行 Rust 测试
make fmt             # 格式化 Rust 代码
make clean           # 清理构建产物
make windows-msi     # 在 Windows 上构建 MSI 安装包
```

## 构建发布包

构建当前平台安装包：

```bash
make build
```

Windows MSI：

```powershell
make windows-msi
```

输出目录：

```text
src-tauri/target/release/bundle/
```

## 配置与数据

autodns 的运行配置由桌面应用管理，通常不需要手动编辑配置文件。

- 运行配置和查询历史保存在本地 SQLite 数据库中。
- 桌面偏好保存在本地 `preferences.json` 中。
- 系统 DNS 接管默认关闭，只有在用户启用并确认后才会修改网络接口 DNS。

## GitHub Actions 发布

发布工作流位于：

```text
.github/workflows/release-tauri.yml
```

创建 tag 后会触发桌面包构建，并上传到 GitHub Release：

```bash
git tag v0.2.0
git push origin v0.2.0
```

## 项目结构

```text
frontend/    React + Vite 前端界面
src-tauri/   Rust + Tauri 桌面端与 DNS 运行时
scripts/     构建辅助脚本
```

## 贡献

欢迎提交 Issue 和 Pull Request。

建议在提交前运行：

```bash
make check
make test
```

如果修改了 Rust 代码，也建议运行：

```bash
make fmt
```

提交 PR 时请尽量说明：

- 变更目的
- 主要改动
- 测试方式
- 是否影响系统 DNS、配置迁移或本地数据

## 安全说明

DNS 工具可能影响系统网络行为。启用系统 DNS 接管前，请确认目标 DNS 服务器可用，并了解如何通过应用或系统网络设置恢复原始 DNS。

系统 DNS 通常只能配置服务器 IP，不能配置端口。因此要接管系统 DNS，autodns 需要监听本机 53 端口；如果 53 端口已被占用或权限不足，应用会阻止接管并给出提示。

## 许可证

本项目基于 MIT License 开源，详见 [LICENSE](LICENSE)。
