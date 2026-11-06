//! Thin adapters to the spelling service supplied by the host system.
//!
//! Quill owns the spelling window and document traversal, but not a word
//! list. macOS uses `NSSpellChecker`, Windows uses the Spell Checking API,
//! and Linux asks the desktop's Enchant broker for the user's language.

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::{NSRange, NSString};

    pub fn available() -> bool {
        !unsafe { NSSpellChecker::sharedSpellChecker().availableLanguages() }.is_empty()
    }

    pub fn check(word: &str) -> Option<bool> {
        let word = NSString::from_str(word);
        let checker = unsafe { NSSpellChecker::sharedSpellChecker() };
        let bad = unsafe { checker.checkSpellingOfString_startingAt(&word, 0) };
        Some(bad.length == 0)
    }

    pub fn suggest(word: &str, limit: usize) -> Vec<String> {
        let word = NSString::from_str(word);
        let checker = unsafe { NSSpellChecker::sharedSpellChecker() };
        let Some(words) = (unsafe {
            checker.guessesForWordRange_inString_language_inSpellDocumentWithTag(
                NSRange::new(0, word.len_utf16()),
                &word,
                None,
                0,
            )
        }) else {
            return Vec::new();
        };
        words.iter().take(limit).map(ToString::to_string).collect()
    }

    pub fn learn(word: &str) -> Result<(), String> {
        let word = NSString::from_str(word);
        unsafe { NSSpellChecker::sharedSpellChecker().learnWord(&word) };
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use std::cell::RefCell;
    use windows::Win32::Globalization::{
        GetUserDefaultLocaleName, ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree,
    };
    use windows::core::{HSTRING, PWSTR};

    enum Checker {
        Untried,
        Missing,
        Ready(ISpellChecker),
    }

    thread_local! {
        // COM interface pointers belong to the apartment that created them.
        static CHECKER: RefCell<Checker> = const { RefCell::new(Checker::Untried) };
    }

    fn create() -> Option<ISpellChecker> {
        unsafe {
            // Winit or a native dialog may already have chosen the apartment.
            // Either way, COM is ready when CoCreateInstance succeeds.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let factory: ISpellCheckerFactory =
                CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let mut locale = [0u16; 85];
            let len = GetUserDefaultLocaleName(&mut locale);
            if len <= 1 {
                return None;
            }
            let language = HSTRING::from_wide(&locale[..len as usize - 1]);
            if let Ok(checker) = factory.CreateSpellChecker(&language) {
                return Some(checker);
            }
            // The display locale need not have its matching language pack.
            // In that case use the first spell-check language Windows says
            // is installed rather than silently disabling the command.
            let languages = factory.SupportedLanguages().ok()?;
            let mut item = [PWSTR::null()];
            let mut fetched = 0;
            if languages.Next(&mut item, Some(&mut fetched)).is_err() || fetched == 0 {
                return None;
            }
            let language = item[0].to_string().ok().map(HSTRING::from);
            CoTaskMemFree(Some(item[0].as_ptr().cast()));
            factory.CreateSpellChecker(language.as_ref()?).ok()
        }
    }

    fn with_checker<R>(f: impl FnOnce(&ISpellChecker) -> R) -> Option<R> {
        CHECKER.with(|slot| {
            let mut checker = slot.borrow_mut();
            if matches!(*checker, Checker::Untried) {
                *checker = create().map_or(Checker::Missing, Checker::Ready);
            }
            match &*checker {
                Checker::Ready(checker) => Some(f(checker)),
                Checker::Untried | Checker::Missing => None,
            }
        })
    }

    pub fn available() -> bool {
        with_checker(|_| ()).is_some()
    }

    pub fn check(word: &str) -> Option<bool> {
        with_checker(|checker| unsafe {
            let word = HSTRING::from(word);
            let Ok(errors) = checker.Check(&word) else {
                return true;
            };
            let mut error = None;
            let _ = errors.Next(&mut error);
            error.is_none()
        })
    }

    pub fn suggest(word: &str, limit: usize) -> Vec<String> {
        with_checker(|checker| unsafe {
            let word = HSTRING::from(word);
            let Ok(words) = checker.Suggest(&word) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            while out.len() < limit {
                let mut item = [PWSTR::null()];
                let mut fetched = 0;
                if words.Next(&mut item, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                if let Ok(word) = item[0].to_string() {
                    out.push(word);
                }
                CoTaskMemFree(Some(item[0].as_ptr().cast()));
            }
            out
        })
        .unwrap_or_default()
    }

    pub fn learn(word: &str) -> Result<(), String> {
        with_checker(|checker| unsafe { checker.Add(&HSTRING::from(word)) })
            .ok_or_else(|| "Windows has no spelling service for the current language.".to_string())?
            .map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::sync::{Mutex, OnceLock};

    type Broker = c_void;
    type Dict = c_void;
    type Init = unsafe extern "C" fn() -> *mut Broker;
    type FreeBroker = unsafe extern "C" fn(*mut Broker);
    type Request = unsafe extern "C" fn(*mut Broker, *const c_char) -> *mut Dict;
    type FreeDict = unsafe extern "C" fn(*mut Broker, *mut Dict);
    type Check = unsafe extern "C" fn(*mut Dict, *const c_char, isize) -> c_int;
    type Suggest =
        unsafe extern "C" fn(*mut Dict, *const c_char, isize, *mut usize) -> *mut *mut c_char;
    type FreeSuggestions = unsafe extern "C" fn(*mut Dict, *mut *mut c_char);
    type Add = unsafe extern "C" fn(*mut Dict, *const c_char, isize);

    struct Enchant {
        library: *mut c_void,
        broker: *mut Broker,
        dict: *mut Dict,
        free_broker: FreeBroker,
        free_dict: FreeDict,
        check: Check,
        suggest: Suggest,
        free_suggestions: FreeSuggestions,
        add: Add,
    }

    // Enchant access is serialized by the Mutex below.
    unsafe impl Send for Enchant {}

    impl Drop for Enchant {
        fn drop(&mut self) {
            unsafe {
                (self.free_dict)(self.broker, self.dict);
                (self.free_broker)(self.broker);
                libc::dlclose(self.library);
            }
        }
    }

    impl Enchant {
        unsafe fn open() -> Option<Self> {
            let mut library = std::ptr::null_mut();
            for name in ["libenchant-2.so.2", "libenchant-2.so"] {
                let name = CString::new(name).ok()?;
                library = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY) };
                if !library.is_null() {
                    break;
                }
            }
            if library.is_null() {
                return None;
            }
            macro_rules! symbol {
                ($name:literal, $ty:ty) => {{
                    let name = concat!($name, "\0");
                    let ptr = unsafe { libc::dlsym(library, name.as_ptr().cast()) };
                    if ptr.is_null() {
                        unsafe { libc::dlclose(library) };
                        return None;
                    }
                    unsafe { std::mem::transmute::<*mut c_void, $ty>(ptr) }
                }};
            }
            let init = symbol!("enchant_broker_init", Init);
            let free_broker = symbol!("enchant_broker_free", FreeBroker);
            let request = symbol!("enchant_broker_request_dict", Request);
            let free_dict = symbol!("enchant_broker_free_dict", FreeDict);
            let check = symbol!("enchant_dict_check", Check);
            let suggest = symbol!("enchant_dict_suggest", Suggest);
            let free_suggestions = symbol!("enchant_dict_free_string_list", FreeSuggestions);
            let add = symbol!("enchant_dict_add", Add);
            let broker = unsafe { init() };
            if broker.is_null() {
                unsafe { libc::dlclose(library) };
                return None;
            }
            let locale = std::env::var("LC_ALL")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|s| !s.is_empty()))
                .or_else(|| std::env::var("LANG").ok().filter(|s| !s.is_empty()))
                .unwrap_or_else(|| "en_US".to_string());
            let language = locale.split('.').next().unwrap_or("en_US");
            let mut candidates = vec![language.to_string()];
            let alternate = if language.contains('-') {
                language.replace('-', "_")
            } else {
                language.replace('_', "-")
            };
            if alternate != language {
                candidates.push(alternate);
            }
            if matches!(language, "C" | "POSIX") {
                candidates.push("en_US".to_string());
            }
            let dict = candidates.into_iter().find_map(|language| {
                let language = CString::new(language).ok()?;
                let dict = unsafe { request(broker, language.as_ptr()) };
                (!dict.is_null()).then_some(dict)
            });
            let Some(dict) = dict else {
                unsafe {
                    free_broker(broker);
                    libc::dlclose(library);
                }
                return None;
            };
            Some(Self {
                library,
                broker,
                dict,
                free_broker,
                free_dict,
                check,
                suggest,
                free_suggestions,
                add,
            })
        }
    }

    static CHECKER: OnceLock<Mutex<Option<Enchant>>> = OnceLock::new();

    fn with_checker<R>(f: impl FnOnce(&mut Enchant) -> R) -> Option<R> {
        let checker = CHECKER.get_or_init(|| Mutex::new(unsafe { Enchant::open() }));
        let mut guard = checker.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_mut().map(f)
    }

    fn cword(word: &str) -> Option<CString> {
        CString::new(word).ok()
    }

    pub fn available() -> bool {
        with_checker(|_| ()).is_some()
    }

    pub fn check(word: &str) -> Option<bool> {
        let word = cword(word)?;
        with_checker(|e| unsafe { (e.check)(e.dict, word.as_ptr(), -1) == 0 })
    }

    pub fn suggest(word: &str, limit: usize) -> Vec<String> {
        let Some(word) = cword(word) else {
            return Vec::new();
        };
        with_checker(|e| unsafe {
            let mut count = 0;
            let words = (e.suggest)(e.dict, word.as_ptr(), -1, &mut count);
            if words.is_null() {
                return Vec::new();
            }
            let mut out = Vec::new();
            for i in 0..count.min(limit) {
                let word = *words.add(i);
                if !word.is_null()
                    && let Ok(word) = CStr::from_ptr(word).to_str()
                {
                    out.push(word.to_string());
                }
            }
            (e.free_suggestions)(e.dict, words);
            out
        })
        .unwrap_or_default()
    }

    pub fn learn(word: &str) -> Result<(), String> {
        let word = cword(word).ok_or_else(|| "The word contains an embedded NUL.".to_string())?;
        with_checker(|e| unsafe { (e.add)(e.dict, word.as_ptr(), -1) })
            .ok_or_else(|| "Linux has no Enchant spelling provider for the current language.")?;
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod platform {
    pub fn available() -> bool {
        false
    }
    pub fn check(_word: &str) -> Option<bool> {
        None
    }
    pub fn suggest(_word: &str, _limit: usize) -> Vec<String> {
        Vec::new()
    }
    pub fn learn(_word: &str) -> Result<(), String> {
        Err("This system does not provide a supported spelling service.".to_string())
    }
}

pub use platform::{available, check, learn, suggest};
