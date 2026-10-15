//! GPUI key names to DOM `KeyboardEvent.key` and `.code` values.
//!
//! GPUI names a key by what is printed on it, lowercase for the named keys
//! (`enter`, `down`, `pageup`, `f1`), and has no physical key code. The DOM
//! wants `Enter`, `ArrowDown`, `PageUp`, and a `code` naming the key's position.
//! This is the only place that mapping lives; the payload builder calls it and
//! JS only reads the result.
//!
//! `code` is derived from GPUI's `key`, which is layout-dependent on every
//! platform, so printable keys assume a US QWERTY position: on AZERTY the key
//! that types `a` reports `KeyA`, where a browser reports `KeyQ`. Named keys
//! are exact, except that the numpad cannot be told apart from the main block
//! (macOS reports both Enters as `enter`).

/// What `KeyboardEvent.key` and `.code` hold for one keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomKey {
    pub key: String,
    /// Empty when the physical key cannot be derived, as in the DOM.
    pub code: String,
}

/// DOM value for a key GPUI could not name.
const UNIDENTIFIED: &str = "Unidentified";

/// `(GPUI name, DOM key, DOM code)`. Names beyond `f1..f35` that only Linux or
/// Windows produce sit at the end.
const NAMED_KEYS: &[(&str, &str, &str)] = &[
    ("enter", "Enter", "Enter"),
    ("escape", "Escape", "Escape"),
    ("tab", "Tab", "Tab"),
    ("backspace", "Backspace", "Backspace"),
    ("delete", "Delete", "Delete"),
    ("insert", "Insert", "Insert"),
    ("space", " ", "Space"),
    ("left", "ArrowLeft", "ArrowLeft"),
    ("right", "ArrowRight", "ArrowRight"),
    ("up", "ArrowUp", "ArrowUp"),
    ("down", "ArrowDown", "ArrowDown"),
    ("home", "Home", "Home"),
    ("end", "End", "End"),
    ("pageup", "PageUp", "PageUp"),
    ("pagedown", "PageDown", "PageDown"),
    // Modifiers. Only keystrokes parsed from text (`simulateKeystrokes("shift")`)
    // and Linux's `*_l` / `*_r` names reach here; the platforms report held
    // modifiers through `modifiers`, not as keys of their own.
    ("shift", "Shift", ""),
    ("control", "Control", ""),
    ("alt", "Alt", ""),
    ("platform", "Meta", ""),
    ("function", "Fn", "Fn"),
    ("shift_l", "Shift", "ShiftLeft"),
    ("shift_r", "Shift", "ShiftRight"),
    ("control_l", "Control", "ControlLeft"),
    ("control_r", "Control", "ControlRight"),
    ("alt_l", "Alt", "AltLeft"),
    ("alt_r", "Alt", "AltRight"),
    ("meta_l", "Meta", "MetaLeft"),
    ("meta_r", "Meta", "MetaRight"),
    ("super_l", "Meta", "MetaLeft"),
    ("super_r", "Meta", "MetaRight"),
    ("caps_lock", "CapsLock", "CapsLock"),
    ("num_lock", "NumLock", "NumLock"),
    ("scroll_lock", "ScrollLock", "ScrollLock"),
    ("menu", "ContextMenu", "ContextMenu"),
    ("print", "PrintScreen", "PrintScreen"),
    ("pause", "Pause", "Pause"),
    ("back", "BrowserBack", "BrowserBack"),
    ("forward", "BrowserForward", "BrowserForward"),
    ("cut", "Cut", ""),
    ("copy", "Copy", ""),
    ("paste", "Paste", ""),
    ("new", "New", ""),
    ("open", "Open", ""),
    ("save", "Save", ""),
    // Linux strips the `kp_` prefix of keypad keys, so these four lose the
    // distinction from the main block's names; the rest are digits and arrows,
    // indistinguishable from the main block.
    ("add", "+", "NumpadAdd"),
    ("subtract", "-", "NumpadSubtract"),
    ("multiply", "*", "NumpadMultiply"),
    ("divide", "/", "NumpadDivide"),
    ("decimal", ".", "NumpadDecimal"),
];

/// US layout: unshifted character, shifted character, `code`.
const PUNCTUATION: &[(char, char, &str)] = &[
    ('1', '!', "Digit1"),
    ('2', '@', "Digit2"),
    ('3', '#', "Digit3"),
    ('4', '$', "Digit4"),
    ('5', '%', "Digit5"),
    ('6', '^', "Digit6"),
    ('7', '&', "Digit7"),
    ('8', '*', "Digit8"),
    ('9', '(', "Digit9"),
    ('0', ')', "Digit0"),
    ('-', '_', "Minus"),
    ('=', '+', "Equal"),
    ('[', '{', "BracketLeft"),
    (']', '}', "BracketRight"),
    ('\\', '|', "Backslash"),
    (';', ':', "Semicolon"),
    ('\'', '"', "Quote"),
    (',', '<', "Comma"),
    ('.', '>', "Period"),
    ('/', '?', "Slash"),
    ('`', '~', "Backquote"),
];

fn function_key(name: &str) -> Option<DomKey> {
    let number = name.strip_prefix('f')?;
    let valid = !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number.parse::<u8>().is_ok_and(|n| n <= 35);
    valid.then(|| DomKey {
        key: name.to_ascii_uppercase(),
        code: name.to_ascii_uppercase(),
    })
}

fn named_key(name: &str) -> Option<DomKey> {
    NAMED_KEYS
        .iter()
        .find(|(gpui, _, _)| *gpui == name)
        .map(|(_, key, code)| DomKey {
            key: (*key).to_string(),
            code: (*code).to_string(),
        })
        .or_else(|| function_key(name))
}

