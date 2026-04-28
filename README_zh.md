# Mouse Poll Rate Tester

一款轻量级 Windows 工具，通过 Windows Raw Input API 实时测量并显示鼠标轮询率。

## 功能

- 实时轮询率显示（Hz）
- 1 秒滑动窗口计算
- 平均值、最大值、最小值追踪
- 可视化进度条（颜色编码）
- HiDPI（逐显示器 DPI v2）支持
- GDI 双缓冲无闪烁渲染
- 纯 Win32 API — 无框架、无运行时依赖
- 无命令行窗口（Windows 子系统）

## 系统要求

- Windows 10 或更高版本
- 鼠标（有线或无线）

## 下载

预编译二进制文件可在 [Releases](https://github.com/akiflax/mouse-poll-rate-tester/releases) 页面获取。

## 从源码构建

```sh
git clone https://github.com/akiflax/mouse-poll-rate-tester.git
cd mouse-poll-rate-tester
cargo build --release
```

二进制文件位于 `target/release/mouse_poll_rate_tester.exe`。

需要 Rust **1.85+**（2024 edition）。

## 工作原理

程序通过 `RegisterRawInputDevices` 注册原始鼠标输入，使用 `RIDEV_INPUTSINK` 标志捕获所有鼠标移动（即使窗口未获得焦点，但窗口必须保持打开）。

每次 `WM_INPUT` 消息触发时，通过 `QueryPerformanceCounter` 捕获时间戳。程序维护一个 1 秒的滑动时间窗口来计算实时轮询率。

## 免责声明

本项目全部代码由 AI（Claude Code）生成。

## 许可证

MIT
