//! Preferences > Toolbar > Customize…: give any toolbar position another
//! picture and another command. Laid out from the original;
//! the changes are kept in the settings.

use crate::commands::{ALL, Cmd, code_name};
use crate::dialog::Dialog;
use crate::toolbar;

#[derive(Clone, Debug)]
pub struct CustomizeState {
    cmds: Vec<Cmd>,
    command: usize,
    /// The picture chosen.
    picture: usize,
}

fn position_names() -> Vec<String> {
    (0..toolbar::positions())
        .map(|i| {
            let (_, cmd) = toolbar::slot(i);
            format!("{}. {}", i + 1, cmd.map_or("(none)", |c| c.name()))
        })
        .collect()
}

fn picture_names() -> Vec<String> {
    (0..toolbar::pictures())
        .map(|index| format!("Icon {} - {}", index + 1, toolbar::picture_name(index)))
        .collect()
}

pub fn dialog() -> (Dialog, CustomizeState) {
    let mut d = Dialog::new("Customize", 418.0, 280.0);
    d.close_box = true;
    d.text("Button Position:", 13.0, 8.0);
    d.popup_owned("position", position_names(), 0, 13.0, 24.0, 302.0);
    d.text("Button:", 13.0, 52.0);
    d.popup_owned("pictures", picture_names(), 0, 13.0, 69.0, 304.0);
    d.text("Command:", 13.0, 100.0);
    let mut cmds: Vec<Cmd> = ALL.to_vec();
    cmds.retain(|cmd| *cmd != Cmd::RepaginateNow);
    cmds.sort_by_key(|c| c.name().to_lowercase());
    let command_names = cmds.iter().map(|cmd| cmd.name().to_string()).collect();
    d.popup_owned("commands", command_names, 0, 13.0, 117.0, 304.0);
    d.group("Command Description", 13.0, 148.0, 386.0, 80.0);
    for (id, y) in [
        ("desc0", 164.0),
        ("desc1", 178.0),
        ("desc2", 192.0),
        ("desc3", 206.0),
        ("desc4", 220.0),
    ] {
        d.note(id, "", 20.0, y, 370.0);
    }
    d.note(
        "note1",
        "To change a button, first select a button position, then choose a",
        104.0,
        240.0,
        300.0,
    );
    d.note(
        "note2",
        "new button and command from the lists, and click Change.",
        104.0,
        252.0,
        300.0,
    );
    d.button("cancel", "Close", 327.0, 12.0, 73.0, true);
    d.button("revert", "Cancel", 329.0, 42.0, 71.0, false);
    d.button("change", "Change", 329.0, 70.0, 71.0, false);
    d.button("reset", "Reset…", 329.0, 96.0, 71.0, false);
    let mut st = CustomizeState {
        cmds,
        command: usize::MAX,
        picture: 0,
    };
    show_position(&mut st, &mut d);
    (d, st)
}

/// Choose the picture and command the chosen position has now.
fn show_position(st: &mut CustomizeState, d: &mut Dialog) {
    let (pic, cmd) = toolbar::slot(d.selected("position"));
    st.picture = pic;
    st.command = cmd
        .and_then(|c| st.cmds.iter().position(|x| *x == c))
        .unwrap_or(usize::MAX);
    d.set_selected("pictures", st.picture);
    if st.command != usize::MAX {
        d.set_selected("commands", st.command);
    }
    d.enable("change", false);
    refresh(st, d);
}

/// Whether the choice differs from what the position has.
fn refresh(st: &CustomizeState, d: &mut Dialog) {
    let (pic, cmd) = toolbar::slot(d.selected("position"));
    let chosen = st.cmds.get(st.command).copied();
    d.enable(
        "change",
        chosen.is_some() && (pic != st.picture || cmd != chosen),
    );
    let description = chosen
        .map(crate::commands_dialog::describe)
        .unwrap_or_default();
    let mut lines = vec![String::new()];
    for word in description.split_whitespace() {
        let should_wrap = lines
            .last()
            .is_some_and(|line| !line.is_empty() && line.len() + word.len() + 1 > 48)
            && lines.len() < 5;
        if should_wrap {
            lines.push(word.to_string());
        } else {
            let line = lines.last_mut().expect("description always has a line");
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    for index in 0..5 {
        d.set_label(
            match index {
                0 => "desc0",
                1 => "desc1",
                2 => "desc2",
                3 => "desc3",
                _ => "desc4",
            },
            lines.get(index).map_or("", String::as_str),
        );
    }
}

pub fn changed(st: &mut CustomizeState, d: &mut Dialog, id: &str) {
    if id == "position" {
        show_position(st, d);
    } else if id == "pictures" {
        st.picture = d.selected("pictures").min(toolbar::pictures() - 1);
        refresh(st, d);
    } else if id == "commands" {
        st.command = d.selected("commands").min(st.cmds.len() - 1);
        refresh(st, d);
    }
}

/// Change, Reset… Returns true when the window should close.
pub fn button(st: &mut CustomizeState, d: &mut Dialog, id: &str) -> bool {
    let position = d.selected("position");
    match id {
        "change" => {
            if let Some(cmd) = st.cmds.get(st.command).copied() {
                let picture = st.picture;
                crate::settings::update(|s| {
                    s.toolbar_buttons.retain(|(p, _, _)| *p != position);
                    s.toolbar_buttons.push((position, picture, code_name(cmd)));
                });
            }
        }
        "reset" => crate::settings::update(|s| s.toolbar_buttons.clear()),
        "revert" => return true,
        _ => {}
    }
    d.set_options("position", position_names(), position);
    show_position(st, d);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_gets_a_new_picture_and_command() {
        let (mut d, mut st) = dialog();
        assert!(
            !d.items
                .iter()
                .any(|item| matches!(item.kind, crate::dialog::Kind::Custom))
        );
        d.set_selected("position", 3);
        changed(&mut st, &mut d, "position");
        st.picture = 0;
        st.command = st.cmds.iter().position(|c| *c == Cmd::WordCount).unwrap();
        button(&mut st, &mut d, "change");
        assert_eq!(toolbar::slot(3), (0, Some(Cmd::WordCount)));
        button(&mut st, &mut d, "reset");
        assert_eq!(toolbar::slot(3).0, 3);
    }
}
