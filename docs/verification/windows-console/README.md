# Windows / macOS hush CLI 验收 — 2026-09-20

基线49f4f56；Windows通过uds_windows本机socket和原生Console guard适配，串口、命令编辑、历史与prompt逻辑共用。原Mac主worktree保持不变，移植在codex/windows-console。Rust固定1.98.1；Mac12项、Windows11项单元测试通过（Mac多1项callout路径测试），包括monitor双向命令/prompt、重复owner拒绝和socket清理。两端check/fmt/Clippy -D warnings/release构建通过。源码与二进制身份见artifact-sha256.txt，Windows检查打印同一源码hash。

Windows DEV LYP原默认Rust1.87且Cargo代理127.0.0.1:7890不可达；使用项目toolchain和锁定依赖vendor离线验证，没有改全局Rust或代理设置。首轮Windows编译暴露mem import被过度cfg，修正后检查通过；随后将Windows函数移到test module前满足Clippy。SSH嵌套-Command造成cwd不继承后，统一改用固定-File检查脚本；最终日志对应实际成功的check/test/clippy/release。

## 实机证据

DUT VIRGIL_LYP_HP COM56，3M、8N1，停止旧只读PowerShell owner后由hush.exe接管。

- 主窗口空Enter取得uart:~$，help返回Available commands。
- interactive monitor输入panel_status，真实设备返回audio_applied=1、matrix_fault=0、route_pending=0、route_fault=0；这不是USB音频或fresh Mixer mutation验收。
- monitor上箭头+Enter再次执行panel_status，历史编辑/转发通过。
- Ctrl-]仅detach interactive monitor，主窗口继续。
- Ctrl-T q关闭主窗口，readonly monitor自动结束，sessions变为空；COM56可重新打开。
- 未执行写配置、重启、烧录、驱动替换或音频流。

原始设备串口日志临时保留：DUT %TEMP%/hush-1789890588.log、本机/tmp/hush-dut-shell.log；SHA256 e90736c81056576e988f9999e9d53052b3c1a2d98eec4c4e3a2916e4f8a35f2f。日志未上传到仓库/Notion附件。该验收只证明终端和设备shell双向链路；没有声称串口零丢字或USB/feedback/THD通过。

Mac新CLI安装于~/.local/bin/hush，旧二进制备份hush.previous；没有停止原有uart-isolation session。Windows DEV/DUT用户.local/bin/hush.exe安装hash一致，HKCU用户PATH登记成功；DUT新SSH可直接执行hush.exe sessions。GUI未构建或发布。
