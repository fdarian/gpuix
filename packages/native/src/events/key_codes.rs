//! macOS virtual key codes (`NSEvent.keyCode`) and the DOM `KeyboardEvent.code`
//! of the physical key, from Chromium's table (`dom_code_data.inc`, mac column).
//!
//! The code names a key's position, so it does not change with the keyboard
//! layout: the key that types `a` on QWERTY and `q` on AZERTY is `KeyA` on both.
//! The table is plain data keyed by the raw `u16`, so it is tested and used by
//! the simulated keyboard on every platform.
//!
//! It also holds the legacy `KeyboardEvent.keyCode` (a Windows virtual-key
//! code) that Chrome on macOS reports for each physical key, from Chromium's
//! `KeyboardCodeFromKeyCode` and `KeyboardCodeFromCharCode`
//! (`keyboard_code_conversion_mac.mm`).

const MAC_KEY_CODES: &[(u16, &str)] = &[
    (0x00, "KeyA"),
    (0x01, "KeyS"),
    (0x02, "KeyD"),
    (0x03, "KeyF"),
    (0x04, "KeyH"),
    (0x05, "KeyG"),
    (0x06, "KeyZ"),
    (0x07, "KeyX"),
    (0x08, "KeyC"),
    (0x09, "KeyV"),
    (0x0a, "IntlBackslash"),
    (0x0b, "KeyB"),
    (0x0c, "KeyQ"),
    (0x0d, "KeyW"),
    (0x0e, "KeyE"),
    (0x0f, "KeyR"),
    (0x10, "KeyY"),
    (0x11, "KeyT"),
    (0x12, "Digit1"),
    (0x13, "Digit2"),
    (0x14, "Digit3"),
    (0x15, "Digit4"),
    (0x16, "Digit6"),
    (0x17, "Digit5"),
    (0x18, "Equal"),
    (0x19, "Digit9"),
    (0x1a, "Digit7"),
    (0x1b, "Minus"),
    (0x1c, "Digit8"),
    (0x1d, "Digit0"),
    (0x1e, "BracketRight"),
    (0x1f, "KeyO"),
    (0x20, "KeyU"),
    (0x21, "BracketLeft"),
    (0x22, "KeyI"),
    (0x23, "KeyP"),
    (0x24, "Enter"),
    (0x25, "KeyL"),
    (0x26, "KeyJ"),
    (0x27, "Quote"),
    (0x28, "KeyK"),
    (0x29, "Semicolon"),
    (0x2a, "Backslash"),
    (0x2b, "Comma"),
    (0x2c, "Slash"),
    (0x2d, "KeyN"),
    (0x2e, "KeyM"),
    (0x2f, "Period"),
    (0x30, "Tab"),
    (0x31, "Space"),
    (0x32, "Backquote"),
    (0x33, "Backspace"),
    (0x35, "Escape"),
    (0x36, "MetaRight"),
    (0x37, "MetaLeft"),
    (0x38, "ShiftLeft"),
    (0x39, "CapsLock"),
    (0x3a, "AltLeft"),
    (0x3b, "ControlLeft"),
    (0x3c, "ShiftRight"),
    (0x3d, "AltRight"),
    (0x3e, "ControlRight"),
    (0x3f, "Fn"),
    (0x40, "F17"),
    (0x41, "NumpadDecimal"),
    (0x43, "NumpadMultiply"),
    (0x45, "NumpadAdd"),
    // The keypad's Clear key, which Chromium reports as NumLock.
    (0x47, "NumLock"),
    (0x48, "AudioVolumeUp"),
    (0x49, "AudioVolumeDown"),
    (0x4a, "AudioVolumeMute"),
    (0x4b, "NumpadDivide"),
    (0x4c, "NumpadEnter"),
    (0x4e, "NumpadSubtract"),
    (0x4f, "F18"),
    (0x50, "F19"),
    (0x51, "NumpadEqual"),
    (0x52, "Numpad0"),
    (0x53, "Numpad1"),
    (0x54, "Numpad2"),
    (0x55, "Numpad3"),
    (0x56, "Numpad4"),
    (0x57, "Numpad5"),
    (0x58, "Numpad6"),
    (0x59, "Numpad7"),
    (0x5a, "F20"),
    (0x5b, "Numpad8"),
    (0x5c, "Numpad9"),
    (0x5d, "IntlYen"),
    (0x5e, "IntlRo"),
    (0x5f, "NumpadComma"),
    (0x60, "F5"),
    (0x61, "F6"),
    (0x62, "F7"),
    (0x63, "F3"),
    (0x64, "F8"),
    (0x65, "F9"),
    (0x66, "Lang2"),
    (0x67, "F11"),
    (0x68, "Lang1"),
    (0x69, "F13"),
    (0x6a, "F16"),
    (0x6b, "F14"),
    (0x6d, "F10"),
    (0x6e, "ContextMenu"),
    (0x6f, "F12"),
    (0x71, "F15"),
    // macOS names the key above Home "Help"; Chromium reports it as Insert.
    (0x72, "Insert"),
    (0x73, "Home"),
    (0x74, "PageUp"),
    (0x75, "Delete"),
    (0x76, "F4"),
    (0x77, "End"),
    (0x78, "F2"),
    (0x79, "PageDown"),
    (0x7a, "F1"),
    (0x7b, "ArrowLeft"),
    (0x7c, "ArrowRight"),
    (0x7d, "ArrowDown"),
    (0x7e, "ArrowUp"),
];

