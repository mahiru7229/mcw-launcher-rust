pub struct GameWindowActivator;

impl GameWindowActivator {
    /// Grant the target PID permission to set the foreground window on Windows.
    pub fn allow_foreground(pid: u32) -> bool {
        #[cfg(target_os = "windows")]
        {
            #[link(name = "user32")]
            extern "system" {
                fn AllowSetForegroundWindow(dwProcessId: u32) -> i32;
            }
            if pid == 0 {
                return false;
            }
            // SAFETY: Standard Win32 user32 call with numeric PID.
            unsafe { AllowSetForegroundWindow(pid) != 0 }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = pid;
            false
        }
    }

    /// Best-effort non-blocking delayed focus helper: polls briefly for the game window
    /// of `pid` (filtering out 0x0 or dummy `<100x100` OpenGL helper windows) and activates it.
    pub fn focus_process_window_delayed(pid: u32) {
        #[cfg(target_os = "windows")]
        {
            if pid == 0 {
                return;
            }
            let _ = std::thread::Builder::new()
                .name("mcw-win-activator".to_string())
                .spawn(move || {
                    for _ in 0..30 {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if Self::try_activate_pid_window(pid) {
                            break;
                        }
                    }
                });
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = pid;
        }
    }

    #[cfg(target_os = "windows")]
    fn try_activate_pid_window(target_pid: u32) -> bool {
        #[repr(C)]
        struct Rect {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }
        #[link(name = "user32")]
        extern "system" {
            fn EnumWindows(
                lpEnumFunc: Option<unsafe extern "system" fn(isize, isize) -> i32>,
                lParam: isize,
            ) -> i32;
            fn IsWindowVisible(hWnd: isize) -> i32;
            fn GetWindowThreadProcessId(hWnd: isize, lpdwProcessId: *mut u32) -> u32;
            fn GetWindow(hWnd: isize, uCmd: u32) -> isize;
            fn GetWindowRect(hWnd: isize, lpRect: *mut Rect) -> i32;
            fn ShowWindow(hWnd: isize, nCmdShow: i32) -> i32;
            fn SetForegroundWindow(hWnd: isize) -> i32;
        }

        struct SearchState {
            target_pid: u32,
            matched_hwnd: isize,
        }

        unsafe extern "system" fn enum_proc(hwnd: isize, lparam: isize) -> i32 {
            const GW_OWNER: u32 = 4;
            let state = &mut *(lparam as *mut SearchState);
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut wpid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut wpid as *mut u32);
            if wpid != state.target_pid {
                return 1;
            }
            if GetWindow(hwnd, GW_OWNER) != 0 {
                return 1;
            }
            let mut rect = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetWindowRect(hwnd, &mut rect as *mut Rect) == 0 {
                return 1;
            }
            if (rect.right - rect.left) < 100 || (rect.bottom - rect.top) < 100 {
                return 1;
            }
            state.matched_hwnd = hwnd;
            0
        }

        const SW_RESTORE: i32 = 9;
        let mut state = SearchState {
            target_pid,
            matched_hwnd: 0,
        };
        // SAFETY: `state` outlives synchronous `EnumWindows` invocation.
        unsafe {
            EnumWindows(Some(enum_proc), &mut state as *mut SearchState as isize);
            if state.matched_hwnd != 0 {
                ShowWindow(state.matched_hwnd, SW_RESTORE);
                SetForegroundWindow(state.matched_hwnd);
                return true;
            }
        }
        false
    }
}
