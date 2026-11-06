//! The original's menus in the Mac's real menu bar at the top of the screen.
//!
//! The menus come from the same `app_menus::build` the drawn menu bar
//! uses. After every action the app calls `sync`, which updates labels
//! ("Undo Typing"), checkmarks and grayed-out items in place, and rebuilds
//! the bar only when its shape changes (a window opened or closed).

use crate::app_menus::Action;
use crate::menu::{Item, Menu};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use objc2_06::rc::Retained;
use objc2_app_kit_03::{NSFont, NSFontAttributeName, NSFontManager, NSFontTraitMask};
use objc2_foundation_03::{MainThreadMarker, NSAttributedString, NSDictionary, NSString};

pub struct NativeMenu {
    /// Kept alive while it is shown.
    _bar: muda::Menu,
    /// One per menu item, in build order, with the item's id in `actions`.
    items: Vec<(NativeItem, u32)>,
    /// Labels, enabled and checked as last shown, to update only changes.
    shown: Vec<(String, bool, bool)>,
    /// The shape last built: menu titles and item counts.
    shape: Vec<(String, usize)>,
    pub actions: Vec<Action>,
}

/// Item ids are "a<index into actions>".
pub fn action_index(id: &muda::MenuId) -> Option<usize> {
    id.0.strip_prefix('a').and_then(|n| n.parse().ok())
}

/// Turn a shortcut label like "⌘⇧Z" into an accelerator.
fn accelerator(label: &str) -> Option<Accelerator> {
    if label.is_empty() {
        return None;
    }
    let mut mods = Modifiers::empty();
    let mut key = None;
    for c in label.chars() {
        match c {
            '⌘' => mods |= Modifiers::META,
            '⇧' => mods |= Modifiers::SHIFT,
            '⌥' => mods |= Modifiers::ALT,
            '⌃' => mods |= Modifiers::CONTROL,
            c => key = Some(c),
        }
    }
    let code = match key?.to_ascii_uppercase() {
        c @ 'A'..='Z' => {
            const LETTERS: [Code; 26] = [
                Code::KeyA,
                Code::KeyB,
                Code::KeyC,
                Code::KeyD,
                Code::KeyE,
                Code::KeyF,
                Code::KeyG,
                Code::KeyH,
                Code::KeyI,
                Code::KeyJ,
                Code::KeyK,
                Code::KeyL,
                Code::KeyM,
                Code::KeyN,
                Code::KeyO,
                Code::KeyP,
                Code::KeyQ,
                Code::KeyR,
                Code::KeyS,
                Code::KeyT,
                Code::KeyU,
                Code::KeyV,
                Code::KeyW,
                Code::KeyX,
                Code::KeyY,
                Code::KeyZ,
            ];
            LETTERS[(c as u8 - b'A') as usize]
        }
        ']' => Code::BracketRight,
        '[' => Code::BracketLeft,
        '=' => Code::Equal,
        '-' => Code::Minus,
        '.' | '>' => Code::Period,
        ',' | '<' => Code::Comma,
        '/' => Code::Slash,
        '\\' => Code::Backslash,
        '`' => Code::Backquote,
        '␣' => Code::Space,
        '⌅' => Code::NumpadEnter,
        _ => return None,
    };
    Some(Accelerator::new(mods, code))
}

fn shape(menus: &[Menu]) -> Vec<(String, usize)> {
    menus
        .iter()
        .map(|m| {
            let items = m
                .items
                .iter()
                .map(|i| 1 + i.submenu.as_ref().map_or(0, |s| s.items.len()))
                .sum();
            (m.title.clone(), items)
        })
        .collect()
}

