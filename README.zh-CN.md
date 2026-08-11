# hush

[English](README.md) | [中文](README.zh-CN.md)

`hush` 是一个面向 `tio` 风格 UART 调试流程的 prompt-aware 包装层，适合设备 UART 一直刷后台日志、但你又需要输入 shell 命令的调试场景。

它的核心目标是：在看到设备 prompt 后暂停后台刷屏，让你稳定输入命令；发送命令前先把暂停期间缓存的旧日志刷出来，再把命令发给设备，避免旧日志混到命令响应后面。

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
