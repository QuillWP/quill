//! Windows common Print dialog and direct GDI spooler output.
use crate::canvas::Canvas;
use crate::docview::DocView;
use quill_layout::{Fonts, GlyphCache};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::*;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::core::PCWSTR;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub struct Job {
    dc: usize,
    pages: Option<(usize, usize)>,
    copies: usize,
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(HDC(self.dc as *mut _));
        }
    }
}
impl Job {
    pub fn choose(owner: &Window, pages: usize) -> Result<Option<Self>, String> {
        let RawWindowHandle::Win32(handle) =
            owner.window_handle().map_err(|e| e.to_string())?.as_raw()
        else {
            return Err("No Windows print owner.".into());
        };
        let mut dialog = PRINTDLGW {
            lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
            hwndOwner: HWND(handle.hwnd.get() as *mut _),
            Flags: PD_RETURNDC
                | PD_NOSELECTION
                | PD_HIDEPRINTTOFILE
                | PD_USEDEVMODECOPIESANDCOLLATE,
            nMinPage: 1,
            nMaxPage: pages.clamp(1, u16::MAX as usize) as u16,
            nFromPage: 1,
            nToPage: pages.clamp(1, u16::MAX as usize) as u16,
            nCopies: 1,
            ..Default::default()
        };
        unsafe {
            let accepted = PrintDlgW(&mut dialog).as_bool();
            let error = if accepted {
                0
            } else {
                CommDlgExtendedError().0
            };
            let _ = GlobalFree(Some(dialog.hDevMode));
            let _ = GlobalFree(Some(dialog.hDevNames));
            if !accepted {
                if !dialog.hDC.is_invalid() {
                    let _ = DeleteDC(dialog.hDC);
                }
                return if error == 0 {
                    Ok(None)
                } else {
                    Err(format!("Windows could not open Print (error {error:#x})."))
                };
            }
            Ok(Some(Self {
                dc: dialog.hDC.0 as usize,
                pages: (dialog.Flags.0 & PD_PAGENUMS.0 != 0)
                    .then_some((dialog.nFromPage as usize, dialog.nToPage as usize)),
                copies: dialog.nCopies.max(1) as usize,
            }))
        }
    }
    pub fn print(
        self,
        view: &mut DocView,
        fonts: &mut Fonts,
        cache: &mut GlyphCache,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<(), String> {
        let dc = HDC(self.dc as *mut _);
        let title: Vec<u16> = view.title.encode_utf16().chain(Some(0)).collect();
        let info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(title.as_ptr()),
            ..Default::default()
        };
        unsafe {
            if StartDocW(dc, &info) <= 0 {
                return Err("Windows could not start the print job.".into());
            }
        }
        let result = self.pages(view, fonts, cache, &mut progress);
        unsafe {
            if result.is_ok() {
                if EndDoc(dc) <= 0 {
                    let _ = AbortDoc(dc);
                    return Err("Windows could not finish the print job.".into());
                }
            } else {
                let _ = AbortDoc(dc);
            }
        }
        result
    }
    fn pages(
        &self,
        view: &mut DocView,
        fonts: &mut Fonts,
        cache: &mut GlyphCache,
        progress: &mut impl FnMut(usize, usize),
    ) -> Result<(), String> {
        let dc = HDC(self.dc as *mut _);
        let (pw, ph) = view.page_size();
        // One bounded 300-dpi page at a time. Extremely large custom paper uses
        // a lower resolution rather than allocating an unbounded bitmap.
        let scale = (300.0f32 / 72.0).min((16_000_000.0 / (pw * ph)).sqrt());
        if !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid document paper dimensions.".into());
        }
        let width = (pw * scale).ceil().max(1.0) as u32;
        let height = (ph * scale).ceil().max(1.0) as u32;
        let mut pixels = vec![0xFFFFFFu32; width as usize * height as usize];
        let (first, last) = self.pages.unwrap_or((1, view.pages.len()));
        let first = first.saturating_sub(1).min(view.pages.len());
        let last = last.min(view.pages.len());
        let total = last.saturating_sub(first) * self.copies;
        let mut done = 0;
        for _ in 0..self.copies {
            for page in first..last {
                pixels.fill(0xFFFFFF);
                view.prepare_pages(fonts, &[page]);
                let mut canvas = Canvas::new(&mut pixels, width, height, scale);
                canvas.printing = true;
                view.draw_page(&mut canvas, fonts, cache, false, page, 0.0, 0.0);
                unsafe {
                    if StartPage(dc) <= 0 {
                        return Err("Windows could not start a printed page.".into());
                    }
                    let physical_w = GetDeviceCaps(Some(dc), PHYSICALWIDTH);
                    let physical_h = GetDeviceCaps(Some(dc), PHYSICALHEIGHT);
                    let offset_x = GetDeviceCaps(Some(dc), PHYSICALOFFSETX);
                    let offset_y = GetDeviceCaps(Some(dc), PHYSICALOFFSETY);
                    let dpi_x = GetDeviceCaps(Some(dc), LOGPIXELSX) as f32;
                    let dpi_y = GetDeviceCaps(Some(dc), LOGPIXELSY) as f32;
                    if dpi_x <= 0.0 || dpi_y <= 0.0 || physical_w <= 0 || physical_h <= 0 {
                        return Err("Windows returned invalid printer paper dimensions.".into());
                    }
                    let ratio = (physical_w as f32 * 72.0 / dpi_x / pw)
                        .min(physical_h as f32 * 72.0 / dpi_y / ph);
                    let dest_w = (pw * ratio * dpi_x / 72.0).round() as i32;
                    let dest_h = (ph * ratio * dpi_y / 72.0).round() as i32;
                    let bitmap = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: width as i32,
                            biHeight: -(height as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    SetStretchBltMode(dc, HALFTONE);
                    let lines = StretchDIBits(
                        dc,
                        (physical_w - dest_w) / 2 - offset_x,
                        (physical_h - dest_h) / 2 - offset_y,
                        dest_w,
                        dest_h,
                        0,
                        0,
                        width as i32,
                        height as i32,
                        Some(pixels.as_ptr().cast()),
                        &bitmap,
                        DIB_RGB_COLORS,
                        SRCCOPY,
                    );
                    if lines == 0 || lines == -1 {
                        return Err("Windows could not render a printed page.".into());
                    }
                    if EndPage(dc) <= 0 {
                        return Err("Windows could not finish a printed page.".into());
                    }
                }
                done += 1;
                progress(done, total);
            }
        }
        Ok(())
    }
}
