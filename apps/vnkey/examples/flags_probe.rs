//! Does an injected event inherit the modifiers that are physically held?
//!
//! Run it, hold Shift for the five seconds it samples, and read the verdict.
//!
//!     cargo run -p vnkey --example flags_probe

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("this probe is macOS-only");
}

#[cfg(target_os = "macos")]
fn main() {
    use core_graphics::event::{CGEvent, CGEventFlags};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    println!("Hold Shift now — sampling for 5 seconds…");

    let mut seen = CGEventFlags::empty();
    for _ in 0..50 {
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            eprintln!("could not create an event source");
            return;
        };
        // Exactly how `inject` builds the events it posts.
        let Ok(event) = CGEvent::new_keyboard_event(source, 0, true) else {
            eprintln!("could not create a keyboard event");
            return;
        };
        seen |= event.get_flags();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    println!("flags observed on a freshly created event: {seen:?}");
    if seen.contains(CGEventFlags::CGEventFlagShift) {
        println!(
            "\nINHERITED. An injected character carries Shift whenever Shift is\n\
             held, so every boundary typed with Shift reaches the application\n\
             as a modified keystroke."
        );
    } else {
        println!("\nNOT inherited — the hypothesis is wrong, look elsewhere.");
    }
}