/// The DOM `code` of a macOS virtual key code, `None` for a key the table does
/// not know.
pub(crate) fn code(key_code: u16) -> Option<&'static str> {
    MAC_KEY_CODES
        .iter()
        .find(|(candidate, _)| *candidate == key_code)
        .map(|(_, code)| *code)
}

/// The macOS virtual key code of a DOM `code`.
pub(crate) fn key_code(code: &str) -> Option<u16> {
    MAC_KEY_CODES
        .iter()
        .find(|(_, candidate)| *candidate == code)
        .map(|(key_code, _)| *key_code)
}

/// What `KeyboardEvent.keyCode` holds for a key with no legacy code: Chromium's
/// `VKEY_UNKNOWN`.
pub(crate) const VKEY_UNKNOWN: u32 = 0;

/// `KeyboardEvent.keyCode` sent for a key an input method took first, whatever
/// key it was.
pub(crate) const VKEY_PROCESSKEY: u32 = 229;

/// `(DOM code, keyCode)` of the physical key, as Chrome on macOS reports it
/// when the layout's character does not decide the code (see `key_names`).
/// Left and right modifiers share a code, except that the right Command key is
/// the "apps" key (93). The keypad's Clear key, which Chromium names NumLock,
/// is `VKEY_CLEAR` (12) on a Mac, and the keypad's equals sign is `=` (187).
/// Keys Chromium maps to nothing are listed with `VKEY_UNKNOWN`.
const VIRTUAL_KEYS: &[(&str, u32)] = &[
    ("KeyA", 65),
    ("KeyB", 66),
    ("KeyC", 67),
    ("KeyD", 68),
    ("KeyE", 69),
    ("KeyF", 70),
    ("KeyG", 71),
    ("KeyH", 72),
    ("KeyI", 73),
    ("KeyJ", 74),
    ("KeyK", 75),
    ("KeyL", 76),
    ("KeyM", 77),
    ("KeyN", 78),
    ("KeyO", 79),
    ("KeyP", 80),
    ("KeyQ", 81),
    ("KeyR", 82),
    ("KeyS", 83),
    ("KeyT", 84),
    ("KeyU", 85),
    ("KeyV", 86),
    ("KeyW", 87),
    ("KeyX", 88),
    ("KeyY", 89),
    ("KeyZ", 90),
    ("Digit0", 48),
    ("Digit1", 49),
    ("Digit2", 50),
    ("Digit3", 51),
    ("Digit4", 52),
    ("Digit5", 53),
    ("Digit6", 54),
    ("Digit7", 55),
    ("Digit8", 56),
    ("Digit9", 57),
    ("Numpad0", 96),
    ("Numpad1", 97),
    ("Numpad2", 98),
    ("Numpad3", 99),
    ("Numpad4", 100),
    ("Numpad5", 101),
    ("Numpad6", 102),
    ("Numpad7", 103),
    ("Numpad8", 104),
    ("Numpad9", 105),
    ("F1", 112),
    ("F2", 113),
    ("F3", 114),
    ("F4", 115),
    ("F5", 116),
    ("F6", 117),
    ("F7", 118),
    ("F8", 119),
    ("F9", 120),
    ("F10", 121),
    ("F11", 122),
    ("F12", 123),
    ("F13", 124),
    ("F14", 125),
    ("F15", 126),
    ("F16", 127),
    ("F17", 128),
    ("F18", 129),
    ("F19", 130),
    ("F20", 131),
    ("F21", 132),
    ("F22", 133),
    ("F23", 134),
    ("F24", 135),
    ("Enter", 13),
    ("NumpadEnter", 13),
    ("Escape", 27),
    ("Tab", 9),
    ("Backspace", 8),
    ("Space", 32),
    ("Delete", 46),
    ("Insert", 45),
    ("Home", 36),
    ("End", 35),
    ("PageUp", 33),
    ("PageDown", 34),
    ("ArrowLeft", 37),
    ("ArrowUp", 38),
    ("ArrowRight", 39),
    ("ArrowDown", 40),
    ("Minus", 189),
    ("Equal", 187),
    ("BracketLeft", 219),
    ("BracketRight", 221),
    ("Backslash", 220),
    ("Semicolon", 186),
    ("Quote", 222),
    ("Comma", 188),
    ("Period", 190),
    ("Slash", 191),
    ("Backquote", 192),
    ("IntlBackslash", 192),
    ("ShiftLeft", 16),
    ("ShiftRight", 16),
    ("ControlLeft", 17),
    ("ControlRight", 17),
    ("AltLeft", 18),
    ("AltRight", 18),
    ("MetaLeft", 91),
    ("MetaRight", 93),
    ("ContextMenu", 93),
    ("CapsLock", 20),
    ("NumpadMultiply", 106),
    ("NumpadAdd", 107),
    ("NumpadSubtract", 109),
    ("NumpadDecimal", 110),
    ("NumpadDivide", 111),
    ("NumpadEqual", 187),
    ("AudioVolumeUp", 175),
    ("AudioVolumeDown", 174),
    ("AudioVolumeMute", 173),
    ("ScrollLock", 145),
    ("PrintScreen", 44),
    ("Pause", 19),
    ("BrowserBack", 166),
    ("BrowserForward", 167),
    ("NumLock", 12),
    ("Fn", VKEY_UNKNOWN),
    ("NumpadComma", VKEY_UNKNOWN),
    ("IntlYen", VKEY_UNKNOWN),
    ("IntlRo", VKEY_UNKNOWN),
    ("Lang1", VKEY_UNKNOWN),
    ("Lang2", VKEY_UNKNOWN),
];

