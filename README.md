# desktop-tidier 课件整理助手

单进程纯 Windows / Linux(KDE) 桌面课件后台自动整理工具，面向教学场景，使您班级大屏的桌面保持整洁
使用Deepseek Harness开发，代码全部由Deepseek编写，因此保留了大部分ai工程文件（绝对不是因为我懒得管）
*注： linux 打包见 `packing/`*

## 主要功能

- 自动整理：扫描桌面/指定目录中超过 N 天的文件，并按词库关键词分类移动至指定目录
- 定期归档：归档按周期执行（默认一周），支持自动跳过变化太小的归档事件，支持自动清理老旧归档。
- 宽松间隔：电脑开机启动 15 秒后自动追赶，距上次整理/归档超期则立刻补做一次。
- 自动台账：记录收录文件
- 配置与词库热重载
- 可关闭的系统通知

### 配置文件目录（首次运行自动生成）

Windows：`%APPDATA%\desktop-tidier\`
Linux：`~/.config/desktop-tidier\`

| 文件 | 作用 |
|---|---|
| `config.json` | **唯一生效配置** |
| `lexicon.txt` | **唯一生效词库** |
| `schema.json` / `config.example.json` / `配置说明.md` | 参考文件（改了不影响程序） |
| `storage/` | 待分类 / 课件库 / 归档 / 台账 / `state.json` |
| `logs/` | 日志 |

## TODO LIST

…………估计以后有时间了可能都不会回来看这个项目，这条只是写在这里占位置。
