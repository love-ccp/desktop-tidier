set shell := ["bash", "-c"]

# 默认：列出可用入口
default:
    @just --list

# 格式检查
fmt:
    cargo fmt --check

# clippy 零警告（含测试目标）
lint:
    cargo clippy --all-targets -- -D warnings

# 单元 + 集成测试
test:
    cargo test

# 发布构建（Linux 本机；Windows exe 走 CI）
build:
    cargo build --release

# Windows 交叉编译（需先 rustup target add x86_64-pc-windows-msvc；正式交付走 CI）
build-win:
    cargo build --release --target x86_64-pc-windows-msvc

# 打包占位（M11 产出 Windows 单文件 exe（CI），M10 做 Linux 打包）
package:
    @echo "package 为占位入口：M11 产出 Windows 单文件 exe（CI），M10 做 Linux 打包（见 packaging/）。"
