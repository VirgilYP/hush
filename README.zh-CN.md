# hush

[English](README.md) | [中文](README.zh-CN.md)

`hush` 是一个面向 `tio` 风格 UART 调试流程的 prompt-aware 包装层，适合设备 UART 一直刷后台日志、但你又需要输入 shell 命令的调试场景。

它的核心目标是：在看到设备 prompt 后暂停后台刷屏，让你稳定输入命令；发送命令前先把暂停期间缓存的旧日志刷出来，再把命令发给设备，避免旧日志混到命令响应后面。

仓库还包含一个 macOS 优先的 Tauri GUI，提供 UART / USB HID 接收终端、手动发送栏和类似 SSCOM“多字符串”的可编辑命令集。

## macOS GUI

GUI 的 UART / HID 设备所有权和字节处理都在 Rust 中完成。WebView 只负责显示，UART 直接调用共享的 `hush` library，不会启动或解析 CLI 子进程。

```bash
cd apps/hush-gui
npm install
npm run tauri dev
```

构建本机 macOS `.app`：

```bash
npm run tauri build -- --bundles app
open src-tauri/target/release/bundle/macos/Hush.app
```

当前开发包是 ad-hoc 签名；如果要发给其他 Mac 使用，仍需 Apple Developer 签名和公证。

### DM30 缺省配置

第一次运行会加载 `DM30` 命令集：

- UART1：115200 波特率、8 数据位、无校验、1 停止位
- 顶部可切换 `UART` / `USB HID`；HID 只列出旧版 DM30 `1ACC:1A61` 和新版 `1ACC:1AEC`，使用 Report ID 1，每条命令（含结束符）最多 63 字节
- 文本行结束符：CR
- 14 条可编辑命令：上电、上报全部配置、DM30 独立调试增益/关闭电平上报，以及两条带警告标识的 IN1 32.0→31.9 dB 问题复现命令
- `Silence UART reports` 一键依次发送 6 条模块级 `rpsw=off`，不会修改增益
- `-200..0 dB` 安全增益测试滑块可以选择 10 个增益模块之一，并同步修改对应的可编辑命令；点击 `Apply` 才真正发送
- Algorithm 下拉按音频链路分组列出当前启用且真实支持 `sw=on/off` 的模块实例，可直接发送 ON/OFF
- macOS 只列出 `/dev/cu.*` callout 设备；旧配置中的 `/dev/tty.*` 会在存在对应 callout 设备时自动迁移

所有槽位都能修改，也可以新增、删除、排序并切换 Text/HEX。`Load` 和 `Export` 使用带版本号的 JSON 命令集文件，只保存命令集名称和槽位，不携带本机容易变化的串口路径。

`examples/dm30-three-task-hil.hush-commands.json` 是用于验收 DM30 `1ACC:1AEC`、`mid=-2,dsp` 和 `mid=51,g=-280..0` 的 12 槽命令集，集中保存合法边界与应当拒绝的反例，可直接通过 `Load` 加载。

接收数据每隔几毫秒从 Rust 转发，并在下一显示帧追加到同一个文本节点，视觉上与 CLI 一样连续刷屏，同时避免反复重建整个终端 DOM。UART 原始字节保存到 `/tmp/hush-gui-*.log`，HID 输入报告保存到 `/tmp/hush-gui-hid-*.log`；暂停显示或清屏不会修改原始日志。

DM30 当前固件中的 `set mid=-2,power=on` 只能从 UART / shell 进入，而且设备上电后才会枚举 USB HID。因此 HID 模式会禁用该槽位；其他 DSP 命令仍使用同一批可编辑槽位发送。

一个串口同一时间只能由一个程序持有。连接 GUI 前，需要先关闭已经占用该端口的 CLI 串口终端。

接收区向上滚动、离开底部时会在新数据抢回滚动位置之前自动进入 Smart Pause。滚回底部只会解除滚动触发的暂停；手动暂停仍需手动恢复。

## 和 tio 的关系

这个工具的定位是对 `tio` 常见使用流程做一层小包装，不是要替代 `tio` 的完整能力。

普通串口会话直接用 `tio` 就很好；当设备持续刷后台日志、你又需要围绕 shell prompt 输入命令时，用 `hush` 在熟悉的串口终端流程上补一层 prompt-aware 的交互状态机。快捷键也刻意保留了 `tio` 用户熟悉的 `Ctrl-T` 前缀风格。

## 安装

```bash
cargo install --path .
```

或者只构建 release 版本：

```bash
cargo build --release
```

安装随仓库分发的 Codex skill：

```bash
scripts/install-codex-skill.sh
```

