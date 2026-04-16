use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, FillRect, InvalidateRect,
    SelectObject, SetBkMode, SetTextColor, TextOutW,
    DEFAULT_CHARSET, DEFAULT_PITCH, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION,
    FONT_QUALITY, HBRUSH, HDC, HFONT, HGDIOBJ, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRect, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    LoadCursorW, PeekMessageW, PostQuitMessage, SetWindowPos,
    RegisterClassW, SetCursor, ShowWindow, TranslateMessage,
    UnregisterClassW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, MSG,
    PM_REMOVE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW, WM_CREATE, WM_DESTROY,
    WM_INPUT, WM_PAINT, WM_SETCURSOR, WNDCLASSW, WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE,
};

const WIDTH: i32 = 640;
const HEIGHT: i32 = 400;

// ═══════════════════════════════════════════════════════════════════════
// 轮询率统计
// ═══════════════════════════════════════════════════════════════════════

struct PollRateStats {
    event_times: Vec<u64>,
    current_poll_rate: f64,
    average_poll_rate: f64,
    max_poll_rate: f64,
    min_poll_rate: f64,
    total_events: u64,
}

impl PollRateStats {
    fn new() -> Self {
        Self {
            event_times: Vec::with_capacity(2000),
            current_poll_rate: 0.0,
            average_poll_rate: 0.0,
            max_poll_rate: 0.0,
            min_poll_rate: f64::MAX,
            total_events: 0,
        }
    }

    fn add_event(&mut self) {
        let now = qpc_micros();
        self.event_times.push(now);
        self.total_events += 1;
        let cutoff = now.saturating_sub(1_000_000);
        self.event_times.retain(|&t| t > cutoff);
        self.recalculate();
    }

    fn recalculate(&mut self) {
        let now = qpc_micros();
        let cutoff = now.saturating_sub(1_000_000);
        let recent: Vec<_> = self.event_times.iter().copied().filter(|&t| t > cutoff).collect();
        if recent.len() >= 2 {
            let span = recent.last().unwrap() - recent.first().unwrap();
            if span > 0 {
                let rate = (recent.len() as f64 - 1.0) / (span as f64 / 1_000_000.0);
                self.current_poll_rate = rate.max(0.0);
                self.max_poll_rate = self.max_poll_rate.max(self.current_poll_rate);
                if self.current_poll_rate > 0.0 {
                    self.min_poll_rate = self.min_poll_rate.min(self.current_poll_rate);
                }
                if self.average_poll_rate == 0.0 {
                    self.average_poll_rate = self.current_poll_rate;
                } else {
                    self.average_poll_rate = self.average_poll_rate * 0.9 + self.current_poll_rate * 0.1;
                }
            }
        }
    }
}

fn qpc_micros() -> u64 {
    unsafe {
        let mut freq = 0i64;
        let mut count = 0i64;
        QueryPerformanceFrequency(&mut freq).unwrap();
        QueryPerformanceCounter(&mut count).unwrap();
        (count as f64 / freq as f64 * 1_000_000.0) as u64
    }
}

static STATS: Mutex<Option<PollRateStats>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(true);

// ═══════════════════════════════════════════════════════════════════════
// 字体（程序生命周期内创建一次）
// ═══════════════════════════════════════════════════════════════════════

struct Fonts {
    title: HFONT,
    big: HFONT,
    normal: HFONT,
}

impl Fonts {
    fn new(scale: i32) -> Self {
        unsafe {
            let facename = PCWSTR(to_wide("Consolas").as_ptr());
            let sz = |base: i32| -(base * scale / 96);
            Self {
                title: CreateFontW(sz(28), 0, 0, 0, 700, 0, 0, 0,
                    DEFAULT_CHARSET, FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(5), DEFAULT_PITCH.0 as u32, facename),
                big: CreateFontW(sz(56), 0, 0, 0, 700, 0, 0, 0,
                    DEFAULT_CHARSET, FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(5), DEFAULT_PITCH.0 as u32, facename),
                normal: CreateFontW(sz(18), 0, 0, 0, 400, 0, 0, 0,
                    DEFAULT_CHARSET, FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0),
                    FONT_QUALITY(5), DEFAULT_PITCH.0 as u32, facename),
            }
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.title.0));
            let _ = DeleteObject(HGDIOBJ(self.big.0));
            let _ = DeleteObject(HGDIOBJ(self.normal.0));
        }
    }
}

static FONTS_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

// ═══════════════════════════════════════════════════════════════════════
// 窗口过程
// ═══════════════════════════════════════════════════════════════════════

