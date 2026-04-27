use enigo::Settings;

/// Ctrl+V через Windows SendInput
#[cfg(target_os = "windows")]
unsafe fn win_paste_text(text: &str) {
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(fmt: u32, h: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn CloseClipboard() -> i32;
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut std::ffi::c_void;
        fn GlobalLock(h: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn GlobalUnlock(h: *mut std::ffi::c_void) -> i32;
    }
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let hmem = GlobalAlloc(0x0002, wide.len() * 2);
    if hmem.is_null() { return; }
    let ptr = GlobalLock(hmem) as *mut u16;
    if ptr.is_null() { return; }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
    GlobalUnlock(hmem);
    if OpenClipboard(0) == 0 { return; }
    EmptyClipboard();
    SetClipboardData(13, hmem);
    CloseClipboard();
    std::thread::sleep(std::time::Duration::from_millis(60));

    #[repr(C)] #[derive(Copy, Clone)]
    struct KeybdInput { w_vk: u16, w_scan: u16, dw_flags: u32, time: u32, dw_extra_info: usize }
    #[repr(C)] #[derive(Copy, Clone)]
    union InputUnion { ki: KeybdInput, _pad: [u64; 4] }
    #[repr(C)]
    struct Input { type_: u32, input: InputUnion }
    extern "system" { fn SendInput(n: u32, p: *const Input, cb: i32) -> u32; }

    let z = KeybdInput { w_vk: 0, w_scan: 0, dw_flags: 0, time: 0, dw_extra_info: 0 };
    let seq = [
        Input { type_: 1, input: InputUnion { ki: KeybdInput { w_vk: 0x11, ..z } } },
        Input { type_: 1, input: InputUnion { ki: KeybdInput { w_vk: 0x56, ..z } } },
        Input { type_: 1, input: InputUnion { ki: KeybdInput { w_vk: 0x56, dw_flags: 2, ..z } } },
        Input { type_: 1, input: InputUnion { ki: KeybdInput { w_vk: 0x11, dw_flags: 2, ..z } } },
    ];
    SendInput(seq.len() as u32, seq.as_ptr(), std::mem::size_of::<Input>() as i32);
}

/// Cmd/Ctrl+V через enigo (macOS / Linux) — создаётся локально, не хранится в структуре
#[cfg(not(target_os = "windows"))]
fn unix_paste_text(text: &str) -> anyhow::Result<()> {
    use clipboard_rs::{Clipboard, ClipboardContext};
    use enigo::{Direction, Enigo, Key, Keyboard};

    let ctx = ClipboardContext::new()
        .map_err(|e| anyhow::anyhow!("Clipboard: {:?}", e))?;
    ctx.set_text(text.to_owned())
        .map_err(|e| anyhow::anyhow!("Clipboard set: {:?}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(60));

    let mut e = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("Enigo: {:?}", e))?;

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    e.key(modifier, Direction::Press).ok();
    e.key(Key::Unicode('v'), Direction::Click).ok();
    e.key(modifier, Direction::Release).ok();
    Ok(())
}

#[cfg(target_os = "windows")]
extern "system" {
    fn GetForegroundWindow() -> isize;
    fn SetForegroundWindow(hwnd: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, lpdwProcessId: *mut u32) -> u32;
    fn GetCurrentThreadId() -> u32;
    fn AttachThreadInput(idAttach: u32, idAttachTo: u32, fAttach: i32) -> i32;
}

// InputInjector не хранит Enigo — это важно для Send на macOS
pub struct InputInjector {
    saved_hwnd: isize,
}

impl InputInjector {
    pub fn new() -> Self {
        Self { saved_hwnd: 0 }
    }

    pub fn save_focus(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe { self.saved_hwnd = GetForegroundWindow(); }
    }

    pub fn save_focus_hwnd(&mut self, hwnd: isize) {
        self.saved_hwnd = hwnd;
    }

    pub fn clear_saved_focus(&mut self) {
        self.saved_hwnd = 0;
    }

    pub fn restore_focus_quick(&self) {
        self.do_set_foreground();
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    fn restore_focus(&self) {
        self.do_set_foreground();
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    fn do_set_foreground(&self) {
        #[cfg(target_os = "windows")]
        if self.saved_hwnd != 0 {
            unsafe {
                let tgt = GetWindowThreadProcessId(self.saved_hwnd, std::ptr::null_mut());
                let cur = GetCurrentThreadId();
                AttachThreadInput(cur, tgt, 1);
                SetForegroundWindow(self.saved_hwnd);
                AttachThreadInput(cur, tgt, 0);
            }
        }
    }

    pub fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        self.restore_focus();
        #[cfg(target_os = "windows")]
        unsafe { win_paste_text(text); }
        #[cfg(not(target_os = "windows"))]
        unix_paste_text(text)?;
        Ok(())
    }
}
