# desktop-tidier 课件整理助手

单进程纯 Rust 的 Windows / Linux(KDE) 桌面课件定期整理后台工具，面向希沃白板长时间开机场景。
**无 GUI 窗口**：仅一个刷子样式的系统托盘图标 + 右键菜单（Windows release 构建隐藏控制台窗口）。

## 功能

- 自动整理：扫描桌面（或指定目录）中超过 N 天的课件文件 → 按词库关键词分类 → 移动到课件库分组目录。
- 定期归档：归档与整理**完全解耦**，只按 `archiver.interval_hours` 周期执行（默认 168h = 一周）；归档前按台账比对新增文件数（`archiver.min_new_files`），不足则跳过；归档 zip 可设**数量/总字节上限**（`max_archives` / `max_total_bytes`），超出时最旧的移入回收站（0 = 不限制）。
- 宽松间隔：班级电脑并非全天开机，启动 15 秒后自动追赶——距上次整理/归档已超期就立即补做一次（状态存于 `storage/state.json`）。
- MD 台账：每次收录按 `entry_format` 追加一条记录，编号顺延、编码顺延。
- 托盘菜单（固定 4 项）：`设置开机自启` / `立刻整理` / `打开配置文件目录` / `退出`。
- 配置与词库热重载（文件监听 + 防抖）；系统通知（可关闭）。

## 构建与验收

```bash
just fmt     # cargo fmt --check
just lint    # cargo clippy --all-targets -- -D warnings
just test    # cargo test
just build   # cargo build --release（Linux 本机）
```

## 运行

```bash
cargo run --release
# 或直接运行二进制：
./target/release/desktop-tidier
```

Windows 双击 `desktop-tidier.exe` 即可（无窗口、无控制台，仅在托盘出现刷子图标）。

### 配置目录（首次运行自动生成，扁平布局）

Windows：`%APPDATA%\desktop-tidier\`；Linux：`~/.config/desktop-tidier\`

| 文件 | 作用 |
|---|---|
| `config.json` | **唯一生效配置**（改它） |
| `lexicon.txt` | **唯一生效词库**（改它） |
| `schema.json` / `config.example.json` / `配置说明.md` | 参考文件（改了不影响程序） |
| `storage/` | 待分类 / 课件库 / 归档 / 台账 / `state.json` |
| `logs/` | 日志 |

## 打包

- **Linux**：见 `packaging/`（`PKGBUILD`、`desktop-tidier.desktop`、systemd 用户服务）。
- **Windows 单文件 exe**：走 GitHub Actions（`.github/workflows/ci.yml`），静态 CRT 无 DLL 依赖、图标内嵌。
  ```bash
  git remote add origin <你的仓库> && git push -u origin main
  # CI 产物：desktop-tidier-windows-x64 / desktop-tidier.exe
  ```
  本地交叉编译（可选）：`just build-win`（需 `rustup target add x86_64-pc-windows-msvc`）。

## 目录

- `src/`：config / platform / scheduler / watcher / organizer / classifier / archiver / indexer / state / timesync / logger / tray
- `config/`：`schema.json`、`config.example.json`、`配置说明.md`、`lexicon.txt`
- `tests/`：集成测试（tempfile 沙盒 + 固定时钟 + 假平台）
- `packaging/`：Linux 打包 + 系统集成文件
- `AGENTS.md`：总规格（防漂移核心，开发前必读）