fn register_raw_input(hwnd: HWND) {
    let rid = RAWINPUTDEVICE {
        usUsagePage: 0x01, usUsage: 0x02,
        dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd,
    };
    unsafe {
        RegisterRawInputDevices(&[rid], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            .expect("RegisterRawInputDevices failed");
    }
}

fn paint_to_dc(hdc: HDC, fonts: &Fonts, stats: &PollRateStats, scale: i32) {
    let s = |v: i32| v * scale / 96;
    unsafe {
        // 背景
        let bg = CreateSolidBrush(COLORREF(0xF0F0F0));
        let rect = RECT { left: 0, top: 0, right: s(WIDTH), bottom: s(HEIGHT) };
        FillRect(hdc, &rect, bg);
        let _ = DeleteObject(HGDIOBJ(bg.0));

        SetBkMode(hdc, TRANSPARENT);

        // 标题
        SelectObject(hdc, HGDIOBJ(fonts.title.0));
        SetTextColor(hdc, COLORREF(0x333333));
        let _ = TextOutW(hdc, s(20), s(20), &to_wide("Mouse Poll Rate Tester"));

        // 大号数字
        SelectObject(hdc, HGDIOBJ(fonts.big.0));
        SetTextColor(hdc, COLORREF(0xCC6600));
        let _ = TextOutW(hdc, s(20), s(55), &to_wide(&format!("{:.0} Hz", stats.current_poll_rate)));

        // 进度条轨道
        let track = CreateSolidBrush(COLORREF(0xDDDDDD));
        let track_rect = RECT { left: s(20), top: s(130), right: s(580), bottom: s(140) };
        FillRect(hdc, &track_rect, track);
        let _ = DeleteObject(HGDIOBJ(track.0));

        // 进度条填充
        let fill_w = ((stats.current_poll_rate / 1000.0).min(1.0) * s(560) as f64) as i32;
        if fill_w > 0 {
            let bar_color = if stats.current_poll_rate >= 900.0 { 0x00AA00 }
                else if stats.current_poll_rate >= 400.0 { 0x00AAFF }
                else { 0xFF4444 };
            let fill_brush = CreateSolidBrush(COLORREF(bar_color));
            let fill_rect = RECT { left: s(20), top: s(130), right: s(20) + fill_w, bottom: s(140) };
            FillRect(hdc, &fill_rect, fill_brush);
            let _ = DeleteObject(HGDIOBJ(fill_brush.0));
        }

        // 统计
        SelectObject(hdc, HGDIOBJ(fonts.normal.0));
        SetTextColor(hdc, COLORREF(0x1A1A1A));

        let lines = [
            format!("Average: {:.1} Hz", stats.average_poll_rate),
            format!("Max: {:.0} Hz", stats.max_poll_rate),
            if stats.min_poll_rate == f64::MAX { "Min: --".into() } else { format!("Min: {:.0} Hz", stats.min_poll_rate) },
            format!("Total Events: {}", stats.total_events),
        ];
        for (i, line) in lines.iter().enumerate() {
            let _ = TextOutW(hdc, s(20), s(165) + i as i32 * s(30), &to_wide(line));
        }

        SetTextColor(hdc, COLORREF(0x999999));
        let _ = TextOutW(hdc, s(20), s(310), &to_wide("Move your mouse to test polling rate"));
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                register_raw_input(hwnd);
                LRESULT(0)
            }
            WM_INPUT => {
                if let Some(s) = STATS.lock().unwrap().as_mut() {
                    s.add_event();
                }
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc_screen = BeginPaint(hwnd, &mut ps);
                let dpi = GetDpiForWindow(hwnd) as i32;
                let fonts_ptr = FONTS_PTR.load(Ordering::SeqCst) as *const Fonts;
                if !fonts_ptr.is_null() {
                    if let Some(stats) = STATS.lock().unwrap().as_ref() {
                        let w = WIDTH * dpi / 96;
                        let h = HEIGHT * dpi / 96;
                        // 双缓冲：绘制到离屏 DC，再一次性 BitBlt 到屏幕
                        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
                        let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
                        let old = SelectObject(hdc_mem, HGDIOBJ(hbm.0));

                        paint_to_dc(hdc_mem, &*fonts_ptr, stats, dpi);

                        let _ = BitBlt(hdc_screen, 0, 0, w, h, Some(hdc_mem), 0, 0, SRCCOPY);

                        SelectObject(hdc_mem, old);
                        let _ = DeleteObject(HGDIOBJ(hbm.0));
                        let _ = DeleteDC(hdc_mem);
                    }
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_SETCURSOR => {
                if (lparam.0 as u32 & 0xFFFF) == 1 {
                    // HTCLIENT
                    let _ = SetCursor(Some(LoadCursorW(None, IDC_ARROW).unwrap()));
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_DESTROY => {
                RUNNING.store(false, Ordering::SeqCst);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// 主函数
// ═══════════════════════════════════════════════════════════════════════

fn main() {
    *STATS.lock().unwrap() = Some(PollRateStats::new());

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let hinstance = HINSTANCE(GetModuleHandleW(None).unwrap().0);
        let class_name = to_wide("MprtWndClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            style: CS_HREDRAW | CS_VREDRAW,
            hbrBackground: HBRUSH((5 + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let title = to_wide("Mouse Poll Rate Tester");
        // 需要先创建窗口才能获取 DPI，所以先用不可见窗口
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT,
            WIDTH, HEIGHT,
            None, None, Some(hinstance), None,
        ).unwrap();

        let dpi = GetDpiForWindow(hwnd) as i32;
        let s = |v: i32| v * dpi / 96;
        let mut rect = RECT { left: 0, top: 0, right: s(WIDTH), bottom: s(HEIGHT) };
        let _ = AdjustWindowRect(&mut rect, WS_OVERLAPPEDWINDOW, false);
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;

        let _ = SetWindowPos(
            hwnd, None,
            0, 0, w, h,
            SWP_NOMOVE | SWP_NOZORDER,
        );

        // 初始化字体（堆上，指针传给 wndproc），使用实际 DPI
        let fonts = Box::new(Fonts::new(dpi));
        let fonts_ref: *const Fonts = &*fonts;
        FONTS_PTR.store(fonts_ref as usize, Ordering::SeqCst);

        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg: MSG = std::mem::zeroed();

        while RUNNING.load(Ordering::SeqCst) {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = InvalidateRect(Some(hwnd), None, false);
        }

        FONTS_PTR.store(0, Ordering::SeqCst);
        drop(fonts);
        let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance));
    }
}
