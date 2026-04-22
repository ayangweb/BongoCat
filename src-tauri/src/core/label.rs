use std::sync::{Arc, Mutex};

use tauri::State;

pub struct LabelState {
    pub text: Arc<Mutex<String>>,
}

impl LabelState {
    pub fn new() -> Self {
        Self {
            text: Arc::new(Mutex::new(String::new())),
        }
    }
}

#[tauri::command]
pub async fn set_label_text(
    text: String,
    state: State<'_, Arc<LabelState>>,
) -> Result<(), String> {
    if let Ok(mut t) = state.text.lock() {
        *t = text;
    }
    Ok(())
}