/// Give a native menu item an AppKit attributed title when the shared menu
/// model requests a font preview. AppKit still supplies the menu behavior,
/// scrolling, accessibility and selection highlighting.
fn font_preview(model: &Item) -> Option<Retained<NSAttributedString>> {
    let Some(font_name) = &model.font else {
        return None;
    };
    let mut font = NSFont::fontWithName_size(&NSString::from_str(font_name), 14.0)?;
    let mut traits = NSFontTraitMask::empty();
    if model.style.bold {
        traits |= NSFontTraitMask::BoldFontMask;
    }
    if model.style.italic {
        traits |= NSFontTraitMask::ItalicFontMask;
    }
    if !traits.is_empty()
        && let Some(mtm) = MainThreadMarker::new()
    {
        font = NSFontManager::sharedFontManager(mtm).convertFont_toHaveTrait(&font, traits);
    };
    let values = [Retained::into_super(Retained::into_super(font))];
    let attrs = NSDictionary::from_retained_objects(&[unsafe { NSFontAttributeName }], &values);
    // SAFETY: NSFontAttributeName's value is the NSFont above, which is the
    // attribute type AppKit requires.
    Some(unsafe {
        NSAttributedString::new_with_attributes(&NSString::from_str(&model.label), &attrs)
    })
}

enum NativeItem {
    Action(MenuItem),
    State(CheckMenuItem),
}

impl NativeItem {
    fn new(model: &Item) -> Self {
        Self::with_prefix(model, 'a')
    }

    /// An item whose id is `prefix` and its action's index.
    fn with_prefix(model: &Item, prefix: char) -> Self {
        let label = model.label.replace('&', "&&");
        let id = format!("{prefix}{}", model.id);
        let item = if model.checkable {
            Self::State(CheckMenuItem::with_id(
                id,
                &label,
                model.enabled,
                model.checked,
                accelerator(&model.key),
            ))
        } else {
            Self::Action(MenuItem::with_id(
                id,
                &label,
                model.enabled,
                accelerator(&model.key),
            ))
        };
        item.preview(model);
        item
    }

    fn append(&self, menu: &Submenu) -> muda::Result<()> {
        match self {
            Self::Action(item) => menu.append(item),
            Self::State(item) => menu.append(item),
        }
    }

    fn preview(&self, model: &Item) {
        let title = font_preview(model);
        match self {
            Self::Action(item) => item.set_attributed_title(title.as_deref()),
            Self::State(item) => item.set_attributed_title(title.as_deref()),
        }
    }

    fn set_text(&self, text: String) {
        match self {
            Self::Action(item) => item.set_text(text),
            Self::State(item) => item.set_text(text),
        }
    }

    fn set_enabled(&self, enabled: bool) {
        match self {
            Self::Action(item) => item.set_enabled(enabled),
            Self::State(item) => item.set_enabled(enabled),
        }
    }

    fn set_checked(&self, checked: bool) {
        if let Self::State(item) = self {
            item.set_checked(checked);
        }
    }
}

/// macOS adds an AutoFill submenu (contacts and passwords) to every app's
/// Edit menu. A word processor has no use for it, so it comes out each
/// time AppKit adds it: when the menu bar is set, and again later on.
/// View has its own Enter Full Screen, which follows the window it acts on;
/// keep AppKit from adding a second one at the bottom of the menu.
fn no_system_full_screen_item() {
    use objc2_06::runtime::AnyObject;
    use objc2_06::{class, msg_send};
    unsafe {
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let key = NSString::from_str("NSFullScreenMenuItemEverywhere");
        let _: () = msg_send![defaults, setBool: false, forKey: &*key];
    }
}

/// Native tooltips, such as the color palette's names, wait about a
/// second by default. Show them after the same rest as Quill's own toolbar
/// tips. Registered, not saved, so a delay set for the whole Mac still wins.
fn quick_tooltips() {
    use objc2_06::runtime::AnyObject;
    use objc2_06::{class, msg_send};
    unsafe {
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let key = NSString::from_str("NSInitialToolTipDelay");
        let millis = crate::app::TIP_DELAY.as_millis() as isize;
        let value: *mut AnyObject = msg_send![class!(NSNumber), numberWithInteger: millis];
        let wanted: *mut AnyObject =
            msg_send![class!(NSDictionary), dictionaryWithObject: value, forKey: &*key];
        let _: () = msg_send![defaults, registerDefaults: wanted];
    }
}

