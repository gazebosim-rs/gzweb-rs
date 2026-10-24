//! Whether the system clipboard can be written. Natively it always can;
//! a browser offers `navigator.clipboard` only in a secure context (https
//! or localhost), and Bevy's web clipboard calls it without a check, so a
//! write over plain http would throw in the middle of a frame.

/// Whether [`bevy::clipboard::Clipboard`] can write here.
pub fn clipboard_available() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsValue;
        let Some(window) = web_sys::window() else {
            return false;
        };
        let navigator = js_sys::Reflect::get(&window, &JsValue::from_str("navigator"));
        window.is_secure_context()
            && navigator.is_ok_and(|n| {
                js_sys::Reflect::get(&n, &JsValue::from_str("clipboard"))
                    .is_ok_and(|c| !c.is_undefined())
            })
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        true
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    /// Natively the clipboard is always there.
    #[test]
    fn the_native_clipboard_is_available() {
        assert!(super::clipboard_available());
    }
}
