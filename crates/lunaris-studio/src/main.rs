// The editor is a Windows program: no console window when it is started by a double click.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    attach_console();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        show(lunaris_studio::USAGE);
        return;
    }
    match lunaris_studio::parse_args(&args) {
        Ok(opts) if opts.build.is_some() => match lunaris_studio::build_game(&opts) {
            Ok(text) => show(&text),
            Err(e) => {
                show(&format!("build failed: {e}"));
                std::process::exit(1);
            }
        },
        Ok(opts) => {
            if let Err(e) = lunaris_studio::run(opts) {
                show(&format!("The editor stopped: {e}"));
                std::process::exit(1);
            }
        }
        Err(e) => {
            show(&format!("{e}\n{}", lunaris_studio::USAGE));
            std::process::exit(2);
        }
    }
}

/// A program of the Windows subsystem has no console of its own: when it is started from one, write to that one.
fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(pid: u32) -> i32;
            fn GetStdHandle(n: u32) -> isize;
            fn SetStdHandle(n: u32, h: isize) -> i32;
            fn CreateFileW(name: *const u16, access: u32, share: u32, security: *const u8, disposition: u32, flags: u32, template: isize) -> isize;
        }
        // SAFETY: plain Win32 calls with NUL-terminated names and valid arguments; a failure just leaves the handles as they were.
        unsafe {
            if AttachConsole(0xFFFF_FFFF) != 0 {
                let out: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
                let h = CreateFileW(out.as_ptr(), 0x4000_0000, 3, std::ptr::null(), 3, 0, 0);
                if h != -1 {
                    for n in [0xFFFF_FFF5u32, 0xFFFF_FFF4u32] {
                        let cur = GetStdHandle(n);
                        if cur == 0 || cur == -1 {
                            SetStdHandle(n, h);
                        }
                    }
                }
            }
        }
    }
}

/// Say something: on the console when there is one (a release build has none, so a message box).
fn show(text: &str) {
    eprintln!("{text}");
    #[cfg(all(windows, not(debug_assertions)))]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            MessageBoxW(0, wide(text).as_ptr(), wide("Lunaris Editor").as_ptr(), 0x40);
        }
    }
}
