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

/// Make the comic fields (issue, volume, artists, …) if the library hasn't got
/// them yet, for a book marked as a comic by hand. Returns those fields.
#[tauri::command]
pub async fn ensure_comic_fields(state: State<'_, AppState>) -> CmdResult<Vec<CustomField>> {
    write_library(&state, move |lib| lib.ensure_comic_fields()).await
}

/// Whether comics added to the open library list their artists as authors (after the writers).
#[tauri::command]
pub async fn comic_artists_as_authors(state: State<'_, AppState>) -> CmdResult<bool> {
    let lib = state.current().ok_or("No library is open")?;
    let Some(native) = lib.native.clone() else { return Ok(false) };
    let on = native.lock().unwrap_or_else(|e| e.into_inner()).comic_artists_as_authors();
    on.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_comic_artists_as_authors(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    write_library(&state, move |lib| lib.set_comic_artists_as_authors(on)).await
}