/// `keyCode` of a modifier named by `key` alone, which is how a simulated press
/// of `"Shift"` arrives: no physical key, so no side.
const MODIFIER_VIRTUAL_KEYS: &[(&str, u32)] = &[
    ("Shift", 16),
    ("Control", 17),
    ("Alt", 18),
    ("Meta", 91),
];

/// `keyCode` of the printable characters Chromium recognises by what they
/// type, shifted and unshifted, on a US layout.
const CHARACTER_VIRTUAL_KEYS: &[(char, u32)] = &[
    (';', 186),
    (':', 186),
    ('=', 187),
    ('+', 187),
    (',', 188),
    ('<', 188),
    ('-', 189),
    ('_', 189),
    ('.', 190),
    ('>', 190),
    ('/', 191),
    ('?', 191),
    ('`', 192),
    ('~', 192),
    ('[', 219),
    ('{', 219),
    ('\\', 220),
    ('|', 220),
    (']', 221),
    ('}', 221),
    ('\'', 222),
    ('"', 222),
];

/// The `keyCode` of a physical key by its DOM `code`.
pub(crate) fn virtual_key(code: &str) -> u32 {
    VIRTUAL_KEYS
        .iter()
        .find(|(candidate, _)| *candidate == code)
        .map_or(VKEY_UNKNOWN, |(_, virtual_key)| *virtual_key)
}

