//! GPUI key names to DOM `KeyboardEvent.key` and `.code` values, and back.
//!
//! GPUI names a key by what is printed on it, lowercase for the named keys
//! (`enter`, `down`, `pageup`, `f1`). The DOM wants `Enter`, `ArrowDown`,
//! `PageUp`, and a `code` naming the key's position. This is the only place
//! that mapping lives, in both directions: the payload builder calls
//! `dom_key_of_event` and JS only reads the result, and the simulated keyboard
//! (`parse_presses`) turns Playwright's `keyboard.press` grammar back into GPUI
//! keystrokes with the same tables.
//!
//! macOS reports the physical key (`NativeKey::key_code`, see `key_codes`) and
//! the character the layout types on it. `code` comes from the first and `key`
//! from the second, as in Chrome, so on AZERTY the key that types `a` reports
//! `KeyQ`. A keystroke with no native key (a platform that does not report one,
//! or a simulated name that is a key rather than a code) derives both from
//! GPUI's `key`, assuming a US QWERTY position. Named keys are exact, except
//! that without a key code the numpad cannot be told apart from the main block.

use crate::events::key_codes;

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
    // Modifiers. Only a simulated press of the key itself (`simulateKeystrokes("Shift")`)
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

/// A table row for a DOM `key` or `code` name. The `key` pass comes first so
/// `"Shift"` is the generic modifier, not `shift_l`.
fn named_row(dom_name: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    NAMED_KEYS
        .iter()
        .find(|(_, key, _)| *key == dom_name)
        .or_else(|| {
            NAMED_KEYS
                .iter()
                .find(|(_, _, code)| !code.is_empty() && *code == dom_name)
        })
}

/// GPUI's name for `F1`..`F35`.
fn function_key_name(dom_name: &str) -> Option<String> {
    let name = format!("f{}", dom_name.strip_prefix('F')?);
    function_key(&name).map(|_| name)
}

/// The character a `Key*`, `Digit*` or punctuation `code` stands for, unshifted.
fn code_character(code: &str) -> Option<char> {
    if let Some(letter) = code.strip_prefix("Key") {
        let mut chars = letter.chars();
        return match (chars.next(), chars.next()) {
            (Some(letter), None) if letter.is_ascii_uppercase() => {
                Some(letter.to_ascii_lowercase())
            }
            _ => None,
        };
    }
    PUNCTUATION
        .iter()
        .find(|(_, _, row_code)| *row_code == code)
        .map(|(plain, _, _)| *plain)
}

/// GPUI `key` and the character that key types, for one typed character.
/// US layout, like `printable_code`. A character that needs shift (`A`, `!`)
/// is typed as itself without the shift modifier, as in Playwright.
fn character_key(typed: char, shift: bool) -> Option<(String, String)> {
    if typed.is_control() {
        return None;
    }
    if typed.is_ascii_alphabetic() {
        let upper = typed.is_ascii_uppercase() || shift;
        let text = if upper {
            typed.to_ascii_uppercase()
        } else {
            typed
        };
        return Some((typed.to_ascii_lowercase().to_string(), text.to_string()));
    }
    if let Some((plain, shifted, _)) = PUNCTUATION
        .iter()
        .find(|(plain, shifted, _)| *plain == typed || *shifted == typed)
    {
        let text = if *shifted == typed || shift { *shifted } else { *plain };
        return Some((plain.to_string(), text.to_string()));
    }
    Some((typed.to_string(), typed.to_string()))
}

/// Splits on `+`, except a `+` with nothing before it is the key itself:
/// `"+"` and `"Shift++"` are the plus key. Playwright's rule.
fn split_press(press: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (index, ch) in press.char_indices() {
        if ch == '+' && index > start {
            parts.push(&press[start..index]);
            start = index + 1;
        }
    }
    parts.push(&press[start..]);
    parts
}

fn unknown_key(name: &str, press: &str) -> String {
    format!(
        "Unknown key {name:?} in {press:?}. A key is a DOM key value (\"Enter\", \"a\", \"ArrowDown\") \
         or code (\"Space\", \"KeyA\", \"Digit1\"), and modifiers join with \"+\" \
         (\"Shift+Tab\", \"Meta+a\", \"ControlOrMeta+Shift+P\")"
    )
}

/// One simulated key: the keystroke a platform would report, and the physical
/// key when the press named a `code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Press {
    pub keystroke: gpui::Keystroke,
    pub native_key: gpui::NativeKey,
}