fn keep_autofill_out() {
    use block2::RcBlock;
    use objc2_06::runtime::AnyObject;
    use objc2_06::{class, msg_send};
    use std::ptr::NonNull;
    static WATCHING: std::sync::Once = std::sync::Once::new();
    WATCHING.call_once(|| {
        let block = RcBlock::new(|note: NonNull<AnyObject>| unsafe {
            let menu: *mut AnyObject = msg_send![note.as_ptr(), object];
            let info: *mut AnyObject = msg_send![note.as_ptr(), userInfo];
            if menu.is_null() || info.is_null() {
                return;
            }
            let key = NSString::from_str("NSMenuItemIndex");
            let index: *mut AnyObject = msg_send![info, objectForKey: &*key];
            if index.is_null() {
                return;
            }
            let i: isize = msg_send![index, integerValue];
            let item: *mut AnyObject = msg_send![menu, itemAtIndex: i];
            if item.is_null() {
                return;
            }
            let id: Option<Retained<NSString>> = msg_send![item, identifier];
            if id.is_some_and(|id| id.to_string() == "_NSMenuItemAutoFillIdentifier") {
                let _: () = msg_send![menu, removeItemAtIndex: i];
                // macOS adds more items straight after this one, so a divider
                // AutoFill leaves at the bottom is only cleared once they're in.
                let Some(menu) = Retained::<AnyObject>::retain(menu) else {
                    return;
                };
                let tidy = RcBlock::new(move || {
                    let count: isize = msg_send![&*menu, numberOfItems];
                    if count > 0 {
                        let last: *mut AnyObject = msg_send![&*menu, itemAtIndex: count - 1];
                        let divider: bool = msg_send![last, isSeparatorItem];
                        if divider {
                            let _: () = msg_send![&*menu, removeItemAtIndex: count - 1];
                        }
                    }
                });
                let main: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
                let _: () = msg_send![main, addOperationWithBlock: &*tidy];
            }
        });
        unsafe {
            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let name = NSString::from_str("NSMenuDidAddItemNotification");
            // The center keeps the observer for as long as Quill runs.
            let _: *mut AnyObject = msg_send![
                center,
                addObserverForName: &*name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
        }
    });
}

/// Context menu item ids are "c<index into the menu's actions>".
pub fn context_index(id: &str) -> Option<usize> {
    id.strip_prefix('c').and_then(|n| n.parse().ok())
}

/// Show `menu` as a native context menu in `window`, at the pointer or
/// with its top-left corner at `at` (logical points from the top-left of
/// the window's content). Returns once the menu closes; a choice arrives
/// later, like the menu bar's, with a "c" id.
pub fn show_context(menu: &Menu, window: &winit::window::Window, at: Option<(f64, f64)>) {
    use muda::ContextMenu;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let build = || -> muda::Result<muda::Menu> {
        let root = muda::Menu::new();
        for it in &menu.items {
            if it.separator {
                root.append(&PredefinedMenuItem::separator())?;
            } else if let Some(children) = &it.submenu {
                let nested = Submenu::new(it.label.replace('&', "&&"), it.enabled);
                nested.set_attributed_title(font_preview(it).as_deref());
                for child in &children.items {
                    if child.separator {
                        nested.append(&PredefinedMenuItem::separator())?;
                    } else {
                        NativeItem::with_prefix(child, 'c').append(&nested)?;
                    }
                }
                root.append(&nested)?;
            } else {
                match NativeItem::with_prefix(it, 'c') {
                    NativeItem::Action(i) => root.append(&i)?,
                    NativeItem::State(i) => root.append(&i)?,
                }
            }
        }
        Ok(root)
    };
    let Ok(root) = build() else {
        return;
    };
    // muda counts up from the bottom of the view, but winit's view counts
    // down from the top, so turn the point over before handing it on.
    // Without a point AppKit opens the menu where the pointer is.
    let height = window
        .inner_size()
        .to_logical::<f64>(window.scale_factor())
        .height;
    let position = at
        .map(|(x, y)| muda::dpi::Position::Logical(muda::dpi::LogicalPosition::new(x, height - y)));
    // SAFETY: winit owns this NSView for as long as the window is open,
    // and the menu shows and closes within this call.
    unsafe {
        root.show_context_menu_for_nsview(handle.ns_view.as_ptr().cast(), position);
    }
}

impl NativeMenu {
    /// Build the bar and show it. Menu choices arrive through `on_choice`.
    pub fn install(
        menus: &[Menu],
        actions: Vec<Action>,
        on_choice: impl Fn(muda::MenuId) + Send + Sync + 'static,
    ) -> Option<NativeMenu> {
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| on_choice(e.id)));
        let mut m = NativeMenu {
            _bar: muda::Menu::new(),
            items: Vec::new(),
            shown: Vec::new(),
            shape: Vec::new(),
            actions,
        };
        m.build(menus).ok()?;
        Some(m)
    }

    fn build(&mut self, menus: &[Menu]) -> muda::Result<()> {
        let bar = muda::Menu::new();
        let mut items = Vec::new();
        let mut shown = Vec::new();
        for (k, menu) in menus.iter().enumerate() {
            let sub = Submenu::new(&menu.title, true);
            for (n, it) in menu.items.iter().enumerate() {
                // The application menu gets the Mac's standard items before
                // its last item, Quit.
                if k == 0 && n + 1 == menu.items.len() && n > 0 {
                    sub.append(&PredefinedMenuItem::separator())?;
                    sub.append(&PredefinedMenuItem::services(None))?;
                    sub.append(&PredefinedMenuItem::separator())?;
                    sub.append(&PredefinedMenuItem::hide(None))?;
                    sub.append(&PredefinedMenuItem::hide_others(None))?;
                    sub.append(&PredefinedMenuItem::show_all(None))?;
                    sub.append(&PredefinedMenuItem::separator())?;
                }
                if it.separator {
                    sub.append(&PredefinedMenuItem::separator())?;
                    continue;
                }
                let label = it.label.replace('&', "&&");
                if let Some(children) = &it.submenu {
                    let nested = Submenu::with_id(format!("a{}", it.id), &label, it.enabled);
                    nested.set_attributed_title(font_preview(it).as_deref());
                    for child in &children.items {
                        if child.separator {
                            nested.append(&PredefinedMenuItem::separator())?;
                            continue;
                        }
                        let item = NativeItem::new(child);
                        item.append(&nested)?;
                        shown.push((child.label.clone(), child.enabled, child.checked));
                        items.push((item, child.id));
                    }
                    sub.append(&nested)?;
                    continue;
                }
                let item = NativeItem::new(it);
                item.append(&sub)?;
                shown.push((it.label.clone(), it.enabled, it.checked));
                items.push((item, it.id));
            }
            bar.append(&sub)?;
        }
        keep_autofill_out();
        no_system_full_screen_item();
        quick_tooltips();
        bar.init_for_nsapp();
        self._bar = bar;
        self.items = items;
        self.shown = shown;
        self.shape = shape(menus);
        Ok(())
    }

    /// Bring the bar up to date with freshly built menus.
    pub fn sync(&mut self, menus: &[Menu], actions: Vec<Action>) {
        self.actions = actions;
        if shape(menus) != self.shape {
            let _ = self.build(menus);
            return;
        }
        let mut visible = Vec::new();
        for menu in menus {
            for item in &menu.items {
                if let Some(children) = &item.submenu {
                    visible.extend(children.items.iter().filter(|i| !i.separator));
                } else if !item.separator {
                    visible.push(item);
                }
            }
        }
        for (k, it) in visible.into_iter().enumerate() {
            let Some((item, id)) = self.items.get_mut(k) else {
                break;
            };
            let was = &mut self.shown[k];
            if *id != it.id {
                // Same shape, different ids: rebuild to be safe.
                let _ = self.build(menus);
                return;
            }
            if was.0 != it.label {
                item.set_text(it.label.replace('&', "&&"));
                item.preview(it);
                was.0 = it.label.clone();
            }
            if was.1 != it.enabled {
                item.set_enabled(it.enabled);
                was.1 = it.enabled;
            }
            if it.checkable {
                item.set_checked(it.checked);
                was.2 = it.checked;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shortcuts() {
        assert!(accelerator("⌘B").is_some());
        assert!(accelerator("⌘⇧Z").is_some());
        assert!(accelerator("⌘⌥N").is_some());
        assert!(accelerator("⌘]").is_some());
        assert_eq!(accelerator("⌘⇧>"), accelerator("⌘⇧."));
        assert_eq!(accelerator("⌘⇧<"), accelerator("⌘⇧,"));
        assert!(accelerator("⇧⌅").is_some());
        assert!(accelerator("").is_none());
    }
}
