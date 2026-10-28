//! Shortcuts on any keyboard layout. Slint only sees the typed character,
//! so with a Russian (or any non-Latin) layout Ctrl+S arrives as Ctrl+«ы»
//! and neither the app's shortcuts nor copy / paste in text fields react.
//! While Ctrl is held, a non-Latin character is replaced with the Latin
//! letter or digit printed on the same physical key.

use std::cell::Cell;
use std::rc::Rc;

use slint::platform::WindowEvent;
use slint::winit_030::winit::event::{ElementState, WindowEvent as WinitEvent};
use slint::winit_030::winit::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};
use slint::winit_030::{EventResult, WinitWindowAccessor};

/// Install on a window (winit backend; does nothing elsewhere).
pub fn install(window: &slint::Window) {
    let mods = Rc::new(Cell::new(ModifiersState::empty()));
    window.on_winit_window_event(move |w, e| {
        crate::pointer::track(e);
        match e {
            WinitEvent::ModifiersChanged(m) => mods.set(m.state()),
            WinitEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                let m = mods.get();
                // Ctrl+Alt is AltGr on Windows: it types characters
                if !(m.control_key() || (cfg!(target_os = "macos") && m.super_key())) || m.alt_key() {
                    return EventResult::Propagate;
                }
                let Key::Character(typed) = &event.logical_key else { return EventResult::Propagate };
                if typed.is_ascii() {
                    return EventResult::Propagate;
                }
                let PhysicalKey::Code(code) = event.physical_key else { return EventResult::Propagate };
                let Some(c) = latin(code, m.shift_key()) else { return EventResult::Propagate };
                let text = c.to_string().into();
                w.dispatch_event(match (event.state, event.repeat) {
                    (ElementState::Released, _) => WindowEvent::KeyReleased { text },
                    (_, true) => WindowEvent::KeyPressRepeated { text },
                    _ => WindowEvent::KeyPressed { text },
                });
                return EventResult::PreventDefault;
            }
            _ => {}
        }
        EventResult::Propagate
    });
}

/// The character of a key on the US layout (letters and digits only).
#[rustfmt::skip]
fn latin(code: KeyCode, shift: bool) -> Option<char> {
    use KeyCode::*;
    let c = match code {
        KeyA => 'a', KeyB => 'b', KeyC => 'c', KeyD => 'd', KeyE => 'e', KeyF => 'f', KeyG => 'g',
        KeyH => 'h', KeyI => 'i', KeyJ => 'j', KeyK => 'k', KeyL => 'l', KeyM => 'm', KeyN => 'n',
        KeyO => 'o', KeyP => 'p', KeyQ => 'q', KeyR => 'r', KeyS => 's', KeyT => 't', KeyU => 'u',
        KeyV => 'v', KeyW => 'w', KeyX => 'x', KeyY => 'y', KeyZ => 'z',
        Digit0 => '0', Digit1 => '1', Digit2 => '2', Digit3 => '3', Digit4 => '4',
        Digit5 => '5', Digit6 => '6', Digit7 => '7', Digit8 => '8', Digit9 => '9',
        _ => return None,
    };
    Some(if shift { c.to_ascii_uppercase() } else { c })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_physical_keys_to_latin() {
        assert_eq!(latin(KeyCode::KeyS, false), Some('s'));
        assert_eq!(latin(KeyCode::KeyZ, true), Some('Z'));
        assert_eq!(latin(KeyCode::Digit3, false), Some('3'));
        assert_eq!(latin(KeyCode::Semicolon, false), None);
    }
}