/// The character a `Numpad*` code types.
fn numpad_character(code: &str) -> Option<char> {
    match code {
        "NumpadMultiply" => Some('*'),
        "NumpadAdd" => Some('+'),
        "NumpadSubtract" => Some('-'),
        "NumpadDecimal" => Some('.'),
        "NumpadDivide" => Some('/'),
        "NumpadEqual" => Some('='),
        "NumpadComma" => Some(','),
        _ => {
            let digit = code.strip_prefix("Numpad")?;
            let mut chars = digit.chars();
            match (chars.next(), chars.next()) {
                (Some(digit), None) if digit.is_ascii_digit() => Some(digit),
                _ => None,
            }
        }
    }
}

/// One Playwright `keyboard.press` key, as the keystroke a platform would
/// report for it: `"Enter"`, `"a"`, `"Shift+Tab"`, `"Meta+a"`, `"Space"`,
/// `"KeyA"`, `"ControlOrMeta+Shift+P"`. Names are case-sensitive, as in the DOM.
///
/// A name that is a `code` and not a `key` (`"KeyA"`, `"Digit1"`, `"ShiftLeft"`,
/// `"NumpadEnter"`) also carries the physical key, so the event reports that
/// `code` and, for a character key, the character the US layout types on it.
/// A name that is a `key` (`"a"`, `"Enter"`) carries none and `code` is derived
/// from it.
///
/// `key_char` is what the key types, and is absent while Control, Alt or Meta
/// is held, as on a real keyboard.
pub(crate) fn parse_press(press: &str) -> Result<Press, String> {
    let parts = split_press(press);
    let (key, modifier_names) = parts.split_last().expect("split_press never returns nothing");
    let mut modifiers = gpui::Modifiers::default();
    for name in modifier_names {
        match *name {
            "Shift" => modifiers.shift = true,
            "Control" => modifiers.control = true,
            "Alt" => modifiers.alt = true,
            "Meta" => modifiers.platform = true,
            "ControlOrMeta" if cfg!(target_os = "macos") => modifiers.platform = true,
            "ControlOrMeta" => modifiers.control = true,
            other => {
                return Err(format!(
                    "{other:?} in {press:?} is not a modifier. The modifiers are \
                     Shift, Control, Alt, Meta and ControlOrMeta"
                ))
            }
        }
    }

    let mut chars = key.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    };
    // Whether `key` names a physical key rather than what it types, and the
    // character the layout types on it.
    let mut names_code = false;
    let mut layout_char = None;
    let (gpui_key, typed) = if single == Some(' ') {
        ("space".to_string(), Some(" ".to_string()))
    } else if let Some(character) = single {
        let (gpui_key, typed) = character_key(character, modifiers.shift)
            .ok_or_else(|| unknown_key(key, press))?;
        (gpui_key, Some(typed))
    } else if *key == "Space" {
        names_code = true;
        layout_char = Some(" ".to_string());
        ("space".to_string(), Some(" ".to_string()))
    } else if let Some(character) = code_character(key) {
        names_code = true;
        let (gpui_key, typed) = character_key(character, modifiers.shift)
            .ok_or_else(|| unknown_key(key, press))?;
        layout_char = Some(typed.clone());
        (gpui_key, Some(typed))
    } else if let Some(character) = numpad_character(key) {
        names_code = true;
        layout_char = Some(character.to_string());
        (character.to_string(), Some(character.to_string()))
    } else if *key == "NumpadEnter" {
        names_code = true;
        ("enter".to_string(), Some("\n".to_string()))
    } else if let Some(row) = named_row(key) {
        names_code = row.1 != *key;
        match row.1 {
            "Shift" => modifiers.shift = true,
            "Control" => modifiers.control = true,
            "Alt" => modifiers.alt = true,
            "Meta" => modifiers.platform = true,
            _ => {}
        }
        let typed = match row.0 {
            "tab" => Some("\t".to_string()),
            "enter" => Some("\n".to_string()),
            _ => None,
        };
        (row.0.to_string(), typed)
    } else if let Some(name) = function_key_name(key) {
        (name, None)
    } else {
        return Err(unknown_key(key, press));
    };

    let types_text = !(modifiers.platform || modifiers.control || modifiers.alt || modifiers.function);
    Ok(Press {
        keystroke: gpui::Keystroke {
            modifiers,
            key: gpui_key,
            key_char: typed.filter(|_| types_text),
        },
        native_key: gpui::NativeKey {
            key_code: names_code.then(|| key_codes::key_code(key)).flatten(),
            layout_char,
        },
    })
}

