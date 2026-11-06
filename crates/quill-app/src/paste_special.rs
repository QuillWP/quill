//! Edit > Paste Special…: paste the Clipboard in a form of your choosing.
//! Formatted Text accepts system HTML/RTF and Quill's own copy; Unformatted Text
//! takes the format of the insertion point; Picture is an image another
//! application copied (PNG, JPEG, TIFF, GIF, WebP or BMP; on the Mac for now).

use crate::dialog::Dialog;

/// A form the Clipboard can be pasted as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Formatted,
    Unformatted,
    Picture,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Formatted => "Formatted Text",
            Form::Unformatted => "Unformatted Text",
            Form::Picture => "Picture",
        }
    }
}

/// Radio button ids, one for each form offered.
const IDS: [&str; 3] = ["form0", "form1", "form2"];

/// The dialog, offering `forms` (at least one) with the first chosen.
pub fn dialog(forms: &[Form]) -> Dialog {
    let h = 64.0 + 22.0 * forms.len() as f32;
    let mut d = Dialog::new("Paste Special", 300.0, h.max(96.0));
    d.text("Paste the Clipboard as:", 12.0, 12.0);
    for (k, f) in forms.iter().enumerate().take(IDS.len()) {
        d.radio(IDS[k], f.name(), 20.0, 36.0 + 22.0 * k as f32, 0, k == 0);
    }
    d.button("ok", "Paste", 210.0, 12.0, 78.0, true);
    d.button("cancel", "Cancel", 210.0, 40.0, 78.0, false);
    d
}

/// The form chosen in the dialog.
pub fn chosen(d: &Dialog, forms: &[Form]) -> Option<Form> {
    let id = d.radio_on(0);
    IDS.iter()
        .position(|&i| i == id)
        .and_then(|k| forms.get(k).copied())
}

/// The bytes of the picture another application copied.
#[cfg(target_os = "macos")]
pub fn clipboard_image() -> Option<Vec<u8>> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    // SAFETY: each message goes to the class or object that answers it:
    // NSPasteboard, NSString and NSData.
    unsafe {
        let board: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
        if board.is_null() {
            return None;
        }
        // Many Mac programs copy pictures as TIFF.
        for kind in [
            c"public.png",
            c"public.jpeg",
            c"public.tiff",
            c"com.compuserve.gif",
            c"org.webmproject.webp",
            c"com.microsoft.bmp",
        ] {
            let kind: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: kind.as_ptr()];
            let data: *mut AnyObject = msg_send![board, dataForType: kind];
            if data.is_null() {
                continue;
            }
            let len: usize = msg_send![data, length];
            let bytes: *const std::ffi::c_void = msg_send![data, bytes];
            if !bytes.is_null() && len > 0 {
                return Some(std::slice::from_raw_parts(bytes.cast::<u8>(), len).to_vec());
            }
        }
        None
    }
}

#[cfg(not(target_os = "macos"))]
pub fn clipboard_image() -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_the_forms_given() {
        let forms = [Form::Unformatted, Form::Picture];
        let mut d = dialog(&forms);
        assert_eq!(chosen(&d, &forms), Some(Form::Unformatted));
        d.set_on("form0", false);
        d.set_on("form1", true);
        assert_eq!(chosen(&d, &forms), Some(Form::Picture));
    }
}
