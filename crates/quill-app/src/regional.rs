//! Initial paper and display units. Saved choices always take precedence.

/// Unicode CLDR supplemental measurementData, 2026-10-26:
/// https://github.com/unicode-org/cldr/blob/main/common/supplemental/supplementalData.xml
/// Paper and measurement systems are independent (Canada uses Letter and metric).
pub fn defaults(region: &str, metric: Option<bool>) -> (i32, i32, bool) {
    let region = region.to_ascii_uppercase();
    let letter = matches!(
        region.as_str(),
        "BZ" | "CA"
            | "CL"
            | "CO"
            | "CR"
            | "GT"
            | "MX"
            | "NI"
            | "PA"
            | "PH"
            | "PR"
            | "SV"
            | "US"
            | "VE"
    );
    let metric = metric.unwrap_or(!matches!(region.as_str(), "US" | "LR" | "GB" | "MM"));
    let (w, h) = if letter {
        (12240, 15840)
    } else {
        (11906, 16838)
    };
    (w, h, metric)
}

fn region(locale: &str) -> Option<String> {
    locale
        .split(['.', '@'])
        .next()?
        .split(['_', '-'])
        .skip(1)
        .take_while(|s| s.len() != 1)
        .find(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase)
}

pub fn detect() -> (i32, i32, bool) {
    #[cfg(target_os = "macos")]
    if let Some((country, metric)) = mac_region() {
        return defaults(&country, Some(metric));
    }
    #[cfg(windows)]
    if let Some((country, metric)) = windows_region() {
        return defaults(&country, metric);
    }
    let locale = |category| {
        ["LC_ALL", category, "LANG"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find_map(|value| region(&value))
    };
    // With no identifiable region retain the historical US defaults.
    let paper = locale("LC_PAPER").unwrap_or_else(|| "US".into());
    let measure = locale("LC_MEASUREMENT").unwrap_or_else(|| paper.clone());
    let (w, h, _) = defaults(&paper, None);
    (w, h, defaults(&measure, None).2)
}

#[cfg(target_os = "macos")]
fn mac_region() -> Option<(String, bool)> {
    use objc2::{class, msg_send, runtime::AnyObject};
    unsafe {
        let locale: *mut AnyObject = msg_send![class!(NSLocale), currentLocale];
        if locale.is_null() {
            return None;
        }
        let country: *mut AnyObject = msg_send![locale, countryCode];
        if country.is_null() {
            return None;
        }
        let utf8: *const std::ffi::c_char = msg_send![country, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let country = std::ffi::CStr::from_ptr(utf8).to_str().ok()?.to_string();
        let metric: bool = msg_send![locale, usesMetricSystem];
        Some((country, metric))
    }
}

#[cfg(windows)]
fn windows_region() -> Option<(String, Option<bool>)> {
    use windows::{
        Win32::Globalization::{GetLocaleInfoEx, GetUserDefaultLocaleName, LOCALE_IMEASURE},
        core::PCWSTR,
    };
    let mut locale = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut locale) };
    if n <= 1 {
        return None;
    }
    let country = region(&String::from_utf16_lossy(&locale[..n as usize - 1]))?;
    let mut value = [0u16; 4];
    let n = unsafe { GetLocaleInfoEx(PCWSTR::null(), LOCALE_IMEASURE, Some(&mut value)) };
    let metric = (n > 1).then_some(value[0] == u16::from(b'0'));
    Some((country, metric))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paper_and_units_follow_region_independently() {
        assert_eq!(defaults("US", None), (12240, 15840, false));
        assert_eq!(defaults("CA", None), (12240, 15840, true));
        assert_eq!(defaults("DE", None), (11906, 16838, true));
        assert_eq!(defaults("GB", None), (11906, 16838, false));
        assert_eq!(defaults("US", Some(true)), (12240, 15840, true));
    }
    #[test]
    fn locale_regions_are_not_guessed_from_language() {
        assert_eq!(region("en_GB.UTF-8"), Some("GB".into()));
        assert_eq!(region("zh-Hant-TW"), Some("TW".into()));
        assert_eq!(region("de-DE-u-ca-gregory"), Some("DE".into()));
        assert_eq!(region("en"), None);
        assert_eq!(region("C.UTF-8"), None);
    }
    #[test]
    fn native_region_is_available_on_mac() {
        #[cfg(target_os = "macos")]
        assert!(mac_region().is_some());
    }
}
