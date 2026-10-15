//! The system's own open and save dialogs (Windows: comdlg32, no library to install). Elsewhere they report that there is none and
//! the editor falls back to the file list in the Project panel.
use std::path::{Path, PathBuf};

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    #[repr(C)]
    struct OpenFileNameW {
        struct_size: u32,
        owner: isize,
        instance: isize,
        filter: *const u16,
        custom_filter: *mut u16,
        max_custom_filter: u32,
        filter_index: u32,
        file: *mut u16,
        max_file: u32,
        file_title: *mut u16,
        max_file_title: u32,
        initial_dir: *const u16,
        title: *const u16,
        flags: u32,
        file_offset: u16,
        file_extension: u16,
        def_ext: *const u16,
        cust_data: isize,
        hook: isize,
        template_name: *const u16,
        reserved: *mut c_void,
        reserved_dw: u32,
        flags_ex: u32,
    }

    #[link(name = "comdlg32")]
    extern "system" {
        fn GetOpenFileNameW(ofn: *mut OpenFileNameW) -> i32;
        fn GetSaveFileNameW(ofn: *mut OpenFileNameW) -> i32;
    }

    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
    const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
    const OFN_EXPLORER: u32 = 0x0008_0000;

    pub fn struct_size() -> usize {
        std::mem::size_of::<OpenFileNameW>()
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn dialog(save: bool, title: &str, filter_name: &str, patterns: &str, dir: &Path, default_name: &str, def_ext: &str) -> Option<PathBuf> {
        // "Name\0pattern\0All files\0*.*\0\0"
        let mut filter: Vec<u16> = Vec::new();
        for part in [filter_name, patterns, "All files", "*.*"] {
            filter.extend(part.encode_utf16());
            filter.push(0);
        }
        filter.push(0);
        let mut buf = vec![0u16; 1024];
        for (i, c) in default_name.encode_utf16().take(1000).enumerate() {
            buf[i] = c;
        }
        let title_w = wide(title);
        let dir_w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let ext_w = wide(def_ext);
        let mut ofn = OpenFileNameW {
            struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
            owner: 0,
            instance: 0,
            filter: filter.as_ptr(),
            custom_filter: std::ptr::null_mut(),
            max_custom_filter: 0,
            filter_index: 1,
            file: buf.as_mut_ptr(),
            max_file: buf.len() as u32,
            file_title: std::ptr::null_mut(),
            max_file_title: 0,
            initial_dir: dir_w.as_ptr(),
            title: title_w.as_ptr(),
            flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST },
            file_offset: 0,
            file_extension: 0,
            def_ext: ext_w.as_ptr(),
            cust_data: 0,
            hook: 0,
            template_name: std::ptr::null(),
            reserved: std::ptr::null_mut(),
            reserved_dw: 0,
            flags_ex: 0,
        };
        // SAFETY: `ofn` points at buffers that live until the call returns (the filter, the title, the directory, the extension and the
        // file name buffer, whose length is given), and the struct size is the real size of the structure.
        let ok = unsafe { if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) } };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..end])))
    }
}

/// Ask for a scene file to open; `None` when cancelled (or when there is no dialog on this system).
pub fn open_scene(dir: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        win::dialog(false, "Open scene", "Lunaris scenes", "*.lunaris.json;*.json", dir, "", "json")
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        None
    }
}

/// Ask where to save a game's program (a folder and a name ending in `.exe`).
pub fn save_program(dir: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        win::dialog(true, "Build the game as", "Programs", "*.exe", dir, name, "exe")
    }
    #[cfg(not(windows))]
    {
        let _ = (dir, name);
        None
    }
}

/// Ask where to save a scene.
pub fn save_scene(dir: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        win::dialog(true, "Save scene as", "Lunaris scenes", "*.lunaris.json", dir, name, "json")
    }
    #[cfg(not(windows))]
    {
        let _ = (dir, name);
        None
    }
}

pub fn available() -> bool {
    cfg!(windows)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_dialog_structure_has_the_size_windows_expects() {
        // OPENFILENAMEW is 152 bytes on 64-bit Windows
        #[cfg(all(windows, target_pointer_width = "64"))]
        assert_eq!(super::win::struct_size(), 152);
    }
}
