//! Character encoding helpers and chooser UI.

use gtk4 as gtk;
use gtk::prelude::*;

pub fn common_encodings() -> Vec<&'static str> {
    vec![
        "UTF-8",
        "UTF-16",
        "UTF-16LE",
        "UTF-16BE",
        "ISO-8859-1",
        "ISO-8859-15",
        "WINDOWS-1252",
        "WINDOWS-1251",
        "KOI8-R",
        "GB18030",
        "BIG5",
        "EUC-JP",
        "SHIFT_JIS",
        "EUC-KR",
    ]
}

pub fn encoding_combo(selected: &str, extra: &[String]) -> gtk::DropDown {
    let mut list: Vec<String> = common_encodings().into_iter().map(|s| s.to_string()).collect();
    for e in extra {
        if !list.iter().any(|x| x == e) {
            list.push(e.clone());
        }
    }
    let model = gtk::StringList::new(&list.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let drop = gtk::DropDown::new(Some(model), None::<gtk::Expression>);
    if let Some(idx) = list.iter().position(|e| e == selected) {
        drop.set_selected(idx as u32);
    }
    drop
}

pub fn selected_encoding(drop: &gtk::DropDown) -> String {
    drop.selected_item()
        .and_downcast::<gtk::StringObject>()
        .map(|o| o.string().to_string())
        .unwrap_or_else(|| "UTF-8".into())
}
