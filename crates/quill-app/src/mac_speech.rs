//! Read Aloud through the Mac's installed speech voices. AVFAudio supplies
//! the voices, so Quill carries no speech model or voice data of its own.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;
use std::cell::RefCell;

// AVSpeechSynthesizer and AVSpeechUtterance live in this system framework.
#[link(name = "AVFAudio", kind = "framework")]
unsafe extern "C" {}

thread_local! {
    /// Speech is controlled on Quill's application thread. Keeping one
    /// synthesizer lets Stop Reading reach the utterance now speaking.
    static SYNTHESIZER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

fn synthesizer() -> Option<Retained<AnyObject>> {
    // SAFETY: called on the Mac application thread. `new` returns an owned
    // AVSpeechSynthesizer, which Retained takes over.
    unsafe {
        let raw: *mut AnyObject = msg_send![class!(AVSpeechSynthesizer), new];
        Retained::from_raw(raw)
    }
}

/// Stop any current utterance and read `text` with the Mac's default voice.
pub fn speak(text: &str) {
    if text.trim().is_empty() {
        return;
    }
    SYNTHESIZER.with_borrow_mut(|saved| {
        let synth = saved.get_or_insert_with(|| synthesizer().expect("AVSpeechSynthesizer"));
        // SAFETY: these are AVSpeechSynthesizer and AVSpeechUtterance's
        // documented methods. Boundary 0 is AVSpeechBoundaryImmediate.
        unsafe {
            let _: bool = msg_send![&**synth, stopSpeakingAtBoundary: 0isize];
            let string = NSString::from_str(text);
            let utterance: *mut AnyObject = msg_send![
                class!(AVSpeechUtterance),
                speechUtteranceWithString: &*string
            ];
            if !utterance.is_null() {
                let _: () = msg_send![&**synth, speakUtterance: utterance];
            }
        }
    });
}

/// Stop Read Aloud immediately. It is harmless when nothing is speaking.
pub fn stop() {
    SYNTHESIZER.with_borrow(|saved| {
        if let Some(synth) = saved {
            // SAFETY: `synth` remains retained for this call; boundary 0 is
            // AVSpeechBoundaryImmediate.
            unsafe {
                let _: bool = msg_send![&**synth, stopSpeakingAtBoundary: 0isize];
            }
        }
    });
}
