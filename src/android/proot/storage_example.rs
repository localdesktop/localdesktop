use crate::android::utils::storage_permissions::StoragePermissionManager;
use winit::platform::android::activity::AndroidApp;

/// Example usage of the fixed storage permissions implementation
/// This demonstrates how to properly request and check storage permissions
/// across different Android versions without AndroidX dependencies
pub fn example_storage_permissions_usage(android_app: AndroidApp) {
    let storage_manager = StoragePermissionManager::new(android_app);
    
    // Get detailed permission status for debugging
    let status = storage_manager.get_permission_status();
    println!("Current permission status:\n{}", status);
    
    // Check if we already have storage permissions
    if storage_manager.has_storage_permission() {
        println!("Storage permissions already granted!");
        
        // Set up storage symlinks (similar to Termux's termux-setup-storage)
        if let Err(e) = storage_manager.setup_storage_symlinks() {
            println!("Failed to setup storage symlinks: {}", e);
        } else {
            let storage_dir = storage_manager.get_storage_directory();
            println!("Storage symlinks created at: {}", storage_dir.display());
        }
    } else {
        println!("Storage permissions not granted, requesting...");
        
        // Request storage permissions (will handle different Android versions automatically)
        storage_manager.request_storage_permissions();
        
        // Note: After requesting permissions, the app should wait for the user response
        // The actual permission grant/deny result will be handled by the callback function
        // handle_permission_result() which is called by the Android system
        
        println!("Permission request sent to user. Check again after user responds.");
    }
    
    // Check if MANAGE_EXTERNAL_STORAGE can be requested (Android 11+)
    if storage_manager.can_request_manage_external_storage() {
        println!("This app can request MANAGE_EXTERNAL_STORAGE permission for full file access");
    }
}

/// This function would be called after the user responds to the permission request
pub fn example_post_permission_check(android_app: AndroidApp) {
    let storage_manager = StoragePermissionManager::new(android_app);
    
    if storage_manager.has_storage_permission() {
        println!("Permissions granted! Setting up storage access...");
        
        match storage_manager.setup_storage_symlinks() {
            Ok(_) => {
                println!("Storage access configured successfully");
                let storage_dir = storage_manager.get_storage_directory();
                println!("Access your Android files through: {}", storage_dir.display());
            }
            Err(e) => {
                println!("Failed to setup storage: {}", e);
            }
        }
    } else {
        println!("Storage permissions still not granted");
        let status = storage_manager.get_permission_status();
        println!("Current status:\n{}", status);
    }
}