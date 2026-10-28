//! Where the last click was (window pixels): effects that start "from the
//! button" read it. A key press forgets it, so a change made from the
//! keyboard starts from the middle instead.

use std::cell::Cell;

use slint::winit_030::winit::event::{ElementState, WindowEvent};

thread_local! {
    static CURSOR: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
    static CLICK: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
}

/// Called from the window's winit hook ([`crate::keys::install`]).
pub fn track(e: &WindowEvent) {
    match e {
        WindowEvent::CursorMoved { position, .. } => CURSOR.set(Some((position.x as f32, position.y as f32))),
        WindowEvent::CursorLeft { .. } => CURSOR.set(None),
        WindowEvent::MouseInput { state: ElementState::Pressed, .. } => CLICK.set(CURSOR.get()),
        WindowEvent::Touch(t) => CLICK.set(Some((t.location.x as f32, t.location.y as f32))),
        WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => CLICK.set(None),
        _ => {}
    }
}

/// The last click, in physical pixels of the window.
pub fn last_click() -> Option<(f32, f32)> {
    CLICK.get()
}