/// A space-separated sequence of presses, parsed whole so a typo dispatches nothing.
pub(crate) fn parse_presses(sequence: &str) -> Result<Vec<Press>, String> {
    let presses = sequence
        .split_whitespace()
        .map(parse_press)
        .collect::<Result<Vec<_>, _>>()?;
    if presses.is_empty() {
        return Err("No keys to press".to_string());
    }
    Ok(presses)
}

/// The DOM `key` and `code` of one key event. The physical key and the
/// layout's character, where the platform reported them, win over what GPUI's
/// US-assuming `key` says.
pub(crate) fn dom_key_of_event(keystroke: &gpui::Keystroke, native_key: &gpui::NativeKey) -> DomKey {
    let derived = dom_key(
        &keystroke.key,
        keystroke.key_char.as_deref(),
        keystroke.modifiers.shift,
    );
    DomKey {
        key: native_key.layout_char.clone().unwrap_or(derived.key),
        code: native_key
            .key_code
            .and_then(key_codes::code)
            .map_or(derived.code, str::to_string),
    }
}

/// `key`, `key_char` and `shift` are the fields of `gpui::Keystroke`.
fn dom_key(key: &str, key_char: Option<&str>, shift: bool) -> DomKey {
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

    fn press(source: &str) -> (String, Option<String>, [bool; 4]) {
        let keystroke = parse_press(source)
            .unwrap_or_else(|error| panic!("{source}: {error}"))
            .keystroke;
        let modifiers = keystroke.modifiers;
        (
            keystroke.key,
            keystroke.key_char,
            [
                modifiers.shift,
                modifiers.control,
                modifiers.alt,
                modifiers.platform,
            ],
        )
    }

    fn text(key: &str, typed: Option<&str>) -> (String, Option<String>) {
        (key.to_string(), typed.map(str::to_string))
    }

    #[test]
    fn presses_name_dom_keys() {
        assert_eq!(press("Enter"), ("enter".into(), Some("\n".into()), [false; 4]));
        assert_eq!(press("ArrowDown").0, "down");
        assert_eq!(press("Escape"), ("escape".into(), None, [false; 4]));
        assert_eq!(press("PageUp").0, "pageup");
        assert_eq!(press("F2"), ("f2".into(), None, [false; 4]));
        assert_eq!(press("Backspace").1, None);
    }

    #[test]
    fn presses_name_codes() {
        assert_eq!(press("Space"), ("space".into(), Some(" ".into()), [false; 4]));
        assert_eq!(press(" ").0, "space");
        assert_eq!(press("KeyA"), ("a".into(), Some("a".into()), [false; 4]));
        assert_eq!(press("Digit1"), ("1".into(), Some("1".into()), [false; 4]));
        assert_eq!(press("Minus").0, "-");
        assert_eq!(press("NumpadAdd"), ("+".into(), Some("+".into()), [false; 4]));
        assert_eq!(press("Numpad7"), ("7".into(), Some("7".into()), [false; 4]));
        assert_eq!(press("NumpadEnter"), ("enter".into(), Some("\n".into()), [false; 4]));
    }

    #[test]
    fn presses_type_the_character() {
        let (key, typed, _) = press("a");
        assert_eq!(text(&key, typed.as_deref()), text("a", Some("a")));
        // Uppercase and shifted symbols type as themselves, without Shift held.
        assert_eq!(press("A"), ("a".into(), Some("A".into()), [false; 4]));
        assert_eq!(press("!"), ("1".into(), Some("!".into()), [false; 4]));
        assert_eq!(press("Shift+b"), ("b".into(), Some("B".into()), [true, false, false, false]));
        assert_eq!(press("Shift+1"), ("1".into(), Some("!".into()), [true, false, false, false]));
        assert_eq!(press("é"), ("é".into(), Some("é".into()), [false; 4]));
    }

    #[test]
    fn presses_join_modifiers_with_plus() {
        assert_eq!(press("Shift+Tab"), ("tab".into(), Some("\t".into()), [true, false, false, false]));
        // Control, Alt and Meta stop a key from typing.
        assert_eq!(press("Control+Shift+P"), ("p".into(), None, [true, true, false, false]));
        assert_eq!(press("Alt+ArrowLeft"), ("left".into(), None, [false, false, true, false]));
        assert_eq!(press("Meta+a"), ("a".into(), None, [false, false, false, true]));
        let control_or_meta = if cfg!(target_os = "macos") {
            [false, false, false, true]
        } else {
            [false, true, false, false]
        };
        assert_eq!(press("ControlOrMeta+a"), ("a".into(), None, control_or_meta));
    }

    #[test]
    fn a_plus_with_nothing_before_it_is_the_key() {
        assert_eq!(press("+"), ("=".into(), Some("+".into()), [false; 4]));
        assert_eq!(press("Shift++").0, "=");
        assert_eq!(press("Control+-").0, "-");
    }

    #[test]
    fn pressing_a_modifier_holds_it() {
        assert_eq!(press("Shift"), ("shift".into(), None, [true, false, false, false]));
        assert_eq!(press("Meta"), ("platform".into(), None, [false, false, false, true]));
    }

    #[test]
    fn presses_reject_unknown_names() {
        for source in ["", "enter", "cmd-shift-p", "Shift+", "F36", "Ctrl+a", "a+b", "Key", "Digit"] {
            assert!(parse_press(source).is_err(), "{source:?} parsed");
        }
        let message = parse_press("cmd-p").unwrap_err();
        assert!(message.contains("Unknown key \"cmd-p\""), "{message}");
        assert!(parse_press("a+b").unwrap_err().contains("not a modifier"));
    }

    #[test]
    fn sequences_split_on_spaces_and_fail_whole() {
        let keys: Vec<String> = parse_presses("a  Shift+b Enter")
            .unwrap()
            .into_iter()
            .map(|press| press.keystroke.key)
            .collect();
        assert_eq!(keys, ["a", "b", "enter"]);
        assert!(parse_presses("a bogus").is_err());
        assert!(parse_presses("  ").is_err());
    }

    #[test]
    fn a_parsed_press_round_trips_to_its_dom_key() {
        for source in ["Enter", "ArrowDown", "Escape", "a", "A", "Shift+b", "Space", "F5", "Shift+1"] {
            let press = parse_press(source).unwrap();
            let dom = dom_key_of_event(&press.keystroke, &press.native_key);
            let expected = match source {
                "Shift+b" => "B",
                "Space" => " ",
                "Shift+1" => "!",
                other => other,
            };
            assert_eq!(dom.key, expected, "{source}");
        }
    }

    fn native(source: &str) -> (Option<u16>, Option<String>) {
        let press = parse_press(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        (press.native_key.key_code, press.native_key.layout_char)
    }

    fn event_dom_key(source: &str) -> (String, String) {
        let press = parse_press(source).unwrap();
        let dom = dom_key_of_event(&press.keystroke, &press.native_key);
        (dom.key, dom.code)
    }

    fn typed_on(key: &str, key_code: u16, layout_char: Option<&str>) -> (String, String) {
        let keystroke = gpui::Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: key.to_string(),
            key_char: None,
        };
        let native_key = gpui::NativeKey {
            key_code: Some(key_code),
            layout_char: layout_char.map(str::to_string),
        };
        let dom = dom_key_of_event(&keystroke, &native_key);
        (dom.key, dom.code)
    }

    #[test]
    fn the_layout_character_is_the_key_and_the_key_code_is_the_code() {
        // Cyrillic layout, the key in the C position: Ctrl+C reports the Cyrillic letter.
        assert_eq!(typed_on("c", 0x08, Some("с")), ("с".into(), "KeyC".into()));
        // AZERTY: the key that types "a" is in the Q position.
        assert_eq!(typed_on("a", 0x0c, Some("a")), ("a".into(), "KeyQ".into()));
        // Option+s types "ß" on the S key.
        assert_eq!(typed_on("s", 0x01, Some("ß")), ("ß".into(), "KeyS".into()));
        // Shift applied to the layout character.
        assert_eq!(typed_on("1", 0x12, Some("!")), ("!".into(), "Digit1".into()));
    }

    #[test]
    fn a_key_without_a_layout_character_keeps_its_name() {
        assert_eq!(typed_on("enter", 0x24, None), ("Enter".into(), "Enter".into()));
        assert_eq!(typed_on("enter", 0x4c, None), ("Enter".into(), "NumpadEnter".into()));
        assert_eq!(typed_on("left", 0x7b, None), ("ArrowLeft".into(), "ArrowLeft".into()));
        assert_eq!(typed_on("f5", 0x60, None), ("F5".into(), "F5".into()));
        assert_eq!(typed_on("home", 0x73, None), ("Home".into(), "Home".into()));
    }

    #[test]
    fn a_key_code_the_table_lacks_falls_back_to_the_derived_code() {
        assert_eq!(typed_on("a", 0xfff, Some("a")), ("a".into(), "KeyA".into()));
    }

    #[test]
    fn no_native_key_derives_everything_from_the_keystroke() {
        let keystroke = gpui::Keystroke {
            modifiers: gpui::Modifiers { shift: true, ..Default::default() },
            key: "a".to_string(),
            key_char: Some("A".to_string()),
        };
        let dom = dom_key_of_event(&keystroke, &gpui::NativeKey::default());
        assert_eq!((dom.key, dom.code), ("A".to_string(), "KeyA".to_string()));
    }

    #[test]
    fn a_press_that_names_a_code_carries_the_physical_key() {
        assert_eq!(native("KeyA"), (Some(0x00), Some("a".into())));
        assert_eq!(native("Shift+KeyA"), (Some(0x00), Some("A".into())));
        assert_eq!(native("Digit1"), (Some(0x12), Some("1".into())));
        assert_eq!(native("Shift+Digit1"), (Some(0x12), Some("!".into())));
        assert_eq!(native("Minus"), (Some(0x1b), Some("-".into())));
        assert_eq!(native("Space"), (Some(0x31), Some(" ".into())));
        assert_eq!(native("ShiftLeft"), (Some(0x38), None));
        assert_eq!(native("MetaRight"), (Some(0x36), None));
        assert_eq!(native("NumpadEnter"), (Some(0x4c), None));
        assert_eq!(native("Numpad3"), (Some(0x55), Some("3".into())));
        assert_eq!(native("NumpadAdd"), (Some(0x45), Some("+".into())));
    }

    #[test]
    fn control_and_meta_do_not_change_the_layout_character() {
        assert_eq!(native("Control+KeyC"), (Some(0x08), Some("c".into())));
        assert_eq!(press("Control+KeyC").1, None);
        assert_eq!(native("Meta+Shift+KeyC"), (Some(0x08), Some("C".into())));
    }

    #[test]
    fn a_press_that_names_a_key_carries_no_physical_key() {
        for source in ["a", "A", "Enter", "Escape", "ArrowLeft", "F5", "Shift+Tab", "Shift", " ", "!"] {
            assert_eq!(native(source), (None, None), "{source}");
        }
    }

    #[test]
    fn events_report_the_code_a_press_names() {
        assert_eq!(event_dom_key("KeyA"), ("a".into(), "KeyA".into()));
        assert_eq!(event_dom_key("Shift+KeyA"), ("A".into(), "KeyA".into()));
        assert_eq!(event_dom_key("Digit1"), ("1".into(), "Digit1".into()));
        assert_eq!(event_dom_key("Shift+Digit1"), ("!".into(), "Digit1".into()));
        assert_eq!(event_dom_key("Control+KeyC"), ("c".into(), "KeyC".into()));
        assert_eq!(event_dom_key("ShiftLeft"), ("Shift".into(), "ShiftLeft".into()));
        assert_eq!(event_dom_key("ShiftRight"), ("Shift".into(), "ShiftRight".into()));
        assert_eq!(event_dom_key("NumpadEnter"), ("Enter".into(), "NumpadEnter".into()));
        assert_eq!(event_dom_key("Enter"), ("Enter".into(), "Enter".into()));
        assert_eq!(event_dom_key("Numpad1"), ("1".into(), "Numpad1".into()));
        assert_eq!(event_dom_key("NumpadAdd"), ("+".into(), "NumpadAdd".into()));
        assert_eq!(event_dom_key("Space"), (" ".into(), "Space".into()));
        // A key name derives its code, as before.
        assert_eq!(event_dom_key("a"), ("a".into(), "KeyA".into()));
        assert_eq!(event_dom_key("A"), ("A".into(), "KeyA".into()));
        assert_eq!(event_dom_key("ArrowLeft"), ("ArrowLeft".into(), "ArrowLeft".into()));
    }
}
