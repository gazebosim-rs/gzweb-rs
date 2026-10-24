//! Saving a file the user asked for (the Plot panel's Export CSV): the
//! browser downloads it; natively it is written to the working directory.

/// Saves `contents` as `name` and says where it went: the path written
/// natively, the name downloaded in the browser.
pub fn save_text(name: &str, mime: &str, contents: &str) -> Result<String, String> {
    imp::save_text(name, mime, contents)
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn save_text(name: &str, _mime: &str, contents: &str) -> Result<String, String> {
        let dir = std::env::current_dir().map_err(|e| e.to_string())?;
        let path = dir.join(name);
        std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path.display().to_string())
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

    fn js(e: JsValue) -> String {
        format!("{e:?}")
    }

    /// How long the Blob URL outlives the click (some browsers start the
    /// download after the click returns).
    const REVOKE_MS: i32 = 10_000;

    /// Through a Blob URL and a clicked `<a download>` in the page.
    pub fn save_text(name: &str, mime: &str, contents: &str) -> Result<String, String> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let parts = js_sys::Array::of1(&JsValue::from_str(contents));
        let options = BlobPropertyBag::new();
        options.set_type(mime);
        let blob = Blob::new_with_str_sequence_and_options(&parts, &options).map_err(js)?;
        let url = Url::create_object_url_with_blob(&blob).map_err(js)?;
        let a: HtmlAnchorElement = document
            .create_element("a")
            .map_err(js)?
            .dyn_into()
            .map_err(|_| "not an anchor".to_string())?;
        a.set_href(&url);
        a.set_download(name);
        let body = document.body().ok_or("no body")?;
        body.append_child(&a).map_err(js)?;
        a.click();
        a.remove();
        let revoke = Closure::once_into_js(move || {
            let _ = Url::revoke_object_url(&url);
        });
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                revoke.unchecked_ref(),
                REVOKE_MS,
            )
            .map_err(js)?;
        Ok(name.to_string())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Natively the file lands in the working directory, and the path
    /// says so.
    #[test]
    fn a_file_is_written_to_the_working_directory() {
        let name = format!("gz-bevy-save-test-{}.csv", std::process::id());
        let path = save_text(&name, "text/csv", "a,b\n1,2\n").unwrap();
        assert!(path.ends_with(&name), "{path}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a,b\n1,2\n");
        std::fs::remove_file(&path).unwrap();
        assert!(save_text("no/such/dir/x.csv", "text/csv", "").is_err());
    }
}
