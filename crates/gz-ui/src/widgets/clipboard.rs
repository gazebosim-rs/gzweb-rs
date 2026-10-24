//! Copying text: a [`CopyText`] puts it on the clipboard (Bevy's: the
//! system's natively, the browser's on the web). The UI copies through the
//! message, never the clipboard itself, so a test can see what was copied
//! without touching the machine's clipboard.

use bevy::clipboard::Clipboard;
use bevy::prelude::*;

/// Put this text on the clipboard.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct CopyText(pub String);

/// Puts each [`CopyText`] on the clipboard, where there is one.
pub(crate) fn copy_text(mut copies: MessageReader<CopyText>, clipboard: Option<ResMut<Clipboard>>) {
    let Some(mut clipboard) = clipboard else {
        copies.clear();
        return;
    };
    for c in copies.read() {
        if let Err(e) = clipboard.set_text(c.0.as_str()) {
            warn!("copy: {e:?}");
        }
    }
}
