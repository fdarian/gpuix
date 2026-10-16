//! macOS virtual key codes (`NSEvent.keyCode`) and the DOM `KeyboardEvent.code`
//! of the physical key, from Chromium's table (`dom_code_data.inc`, mac column).
//!
//! The code names a key's position, so it does not change with the keyboard
//! layout: the key that types `a` on QWERTY and `q` on AZERTY is `KeyA` on both.
//! The table is plain data keyed by the raw `u16`, so it is tested and used by
//! the simulated keyboard on every platform.

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
}
