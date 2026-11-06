//! Native RTF clipboard flavor and ownership identity. arboard handles HTML
//! and Unicode text on all platforms; these add the word-processor flavor.

pub const MAX_RICH_BYTES: usize = 16 << 20;

#[cfg(target_os = "macos")]
mod platform {
    use super::MAX_RICH_BYTES;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    pub fn sequence() -> Option<u64> {
        unsafe {
            let board: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
            if board.is_null() {
                return None;
            }
            let count: isize = msg_send![board, changeCount];
            Some(count as u64)
        }
    }

    pub fn rtf() -> Option<Vec<u8>> {
        unsafe {
            let board: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
            if board.is_null() {
                return None;
            }
            let kind = NSString::from_str("public.rtf");
            let data: *mut AnyObject = msg_send![board, dataForType: &*kind];
            if data.is_null() {
                return None;
            }
            let len: usize = msg_send![data, length];
            if len == 0 || len > MAX_RICH_BYTES {
                return None;
            }
            let bytes: *const std::ffi::c_void = msg_send![data, bytes];
            (!bytes.is_null()).then(|| std::slice::from_raw_parts(bytes.cast::<u8>(), len).to_vec())
        }
    }

    pub fn put_rtf(bytes: &[u8]) -> bool {
        unsafe {
            let board: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
            if board.is_null() {
                return false;
            }
            let kind = NSString::from_str("public.rtf");
            let data: *mut AnyObject = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>() length: bytes.len()];
            msg_send![board, setData: data forType: &*kind]
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::MAX_RICH_BYTES;
    use std::ffi::c_void;
    type Handle = *mut c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(window: Handle) -> i32;
        fn CloseClipboard() -> i32;
        fn RegisterClipboardFormatW(name: *const u16) -> u32;
        fn GetClipboardData(format: u32) -> Handle;
        fn SetClipboardData(format: u32, data: Handle) -> Handle;
        fn GetClipboardSequenceNumber() -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
        fn GlobalFree(data: Handle) -> Handle;
        fn GlobalLock(data: Handle) -> Handle;
        fn GlobalUnlock(data: Handle) -> i32;
        fn GlobalSize(data: Handle) -> usize;
    }
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    fn open() -> Option<Open> {
        (unsafe { OpenClipboard(std::ptr::null_mut()) } != 0).then(|| Open)
    }
    fn format() -> u32 {
        let name: Vec<u16> = "Rich Text Format\0".encode_utf16().collect();
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    }
    pub fn sequence() -> Option<u64> {
        let sequence = unsafe { GetClipboardSequenceNumber() };
        (sequence != 0).then_some(sequence as u64)
    }
    pub fn rtf() -> Option<Vec<u8>> {
        let _open = open()?;
        unsafe {
            let data = GetClipboardData(format());
            if data.is_null() {
                return None;
            }
            let len = GlobalSize(data);
            if len == 0 || len > MAX_RICH_BYTES {
                return None;
            }
            let ptr = GlobalLock(data);
            if ptr.is_null() {
                return None;
            }
            let mut bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), len).to_vec();
            GlobalUnlock(data);
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            Some(bytes)
        }
    }
    pub fn put_rtf(bytes: &[u8]) -> bool {
        let Some(_open) = open() else {
            return false;
        };
        unsafe {
            let data = GlobalAlloc(0x0042, bytes.len() + 1); // movable, zero initialized
            if data.is_null() {
                return false;
            }
            let ptr = GlobalLock(data);
            if ptr.is_null() {
                GlobalFree(data);
                return false;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            GlobalUnlock(data);
            if SetClipboardData(format(), data).is_null() {
                GlobalFree(data);
                return false;
            }
            // The clipboard owns this HGLOBAL after successful SetClipboardData.
            true
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    pub fn sequence() -> Option<u64> {
        None
    }
    pub fn rtf() -> Option<Vec<u8>> {
        None
    }
    pub fn put_rtf(_: &[u8]) -> bool {
        false
    }
}

pub use platform::{rtf, sequence};

pub fn put_rtf(bytes: &[u8]) -> bool {
    bytes.len() <= MAX_RICH_BYTES && platform::put_rtf(bytes)
}