/// The `keyCode` of a modifier given only its DOM `key`.
pub(crate) fn modifier_virtual_key(key: &str) -> u32 {
    MODIFIER_VIRTUAL_KEYS
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map_or(VKEY_UNKNOWN, |(_, virtual_key)| *virtual_key)
}

/// The `keyCode` of a character a key types, for the letters, digits and
/// punctuation Chromium maps by character; `None` for any other character.
pub(crate) fn character_virtual_key(character: char) -> Option<u32> {
    if character.is_ascii_alphabetic() {
        return Some(u32::from(character.to_ascii_uppercase()));
    }
    if character.is_ascii_digit() {
        return Some(u32::from(character));
    }
    CHARACTER_VIRTUAL_KEYS
        .iter()
        .find(|(candidate, _)| *candidate == character)
        .map(|(_, virtual_key)| *virtual_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_follow_the_physical_position_not_the_alphabet() {
        for (key_code, expected) in [
            (0x00, "KeyA"),
            (0x01, "KeyS"),
            (0x06, "KeyZ"),
            (0x0c, "KeyQ"),
            (0x2e, "KeyM"),
            (0x10, "KeyY"),
        ] {
            assert_eq!(code(key_code), Some(expected));
        }
    }

    #[test]
    fn digits_and_punctuation() {
        for (key_code, expected) in [
            (0x12, "Digit1"),
            (0x16, "Digit6"),
            (0x17, "Digit5"),
            (0x1d, "Digit0"),
            (0x1b, "Minus"),
            (0x18, "Equal"),
            (0x21, "BracketLeft"),
            (0x1e, "BracketRight"),
            (0x2a, "Backslash"),
            (0x29, "Semicolon"),
            (0x27, "Quote"),
            (0x2b, "Comma"),
            (0x2f, "Period"),
            (0x2c, "Slash"),
            (0x32, "Backquote"),
        ] {
            assert_eq!(code(key_code), Some(expected));
        }
    }

    #[test]
    fn the_numpad_is_told_apart_from_the_main_block() {
        assert_eq!(code(0x24), Some("Enter"));
        assert_eq!(code(0x4c), Some("NumpadEnter"));
        assert_eq!(code(0x52), Some("Numpad0"));
        assert_eq!(code(0x5c), Some("Numpad9"));
        assert_eq!(code(0x41), Some("NumpadDecimal"));
        assert_eq!(code(0x43), Some("NumpadMultiply"));
        assert_eq!(code(0x45), Some("NumpadAdd"));
        assert_eq!(code(0x4b), Some("NumpadDivide"));
        assert_eq!(code(0x4e), Some("NumpadSubtract"));
        assert_eq!(code(0x51), Some("NumpadEqual"));
        assert_eq!(code(0x5f), Some("NumpadComma"));
        assert_eq!(code(0x47), Some("NumLock"));
    }

    #[test]
    fn international_keys() {
        assert_eq!(code(0x0a), Some("IntlBackslash"));
        assert_eq!(code(0x5d), Some("IntlYen"));
        assert_eq!(code(0x5e), Some("IntlRo"));
        assert_eq!(code(0x66), Some("Lang2"));
        assert_eq!(code(0x68), Some("Lang1"));
    }

    #[test]
    fn editing_and_navigation_keys() {
        for (key_code, expected) in [
            (0x33, "Backspace"),
            (0x75, "Delete"),
            (0x72, "Insert"),
            (0x73, "Home"),
            (0x77, "End"),
            (0x74, "PageUp"),
            (0x79, "PageDown"),
            (0x7b, "ArrowLeft"),
            (0x7c, "ArrowRight"),
            (0x7d, "ArrowDown"),
            (0x7e, "ArrowUp"),
            (0x30, "Tab"),
            (0x31, "Space"),
            (0x35, "Escape"),
            (0x6e, "ContextMenu"),
        ] {
            assert_eq!(code(key_code), Some(expected));
        }
    }

    #[test]
    fn function_keys_are_not_in_numeric_order() {
        for (key_code, expected) in [
            (0x7a, "F1"),
            (0x78, "F2"),
            (0x63, "F3"),
            (0x76, "F4"),
            (0x60, "F5"),
            (0x61, "F6"),
            (0x62, "F7"),
            (0x64, "F8"),
            (0x65, "F9"),
            (0x6d, "F10"),
            (0x67, "F11"),
            (0x6f, "F12"),
            (0x69, "F13"),
            (0x6b, "F14"),
            (0x71, "F15"),
            (0x6a, "F16"),
            (0x40, "F17"),
            (0x4f, "F18"),
            (0x50, "F19"),
            (0x5a, "F20"),
        ] {
            assert_eq!(code(key_code), Some(expected));
        }
    }

    #[test]
    fn modifier_keys_tell_left_from_right() {
        for (key_code, expected) in [
            (0x38, "ShiftLeft"),
            (0x3c, "ShiftRight"),
            (0x3b, "ControlLeft"),
            (0x3e, "ControlRight"),
            (0x3a, "AltLeft"),
            (0x3d, "AltRight"),
            (0x37, "MetaLeft"),
            (0x36, "MetaRight"),
            (0x39, "CapsLock"),
            (0x3f, "Fn"),
        ] {
            assert_eq!(code(key_code), Some(expected));
        }
    }

    #[test]
    fn volume_keys() {
        assert_eq!(code(0x48), Some("AudioVolumeUp"));
        assert_eq!(code(0x49), Some("AudioVolumeDown"));
        assert_eq!(code(0x4a), Some("AudioVolumeMute"));
    }

    #[test]
    fn a_key_the_table_does_not_know_has_no_code() {
        assert_eq!(code(0x34), None);
        assert_eq!(code(0xffff), None);
        assert_eq!(key_code("Unknown"), None);
    }

    #[test]
    fn every_row_round_trips_and_neither_column_repeats() {
        for (key_code_value, code_value) in MAC_KEY_CODES {
            assert_eq!(code(*key_code_value), Some(*code_value));
            assert_eq!(key_code(code_value), Some(*key_code_value));
        }
    }

    #[test]
    fn letters_and_digits_have_their_ascii_codes() {
        assert_eq!(virtual_key("KeyA"), 65);
        assert_eq!(virtual_key("KeyZ"), 90);
        assert_eq!(virtual_key("Digit0"), 48);
        assert_eq!(virtual_key("Digit9"), 57);
        assert_eq!(character_virtual_key('a'), Some(65));
        assert_eq!(character_virtual_key('Q'), Some(81));
        assert_eq!(character_virtual_key('7'), Some(55));
    }

    #[test]
    fn punctuation_keys_have_the_oem_codes() {
        for (code, character, shifted, expected) in [
            ("Semicolon", ';', ':', 186),
            ("Equal", '=', '+', 187),
            ("Comma", ',', '<', 188),
            ("Minus", '-', '_', 189),
            ("Period", '.', '>', 190),
            ("Slash", '/', '?', 191),
            ("Backquote", '`', '~', 192),
            ("BracketLeft", '[', '{', 219),
            ("Backslash", '\\', '|', 220),
            ("BracketRight", ']', '}', 221),
            ("Quote", '\'', '"', 222),
        ] {
            assert_eq!(virtual_key(code), expected, "{code}");
            assert_eq!(character_virtual_key(character), Some(expected), "{character}");
            assert_eq!(character_virtual_key(shifted), Some(expected), "{shifted}");
        }
        assert_eq!(virtual_key("IntlBackslash"), 192);
        assert_eq!(character_virtual_key('é'), None);
        assert_eq!(character_virtual_key('ф'), None);
    }

    #[test]
    fn the_numpad_has_its_own_codes() {
        assert_eq!(virtual_key("Numpad0"), 96);
        assert_eq!(virtual_key("Numpad9"), 105);
        assert_eq!(virtual_key("NumpadMultiply"), 106);
        assert_eq!(virtual_key("NumpadAdd"), 107);
        assert_eq!(virtual_key("NumpadSubtract"), 109);
        assert_eq!(virtual_key("NumpadDecimal"), 110);
        assert_eq!(virtual_key("NumpadDivide"), 111);
        assert_eq!(virtual_key("NumpadEnter"), 13);
        assert_eq!(virtual_key("NumpadEqual"), 187);
        assert_eq!(virtual_key("Enter"), 13);
        // The keypad's Clear key, which Chromium reports as NumLock.
        assert_eq!(virtual_key("NumLock"), 12);
    }

    #[test]
    fn function_keys_count_from_112() {
        assert_eq!(virtual_key("F1"), 112);
        assert_eq!(virtual_key("F12"), 123);
        assert_eq!(virtual_key("F13"), 124);
        assert_eq!(virtual_key("F20"), 131);
        assert_eq!(virtual_key("F24"), 135);
        assert_eq!(virtual_key("F25"), VKEY_UNKNOWN);
    }

    #[test]
    fn modifiers_share_a_code_across_sides_except_the_right_command_key() {
        for (code, expected) in [
            ("ShiftLeft", 16),
            ("ShiftRight", 16),
            ("ControlLeft", 17),
            ("ControlRight", 17),
            ("AltLeft", 18),
            ("AltRight", 18),
            ("MetaLeft", 91),
            ("MetaRight", 93),
            ("CapsLock", 20),
        ] {
            assert_eq!(virtual_key(code), expected, "{code}");
        }
        assert_eq!(modifier_virtual_key("Shift"), 16);
        assert_eq!(modifier_virtual_key("Meta"), 91);
        assert_eq!(modifier_virtual_key("Enter"), VKEY_UNKNOWN);
    }

    #[test]
    fn editing_and_navigation_keys_have_their_codes() {
        for (code, expected) in [
            ("Backspace", 8),
            ("Tab", 9),
            ("Enter", 13),
            ("Escape", 27),
            ("Space", 32),
            ("PageUp", 33),
            ("PageDown", 34),
            ("End", 35),
            ("Home", 36),
            ("ArrowLeft", 37),
            ("ArrowUp", 38),
            ("ArrowRight", 39),
            ("ArrowDown", 40),
            ("Insert", 45),
            ("Delete", 46),
            ("ContextMenu", 93),
        ] {
            assert_eq!(virtual_key(code), expected, "{code}");
        }
    }

    #[test]
    fn a_key_without_a_legacy_code_is_vkey_unknown() {
        for code in ["Fn", "IntlYen", "IntlRo", "NumpadComma", "Lang1", "Lang2", "Unknown", ""] {
            assert_eq!(virtual_key(code), VKEY_UNKNOWN, "{code}");
        }
    }

    #[test]
    fn every_physical_key_of_the_mac_table_has_a_row() {
        for (_, code_value) in MAC_KEY_CODES {
            assert!(
                VIRTUAL_KEYS.iter().any(|(candidate, _)| candidate == code_value),
                "{code_value} has no keyCode row"
            );
        }
    }
}
