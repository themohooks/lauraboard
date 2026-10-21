//! Windows: application icon and manifest (see `lb-winres`).

fn main() {
    lb_winres::embed(std::path::Path::new("../lb-gui/assets/app/laurabrod.ico"));
}