fn is_text(key_char: &str) -> bool {
    !key_char.is_empty() && !key_char.chars().any(char::is_control)
}

/// The DOM key for a printable GPUI `key`: the produced character, shifted when
/// GPUI had no `key_char` to say what was typed.
fn printable_key(key: char, key_char: Option<&str>, shift: bool) -> String {
    if let Some(typed) = key_char.filter(|typed| is_text(typed)) {
        return typed.to_string();
    }
    if !shift {
        return key.to_string();
    }
    if key.is_ascii_lowercase() {
        return key.to_ascii_uppercase().to_string();
    }
    PUNCTUATION
        .iter()
        .find(|(plain, _, _)| *plain == key)
        .map_or_else(|| key.to_string(), |(_, shifted, _)| shifted.to_string())
}

fn printable_code(key: char) -> String {
    if key.is_ascii_alphabetic() {
        return format!("Key{}", key.to_ascii_uppercase());
    }
    PUNCTUATION
        .iter()
        .find(|(plain, shifted, _)| *plain == key || *shifted == key)
        .map_or_else(String::new, |(_, _, code)| (*code).to_string())
}

/// `key`, `key_char` and `shift` are the fields of `gpui::Keystroke`.
pub(crate) fn dom_key(key: &str, key_char: Option<&str>, shift: bool) -> DomKey {
    if let Some(named) = named_key(key) {
        return named;
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(single), None) => DomKey {
            key: printable_key(single, key_char, shift),
            code: printable_code(single),
        },
        _ => DomKey {
            key: UNIDENTIFIED.to_string(),
            code: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, key_char: Option<&str>, shift: bool) -> (String, String) {
        let dom = dom_key(key, key_char, shift);
        (dom.key, dom.code)
    }

    fn expect(key: &str, dom_key: &str, code: &str) {
        assert_eq!(
            pair(key, None, false),
            (dom_key.to_string(), code.to_string()),
            "GPUI key {key:?}"
        );
    }

    #[test]
    fn named_keys() {
        expect("enter", "Enter", "Enter");
        expect("escape", "Escape", "Escape");
        expect("tab", "Tab", "Tab");
        expect("backspace", "Backspace", "Backspace");
        expect("delete", "Delete", "Delete");
        expect("insert", "Insert", "Insert");
        expect("space", " ", "Space");
        expect("left", "ArrowLeft", "ArrowLeft");
        expect("right", "ArrowRight", "ArrowRight");
        expect("up", "ArrowUp", "ArrowUp");
        expect("down", "ArrowDown", "ArrowDown");
        expect("home", "Home", "Home");
        expect("end", "End", "End");
        expect("pageup", "PageUp", "PageUp");
        expect("pagedown", "PageDown", "PageDown");
    }

    #[test]
    fn function_keys() {
        expect("f1", "F1", "F1");
        expect("f12", "F12", "F12");
        expect("f35", "F35", "F35");
        expect("f36", UNIDENTIFIED, "");
        expect("f0", UNIDENTIFIED, "");
        expect("f", "f", "KeyF");
    }

    #[test]
    fn modifier_keys() {
        expect("shift", "Shift", "");
        expect("control", "Control", "");
        expect("alt", "Alt", "");
        expect("platform", "Meta", "");
        expect("shift_r", "Shift", "ShiftRight");
        expect("super_l", "Meta", "MetaLeft");
    }

    #[test]
    fn letters_digits_and_punctuation() {
        expect("a", "a", "KeyA");
        expect("z", "z", "KeyZ");
        expect("1", "1", "Digit1");
        expect("0", "0", "Digit0");
        expect("-", "-", "Minus");
        expect("=", "=", "Equal");
        expect("[", "[", "BracketLeft");
        expect(";", ";", "Semicolon");
        expect("'", "'", "Quote");
        expect(",", ",", "Comma");
        expect(".", ".", "Period");
        expect("/", "/", "Slash");
        expect("`", "`", "Backquote");
        expect("\\", "\\", "Backslash");
    }

    #[test]
    fn shift_decides_the_case_without_a_key_char() {
        assert_eq!(pair("a", None, true), ("A".into(), "KeyA".into()));
        assert_eq!(pair("1", None, true), ("!".into(), "Digit1".into()));
        assert_eq!(pair("/", None, true), ("?".into(), "Slash".into()));
    }

    #[test]
    fn key_char_is_what_was_typed() {
        assert_eq!(pair("a", Some("A"), true), ("A".into(), "KeyA".into()));
        assert_eq!(pair("s", Some("ß"), false), ("ß".into(), "KeyS".into()));
        assert_eq!(pair("q", Some("ๆ"), false), ("ๆ".into(), "KeyQ".into()));
    }

    #[test]
    fn shifted_symbol_keys_keep_their_physical_key() {
        // Linux reports the symbol itself as the key.
        assert_eq!(pair("!", Some("!"), true), ("!".into(), "Digit1".into()));
        assert_eq!(pair("<", None, true), ("<".into(), "Comma".into()));
    }

    #[test]
    fn keys_that_are_not_text_use_the_key_name() {
        // A named key never takes its key_char ("\n" for enter).
        assert_eq!(pair("enter", Some("\n"), false), ("Enter".into(), "Enter".into()));
        assert_eq!(pair("space", Some(" "), false), (" ".into(), "Space".into()));
        // A control character is not what was typed.
        assert_eq!(pair("a", Some("\u{1}"), false), ("a".into(), "KeyA".into()));
    }

    #[test]
    fn unknown_names_are_unidentified() {
        expect("hyper_l", UNIDENTIFIED, "");
        assert_eq!(pair("é", None, false), ("é".into(), String::new()));
    }
}
