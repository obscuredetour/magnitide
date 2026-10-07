//! Custom fields (Settings › Library): listing them for any library, and
//! making, renaming and deleting them in Magnitide libraries. Values are
//! edited with the rest of a book (`BookEdit::custom`).

use magnitide_core::{CustomField, NewField};
use tauri::State;

use crate::AppState;
use crate::commands::{CmdResult, write_library};

/// The open library's custom fields, in order (calibre libraries' too, read-only).
#[tauri::command]
pub async fn custom_fields(state: State<'_, AppState>) -> CmdResult<Vec<CustomField>> {
    let lib = state.current().ok_or("No library is open")?;
    Ok(lib.catalog.custom_columns.iter().filter_map(CustomField::from_column).collect())
}

#[tauri::command]
pub async fn add_custom_field(state: State<'_, AppState>, field: NewField) -> CmdResult<CustomField> {
    write_library(&state, move |lib| lib.add_custom_field(&field)).await
}

/// Rename a field, and for a choice field, set its choices.
#[tauri::command]
pub async fn update_custom_field(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    choices: Option<Vec<String>>,
) -> CmdResult<CustomField> {
    write_library(&state, move |lib| lib.update_custom_field(id, &name, choices.as_deref())).await
}

/// Delete a field and every book's value for it.
#[tauri::command]
pub async fn delete_custom_field(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    write_library(&state, move |lib| lib.delete_custom_field(id)).await
}
