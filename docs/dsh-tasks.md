# dsh 任务 prompt（M0 / M1）

> 用法：每个任务 = 一次 dsh headless 会话 = 一个可验证增量。喂入下述 prompt，跑完贴 `just fmt && just lint && just test` 结果；人工看 diff 通过后再进下一任务。

## M0 —— 仓库骨架 + justfile + AGENTS.md 落地 + CI

```
角色：Rust 资深工程师。先完整读 AGENTS.md、config/schema.json、config/配置说明.md，遵守其中全部约束。

任务：初始化 desktop-tidier 仓库骨架，不实现任何业务逻辑。

要求：
1. 创建单 crate（lib + bin 分离）：src/lib.rs、src/main.rs（main 只调 lib 的 run()）、src/error.rs。
2. Cargo.toml 列出 AGENTS.md §3 的全部依赖（按目标平台分：windows 用 tray-icon+muda+windows，linux 用 ksni+libc+rustix）；reqwest 设 default-features=false + rustls-tls；windows crate 只开用到的 feature。写完立即 `cargo generate-lockfile` 并提交 Cargo.lock。
3. justfile 提供 fmt / lint / test / build / package 五个入口（package 暂为占位）。
4. 建 lib 级 Error（thiserror）+ bin 级 anyhow 包装；提供 `pub fn run() -> anyhow::Result<()>` 空实现（只打印版本后返回 Ok）。
5. CI（.github/workflows/ci.yml）：linux-latest 上 `just fmt && just lint && just test`；另起一个 windows-latest job 做 `cargo build --release --target x86_64-pc-windows-msvc`（先只 build，不跑测试）。
6. 提交 Cargo.lock，确保依赖版本被锁死。

验收（DoD）：`just fmt && just lint && just test && just build` 四命令在本地 Linux 全部通过；CI 配置语法正确；Cargo.lock 已提交。
禁止：实现任何业务模块；新增 AGENTS.md §3 之外的依赖；在 main 里用 #[tokio::main]。
```

## M1 —— Config 结构 + Schema 校验 + 示例生成 + 错误提示

```
角色：Rust 资深工程师。先完整读 AGENTS.md（§5/§6/§11）、config/schema.json、config/配置说明.md、config/config.example.json，遵守其中全部约束。

任务：实现 M1（src/config/**），这是第一个冻结模块，格式一旦定了不再改。

要求：
1. 用 serde 定义 Config 结构，字段与 schema.json / config.example.json 完全一致；加载用 serde_json + jsonschema 双重校验（先 parse 成 Value 跑 schema，再反序列化）。
2. Config::load(path)：文件不存在 → 从内置示例生成 config.json 并返回；JSON 语法错或 schema 校验失败 → 返回带"字段 JSON Pointer + 字段名 + 期望 + 实际 + 改正建议"的错误（错误文本格式见 AGENTS.md §6，禁止用 unwrap）。
3. Config::write_example(path)：写出 config.example.json（内容与内置示例一致）。
4. 热重载骨架：ConfigStore 用 ArcSwap<Arc<Config>>，reload() 成功返回 Reloaded 并替换，失败返回 Rejected(String) 且保留旧值。本任务先只做 load/reload 逻辑，不接 notify 监听（那是 M8）。
5. 相对路径解析辅助：提供 `resolve_paths(&mut self, base: &Path)`，把 storage.*/logger.file/indexer.md_path/classifier.lexicon_path 等相对路径拼到配置文件所在目录。
6. 单元测试（tests/config_*.rs，用 tempfile 沙盒）覆盖：
   - 合法示例能加载且字段值正确；
   - 缺文件 → 自动生成 config.json 且内容等于示例；
   - 坏 JSON（如 watcher.min_age_days="abc"、缺 required 字段、unknown 字段）→ 错误消息含字段路径与改正建议；
   - reload 坏配置 → 返回 Rejected 且 get() 仍是旧值。
7. 完成后运行 `just fmt && just lint && just test` 并贴结果。

验收（DoD）：坏 JSON 的单测断言错误消息含 JSON Pointer 路径 + 字段名 + 建议；reload 失败保留旧值。
禁止：改 schema.json / config.example.json 的字段（如有问题停下说明）；引入 jsonschema 之外的校验依赖；精确行号定位（本任务明确不做）。
```

## 后续 M2~M11 任务模板（复用 M0/M1 格式）

每个任务的固定骨架：
1. 角色 + "先读 AGENTS.md 与 docs 中的冻结接口，遵守全部约束"。
2. 任务 + 输入（只许改 `src/<module>/**` 与 `tests/<module>_*.rs`）。
3. 要求（含具体单测覆盖点，映射 AGENTS.md §12 的 DoD）。
4. 完成后跑 `just fmt && just lint && just test` 贴结果。
5. 禁止事项（新增依赖 / 改冻结接口 / 改日志与配置格式）。

冻结顺序：M1（配置）与 M2（日志）先冻结；M3 平台抽象后，业务层禁止再出现 `#[cfg]` 平台分支。

## 新增需求速记（本次追加，写 M2~M11 prompt 时必须体现）

1. 托盘 = **刷子图标** + 固定四菜单（自上而下）：`设置开机自启` / `立刻整理` / `打开配置文件目录` / `退出`；无"重载/暂停"菜单项。
2. 扫描整理频率可自定义，默认两天一次 → `watcher.scan_interval_secs`（默认 `172800`）。
3. 首个交付物 = Windows 单文件 `.exe`（M11 直接产出，先不打包安装器）。
4. `PlatformAdapter` 新增 `set_autostart` / `is_autostart_enabled` / `open_in_file_manager`（M3 实现，M9 菜单接入）。
