mod elevate;
mod library;
mod project;
mod robot;
mod sftp;

use tauri_plugin_fs::FsExt;

/// A file picker grants the selected PML; its declared children need the same project root.
#[tauri::command]
fn authorize_project_root(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let file = std::path::PathBuf::from(path);
    if !app.fs_scope().is_allowed(&file) || !file.is_file() {
        return Err("Select the project through the file picker first.".into());
    }
    if !file.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pml")) {
        return Ok(());
    }
    let parent = file.parent().ok_or("Project has no parent folder")?;
    app.fs_scope().allow_directory(parent, true).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_persisted_scope::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(project::Sessions::default())
        .manage(robot::RobotState::default())
        .manage(sftp::SftpState::default())
        .invoke_handler(tauri::generate_handler![
            authorize_project_root,
            project::project_open,
            project::project_close,
            project::project_edit,
            project::project_undo,
            project::project_redo,
            project::project_save,
            project::project_copy,
            project::project_new,
            project::project_insert_library_box,
            project::project_add_behavior,
            project::project_import_files,
            project::project_import_behavior,
            project::project_remove_behavior,
            project::project_remove_folder,
            project::project_removal_files,
            project::project_move_path,
            project::project_duplicate_behavior,
            project::project_reorder,
            project::project_set_choren,
            project::project_manifest,
            project::project_set_manifest,
            project::project_rename,
            project::library_add_box,
            project::project_update_translations,
            project::project_add_dialog,
            project::project_read_text,
            project::project_read_binary,
            project::project_write_text,
            project::project_resource,
            project::project_export_crg,
            project::project_build_package,
            project::project_save_as_folder,
            project::project_export_qianim,
            project::project_export_motion_python,
            project::project_import_qianim,
            library::library_index,
            library::library_tree,
            library::library_share_dir,
            library::library_box,
            library::library_create,
            library::library_import_cbl,
            library::library_export_cbl,
            library::library_remove,
            library::library_manage,
            library::library_copy,
            library::library_edit,
            library::poses_read,
            library::poses_write,
            robot::robot_models,
            robot::robot_descriptor,
            robot::robot_start_virtual,
            robot::robot_discover,
            robot::robot_connect,
            robot::robot_disconnect,
            robot::robot_call,
            robot::robot_services,
            robot::robot_camera_frame,
            robot::robot_play,
            robot::robot_activate_input,
            robot::robot_set_parameters,
            robot::robot_stop,
            robot::robot_status,
            robot::robot_mesh,
            sftp::sftp_connect,
            sftp::sftp_disconnect,
            sftp::sftp_list,
            sftp::sftp_mkdir,
            sftp::sftp_remove,
            sftp::sftp_cancel,
            sftp::sftp_upload,
            sftp::sftp_download
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