默认会安装到 `$CODEX_HOME/skills/hush-uart-console`；如果没有设置
`CODEX_HOME`，则安装到 `~/.codex/skills/hush-uart-console`。也可以指定其他位置：

```bash
scripts/install-codex-skill.sh --dest /path/to/skills
scripts/install-codex-skill.sh --target /path/to/hush-uart-console --force
```

## 使用

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000
```

也可以通过环境变量指定默认串口设备：

```bash
export HUSH_DEVICE=/dev/cu.usbmodem01234567895
hush -b 3000000
```

默认日志会写到 `/tmp` 下，例如：

```text
/tmp/hush-1779081234.log
```

日志文件保存 UART 原始字节。屏幕显示层做的换行整理不会改动日志内容。

主会话还会基于串口设备名打开一个本地 monitor socket。以上面的命令为例，另一个终端可以这样接入：

```bash
hush monitor usbmodem01234567895
```

monitor 窗口会镜像主 `hush` 屏幕，并把自己的键盘输入转发给主会话。它不会自己打开串口设备。

## 交互模型

正常模式：

```text
设备日志实时刷屏
```

按一次空 `Enter`：

```text
给设备发送一个行结束符
等待看到 '$' prompt
显示 prompt
暂停后续设备输出
```

输入命令后再按 `Enter`：

```text
清掉本地输入行
先把暂停期间缓存的设备输出刷到屏幕
再把你的命令发送给设备
恢复实时输出
```

这样旧的后台日志不会出现在命令响应后面。

## 快捷键

```text
空 Enter     发送换行，等待 '$'，然后停在 prompt
Enter        刷出暂停输出，然后发送当前输入行
方向键 ↑ / ↓  选择持久化历史中更旧 / 更新的命令
Ctrl-U       清空当前输入
Backspace    删除一个输入字符
Ctrl-C       向设备发送 Ctrl-C
Ctrl-T r     恢复实时输出
Ctrl-T q     退出
Ctrl-T l     清屏
Ctrl-T ?     显示帮助
```

## 监控窗口

先在拥有串口设备的终端里正常启动 `hush`：

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000
```

然后在另一个终端接入：

```bash
hush monitor usbmodem01234567895
```

在 monitor 窗口里输入的内容会转发到主 `hush` 会话，并走主会话同一套 prompt-aware 状态机。`Ctrl-T` 组合键也会转发，所以 `Ctrl-T r`、`Ctrl-T l`、`Ctrl-T ?`、`Ctrl-T q` 都是在控制主 `hush` 会话。

如果只想退出当前 monitor 窗口，按 `Ctrl-]`。

如果主 `hush` 会话退出，或者主终端被关掉，已接入的 monitor 窗口会在 monitor socket 断开后自动退出。

只读监控可以这样打开：

```bash
hush monitor --read-only usbmodem01234567895
```

列出当前可 monitor 的 session：

```bash
hush sessions
hush monitor --list
```

启动主会话时也可以指定稳定的 session 名：

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000 --session dut-a
hush monitor dut-a
```

## Agent 协作流程

AI 辅助调试时，推荐由用户自己的真实终端持有串口，Agent 只通过 monitor 接入：

```bash
# 用户终端
hush /dev/cu.usbmodem01234567895 -b 3000000 --session dut-c

# Agent 终端
hush sessions
hush monitor dut-c
```

Agent 可以在 monitor 窗口输入命令，命令会经由主 `hush` 会话发送到设备；用户在主终端里能看到 Agent 输入了什么、设备返回了什么。因为两个窗口共享同一条输入流，不要两边同时打字。

如果只想让 Agent 旁观、不允许输入，用：

```bash
hush monitor --read-only dut-c
```

## 参数

```text
-d <device>              串口设备。也可以作为位置参数传入。
-b <baud>                波特率。默认：3000000。
-l <logfile>             日志文件路径。默认：/tmp/hush-*.log。
--session <name>         monitor session 名。默认：串口设备 basename。
--newline cr|lf|crlf     命令行结束符。默认：cr。
```

## 说明

- 当前 prompt 检测使用 `$` 字符。
- 命令历史会跨 `hush` 会话持久化保存：设置了 `XDG_STATE_HOME` 时使用 `$XDG_STATE_HOME/hush/history`，否则使用 `~/.local/state/hush/history`。每次启动会载入最近 1000 条命令。浏览到最新命令后继续按 ↓，会恢复开始浏览历史前尚未发送的输入草稿。
- 默认命令行结束符是 carriage return，也就是 `cr` / `\r`，这是很多嵌入式 UART shell 的常见输入方式。
- 如果你的 shell 需要 LF 或 CRLF，可以使用 `--newline lf` 或 `--newline crlf`。
