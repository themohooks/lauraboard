//! Exercise actual text editing and layout, including caret-driven scrolling.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{
    ComponentHandle,
    platform::{Key, WindowEvent},
};
use std::rc::Rc;

slint::slint! {
    import { ScrollingInput } from "../ui/components/scrolling-input.slint";
    export component InputProbe inherits Window {
        width: 160px;
        height: 40px;
        out property <length> text-x: editor.x;
        Rectangle {
            clip: true;
            editor := ScrollingInput {
                viewport-width: parent.width;
                text: "0123456789 abcdefghijklmnopqrstuvwxyz 0123456789 abcdefghijklmnopqrstuvwxyz";
                font-size: 16px;
            }
        }
        public function focus-editor() { editor.focus(); }
    }
}

#[test]
fn caret_reveals_both_ends_of_a_long_single_line() {
    struct Headless(Rc<MinimalSoftwareWindow>);
    impl slint::platform::Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
            Ok(self.0.clone())
        }
    }
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let ui = InputProbe::new().unwrap();
    window.set_size(slint::PhysicalSize::new(160, 40));
    ui.show().unwrap();
    let draw = || {
        window.request_redraw();
        window.draw_if_needed(|r| {
            r.render(&mut vec![slint::Rgb8Pixel::default(); 160 * 40], 160);
        });
    };
    draw();
    ui.invoke_focus_editor();
    // Slint maps Control to Command on Apple platforms. Home/End there
    // scroll the document rather than moving the text caret.
    let navigate = |to_end: bool| {
        let key = if cfg!(target_vendor = "apple") {
            if to_end { Key::RightArrow } else { Key::LeftArrow }
        } else if to_end {
            Key::End
        } else {
            Key::Home
        };
        if cfg!(target_vendor = "apple") {
            window.dispatch_event(WindowEvent::KeyPressed { text: Key::Control.into() });
        }
        window.dispatch_event(WindowEvent::KeyPressed { text: key.into() });
        window.dispatch_event(WindowEvent::KeyReleased { text: key.into() });
        if cfg!(target_vendor = "apple") {
            window.dispatch_event(WindowEvent::KeyReleased { text: Key::Control.into() });
        }
    };
    navigate(true);
    draw();
    assert!(ui.get_text_x() < -100.0, "Moving the caret to the end must reveal the hidden end of the line");
    navigate(false);
    draw();
    assert!(ui.get_text_x().abs() < 0.01, "Moving the caret to the start must reveal the start again");
}
