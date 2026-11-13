//! Offscreen rendering for tests and command-line snapshots.

use super::*;

/// Render a window offscreen to a PNG, optionally after a script of steps
/// separated by `|`: `type:some text` types, `scroll:300` scrolls to that many points down, `cmd:Bold` runs a command
/// (names as in `Cmd`), `shift:Left` runs a movement command extending the
/// selection, `click:x,y`, `down:x,y`, `move:x,y` and `up:x,y` work the
/// mouse. For tests and for checking the look without a screen.
pub fn snapshot(
    out: &std::path::Path,
    width: u32,
    height: u32,
    scale: f32,
    doc: Option<Document>,
    script: &str,
) {
    let catalog = Catalog::scan_system();
    crate::font_names::set(&catalog);
    let mut fonts = Fonts::new(catalog);
    let font_names: Vec<String> = fonts.family_names().iter().map(|s| s.to_string()).collect();
    let font_families = fonts.family_info();
    let windows = vec![("Untitled1".to_string(), true)];
    let env = Env {
        font_names: &font_names,
        font_families: &font_families,
        recent: &[],
        windows: &windows,
    };
    let mut cache = GlyphCache::new();
    let mut doc = doc.unwrap_or_else(|| crate::settings::get().new_document());
    // As when the app opens a file.
    crate::font_names::use_family_names(&mut doc);
    let mut st = WinState::new(DocView::new(doc, "Untitled1".into()));
    st.ui = ui_scale(scale);
    // There is no system to follow offscreen: Auto draws light.
    st.dark = crate::settings::with(|s| s.appearance).is_dark(false);
    let mut clip = Clipboard::new();
    let (pw, ph) = (
        (width as f32 * scale) as u32,
        (height as f32 * scale) as u32,
    );
    let mut buf = vec![0u32; (pw * ph) as usize];
    let find = |name: &str| {
        command_map::ALL
            .iter()
            .find(|c| format!("{c:?}") == name)
            .copied()
    };
    st.paint(
        &mut Canvas::new(&mut buf, pw, ph, scale),
        &mut fonts,
        &mut cache,
        &env,
        true,
    );
    for step in script.split('|').filter(|s| !s.is_empty()) {
        // The window lays out what it shows between two events.
        st.view.prepare(&mut fonts);
        let (kind, arg) = step.split_once(':').unwrap_or((step, ""));
        let (x, y) = arg
            .split_once(',')
            .map(|(x, y)| (x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0)))
            .unwrap_or((0.0, 0.0));
        let mut label = Label {
            fonts: &mut fonts,
            cache: &mut cache,
        };
        match kind {
            // Draw as macOS dark mode does.
            "dark" => st.dark = true,
            // Let idle jobs run, as they do between native window events.
            "idle" => {
                let duration = Duration::from_millis(arg.parse::<u64>().unwrap_or(800).min(5000));
                let until = Instant::now() + duration;
                while Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(10));
                    st.tick();
                }
            }
            "type" => {
                if st.zoom_edit.is_some() {
                    st.zoom_text(arg);
                } else {
                    st.view.type_text(&arg.replace("\\n", "\n"));
                }
            }
            "zoomchoice" => {
                if let Ok(choice) = arg.parse() {
                    st.choose_zoom(choice);
                }
            }
            "zoomkey" => st.zoom_key(
                match arg {
                    "Escape" => DKey::Escape,
                    "Backspace" => DKey::Backspace,
                    _ => DKey::Return,
                },
                None,
            ),
            // Lay out again now and say how long it took: for measuring.
            "relayout" => {
                let t = std::time::Instant::now();
                st.view.update_layout(label.fonts);
                eprintln!(
                    "layout {:.2} ms, {} paragraphs, {} pages",
                    t.elapsed().as_secs_f64() * 1000.0,
                    st.view.doc.main.paras.len(),
                    st.view.pages.len()
                );
            }
            // A settings line, as the settings file has it: setting:key = value.
            "setting" => {
                let text = format!("{}{arg}\n", crate::settings::get().to_text());
                crate::settings::update(|s| *s = crate::settings::Settings::from_text(&text));
                st.dark = crate::settings::with(|s| s.appearance).is_dark(st.dark);
            }
            // Set only the screen magnification, leaving document measurements alone.
            "zoom" => {
                if let Ok(percent) = arg.parse::<f32>() {
                    st.document_zoom = (percent / 100.0).clamp(0.25, 5.0);
                }
            }
            "cmd" | "shift" => match find(arg) {
                Some(c) => match if kind == "shift" {
                    st.view.exec(c, true, &mut clip)
                } else {
                    st.act(Action::Cmd(c), &mut clip)
                } {
                    Effect::Dialog(DialogId::ShowClipboard) => {
                        st.open_dialog(DialogId::ShowClipboard, &env);
                        let text = clip.fragment().map(|f| f.plain_text()).unwrap_or_default();
                        st.set_clipboard_text(text);
                    }
                    Effect::Dialog(d) => st.open_dialog(d, &env),
                    Effect::FindAgain => st.find_again(),
                    Effect::Split => st.toggle_split(),
                    Effect::Pane(story) => st.open_pane(story),
                    Effect::OpenNote(story, note) => st.open_note_in(story, note),
                    Effect::ReplyToAnnotation => st.annotation_command(true),
                    Effect::ResolveAnnotation => st.annotation_command(false),
                    Effect::DeleteAnnotation => st.delete_annotation(),
                    Effect::Zoom(step) => st.zoom_step(step),
                    _ => {}
                },
                None => eprintln!("quill: unknown command {arg}"),
            },
            // Put the insertion point in paragraph x, y bytes in:
            // caret:120,0.
            "caret" => {
                let paras = &st.view.doc.main.paras;
                let para = (x as usize).min(paras.len() - 1);
                let text = &paras[para].text;
                let mut at = (y as usize).min(text.len());
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                st.view.sel = quill_core::Selection::caret(quill_core::Position::new(para, at));
            }
            "click" => {
                st.mouse_down(x, y, Mods::default(), &mut label, &env, &mut clip);
                st.mouse_up(x, y, &mut clip);
            }
            "context" => {
                st.open_context(x, y, &mut label, &mut clip);
            }
            "down" => {
                st.mouse_down(x, y, Mods::default(), &mut label, &env, &mut clip);
            }
            "move" => {
                st.mouse_move(x, y, &mut label, &env);
            }
            "tip" => {
                if let Some(tip) = &mut st.tip {
                    tip.shown = true;
                }
            }
            "item" => {
                // Click a control in the front dialog by its id.
                let at = st.dialogs.last().and_then(|(_, d)| d.item_center(arg));
                match at {
                    Some((cx, cy)) => {
                        let (x, y) = (cx * st.ui, cy * st.ui);
                        st.mouse_down(x, y, Mods::default(), &mut label, &env, &mut clip);
                        st.mouse_up(x, y, &mut clip);
                    }
                    None => eprintln!("quill: no dialog item {arg}"),
                }
            }
            "table" => {
                // Insert a table at the insertion point: table:rows,cols.
                let (r, c) = (x as usize, y as usize);
                let v = &mut st.view;
                let w = v.doc.props.text_width() / c.max(1) as i32;
                v.doc.begin("Insert Table", v.sel);
                let at = v
                    .doc
                    .insert_table(quill_core::StoryId::Main, v.sel.focus, r, c, w);
                crate::table_dialogs::border_new_table(v, at);
                v.sel = quill_core::Selection::caret(at);
                let sel = v.sel;
                v.doc.commit(sel);
            }
            // Open the color palette, starting on a color as an HTML code
            // (palette:#336699), or its editor (editcolor:#336699).
            "palette" => {
                let start = crate::palette::parse_hex(arg);
                let (d, ps) = crate::palette::palette(start, &st.view.doc);
                st.dialogs.push((Which::Palette(ps), d));
            }
            "editcolor" => {
                let start = crate::palette::parse_hex(arg).unwrap_or(quill_core::props::Rgb::BLACK);
                let (d, es) = crate::palette::editor(start, crate::palette::Target::Add);
                st.dialogs.push((Which::EditColor(es), d));
            }
            "pdf" => {
                // Write every page to a PDF file: pdf:/path/out.pdf.
                let v = st.main_view_mut();
                v.headers_wanted = true;
                v.update_layout(label.fonts);
                let n = v.pages.len();
                let bytes = crate::pdf::write(v, label.fonts, label.cache, 0..n);
                if let Err(e) = std::fs::write(arg, bytes) {
                    eprintln!("quill: cannot write {arg}: {e}");
                }
            }
            "picture" => {
                // Insert a picture file: picture:/path/to/file.png.
                match std::fs::read(arg)
                    .ok()
                    .and_then(quill_formats::images::picture)
                {
                    Some(pic) => st.view.insert_picture(pic),
                    None => eprintln!("quill: cannot read picture {arg}"),
                }
            }
            // Scroll the window's text to a distance down: scroll:2000.
            "scroll" => st.view.scroll_y = arg.parse().unwrap_or(0.0),
            // Put the insertion point at a paragraph and offset and show it:
            // goto:92,771.
            "goto" => {
                st.view
                    .set_selection(quill_core::Selection::caret(quill_core::Position::new(
                        x as usize, y as usize,
                    )));
                st.view.scroll_to_caret();
            }
            "pane" => {
                // Open a header or footer pane: pane:header, pane:footer, or
                // pane:main to put the focus back on the main text.
                match arg {
                    "header" => st.open_pane(quill_core::StoryId::Header),
                    "footer" => st.open_pane(quill_core::StoryId::Footer),
                    "close" => st.close_pane(),
                    _ => st.focus_main(),
                }
            }
            "pick" => {
                // Choose a popup item in the front dialog: pick:shading=3.
                let (id, n) = arg.split_once('=').unwrap_or((arg, "0"));
                let n: usize = n.parse().unwrap_or(0);
                let found = st.dialogs.last_mut().and_then(|(_, d)| {
                    let id = d.items.iter().find(|i| i.id == id).map(|i| i.id)?;
                    d.set_selected(id, n);
                    Some(id)
                });
                match found {
                    Some(id) => st.dialog_outcome(Outcome::Changed(id), &mut clip),
                    None => eprintln!("quill: no dialog item {id}"),
                }
            }
            "key" => {
                let k = match arg {
                    "return" => DKey::Return,
                    "escape" => DKey::Escape,
                    "tab" => DKey::Tab { back: false },
                    "backspace" => DKey::Backspace,
                    "all" => DKey::SelectAll,
                    _ => DKey::Char,
                };
                let text = if k == DKey::Char { Some(arg) } else { None };
                st.dialog_key(k, text, &mut clip);
            }
            "up" => {
                st.mouse_up(x, y, &mut clip);
            }
            _ => eprintln!("quill: unknown step {step}"),
        }
        st.view.update_layout(&mut fonts);
        st.paint(
            &mut Canvas::new(&mut buf, pw, ph, scale),
            &mut fonts,
            &mut cache,
            &env,
            true,
        );
    }
    if st.menu.is_none() {
        st.view.scroll_to_caret();
    }
    st.paint(
        &mut Canvas::new(&mut buf, pw, ph, scale),
        &mut fonts,
        &mut cache,
        &env,
        true,
    );
    if let Err(e) = crate::png::write(out, pw, ph, &buf) {
        eprintln!("quill: cannot write {}: {e}", out.display());
    }
}
