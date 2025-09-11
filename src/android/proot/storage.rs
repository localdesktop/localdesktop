use crate::android::{
    proot::process::ArchProcess,
    utils::{
        application_context::get_application_context, storage_permissions::StoragePermissionManager,
    },
};
use crate::core::config::ARCH_FS_ROOT;
use std::{fs, path::Path};
use winit::platform::android::activity::AndroidApp;

#[derive(Debug)]
pub enum StorageSetupError {
    PermissionDenied,
    FileSystemError(String),
    ProotError(String),
}

impl std::fmt::Display for StorageSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageSetupError::PermissionDenied => write!(f, "Storage permission not granted"),
            StorageSetupError::FileSystemError(msg) => write!(f, "Filesystem error: {}", msg),
            StorageSetupError::ProotError(msg) => write!(f, "Proot error: {}", msg),
        }
    }
}

impl std::error::Error for StorageSetupError {}

pub struct StorageSetup {
    permission_manager: StoragePermissionManager,
}

impl StorageSetup {
    pub fn new(android_app: AndroidApp) -> Self {
        Self {
            permission_manager: StoragePermissionManager::new(android_app),
        }
    }

    /// Main storage setup function - equivalent to termux-setup-storage
    pub fn setup_storage_access(&self) -> Result<(), StorageSetupError> {
        log::info!("Setting up storage access for Local Desktop");

        // Step 1: Check and request permissions if needed
        if !self.permission_manager.has_storage_permission() {
            log::info!("Storage permission not granted, requesting permission");
            self.permission_manager.request_storage_permissions();
            return Err(StorageSetupError::PermissionDenied);
        }

        // Step 2: Create storage symlinks in Termux data directory
        self.permission_manager
            .setup_storage_symlinks()
            .map_err(|e| StorageSetupError::FileSystemError(e.to_string()))?;

        // Step 3: Create storage directory in Arch filesystem
        self.create_arch_storage_directory()
            .map_err(|e| StorageSetupError::FileSystemError(e.to_string()))?;

        // Step 4: Create bind mount points for proot
        self.create_bind_mount_directories()
            .map_err(|e| StorageSetupError::FileSystemError(e.to_string()))?;

        log::info!("Storage access setup completed successfully");
        Ok(())
    }

    /// Create storage directory in Arch Linux filesystem
    fn create_arch_storage_directory(&self) -> Result<(), Box<dyn std::error::Error>> {
        let fs_root = Path::new(ARCH_FS_ROOT);
        let storage_dir = fs_root.join("home").join("user").join("storage");

        // Create the storage directory
        fs::create_dir_all(&storage_dir)?;
        log::info!("Created Arch storage directory: {}", storage_dir.display());

        // Create subdirectories for common storage locations
        let subdirs = vec![
            "shared",
            "downloads",
            "dcim",
            "pictures",
            "music",
            "movies",
            "documents",
            "external",
        ];

        for subdir in subdirs {
            let subdir_path = storage_dir.join(subdir);
            fs::create_dir_all(&subdir_path)?;
            log::debug!("Created storage subdirectory: {}", subdir_path.display());
        }

        Ok(())
    }

    /// Create directories that will be used as bind mount points
    fn create_bind_mount_directories(&self) -> Result<(), Box<dyn std::error::Error>> {
        let context = get_application_context();
        let termux_storage = context.data_dir.join("storage");

        // Ensure termux storage directory exists
        if !termux_storage.exists() {
            return Err(format!(
                "Termux storage directory not found: {}. Run setup_storage_symlinks first.",
                termux_storage.display()
            )
            .into());
        }

        // Create mount directories in Arch FS that will be bind mounted
        let fs_root = Path::new(ARCH_FS_ROOT);
        let mount_base = fs_root.join("mnt").join("android");

        fs::create_dir_all(&mount_base)?;
        log::info!("Created Android mount base: {}", mount_base.display());

        // Create mount points for each storage location
        let mount_points = vec![
            ("shared", "/storage/emulated/0"),
            ("downloads", "/storage/emulated/0/Download"),
            ("dcim", "/storage/emulated/0/DCIM"),
            ("pictures", "/storage/emulated/0/Pictures"),
            ("music", "/storage/emulated/0/Music"),
            ("movies", "/storage/emulated/0/Movies"),
            ("documents", "/storage/emulated/0/Documents"),
            ("external", "/storage/extSdCard"), // This may not exist on all devices
        ];

        for (name, _android_path) in mount_points {
            let mount_point = mount_base.join(name);
            fs::create_dir_all(&mount_point)?;
            log::debug!("Created mount point: {}", mount_point.display());
        }

        Ok(())
    }

    /// Get the bind mount arguments for proot
    pub fn get_storage_bind_mounts(&self) -> Vec<String> {
        let mut bind_mounts = Vec::new();

        if !self.permission_manager.has_storage_permission() {
            log::warn!("Storage permission not granted, skipping storage bind mounts");
            log::debug!("Permission status:\n{}", self.permission_manager.get_permission_status());
            return bind_mounts;
        }

        log::info!("Storage permission granted, setting up bind mounts");

        let context = get_application_context();
        let termux_storage = context.data_dir.join("storage");
        let fs_root = Path::new(ARCH_FS_ROOT);
        let mount_base = fs_root.join("mnt").join("android");

        // Define storage bind mounts
        let storage_mounts = vec![
            ("shared", "/storage/emulated/0"),
            ("downloads", "/storage/emulated/0/Download"),
            ("dcim", "/storage/emulated/0/DCIM"),
            ("pictures", "/storage/emulated/0/Pictures"),
            ("music", "/storage/emulated/0/Music"),
            ("movies", "/storage/emulated/0/Movies"),
            ("documents", "/storage/emulated/0/Documents"),
        ];

        for (name, android_path) in storage_mounts {
            let termux_symlink = termux_storage.join(name);
            let mount_point = mount_base.join(name);

            // Only add bind mount if the symlink exists and the Android path exists
            if (termux_symlink.exists() || termux_symlink.is_symlink()) && Path::new(android_path).exists() {
                // Bind mount from Android storage path to the mount point inside chroot
                bind_mounts.push(format!("--bind={}:{}", android_path, mount_point.display()));
                log::debug!(
                    "Added storage bind mount: {} -> {}",
                    android_path,
                    mount_point.display()
                );
            } else {
                log::debug!("Skipping mount for {} - symlink exists: {}, android path exists: {}", 
                    name, 
                    termux_symlink.exists() || termux_symlink.is_symlink(),
                    Path::new(android_path).exists()
                );
            }
        }

        // Try to detect and bind external storage
        let external_paths = vec![
            "/storage/extSdCard",
            "/storage/sdcard1",
            "/storage/external_sd",
            "/mnt/external_sd",
            "/mnt/extSdCard",
        ];

        for (i, ext_path) in external_paths.iter().enumerate() {
            if Path::new(ext_path).exists() {
                let mount_name = if i == 0 {
                    "external".to_string()
                } else {
                    format!("external{}", i)
                };
                let mount_point = mount_base.join(&mount_name);

                bind_mounts.push(format!("--bind={}:{}", ext_path, mount_point.display()));
                log::info!(
                    "Added external storage bind mount: {} -> {}",
                    ext_path,
                    mount_point.display()
                );
            }
        }

        bind_mounts
    }

    /// Test storage access by attempting to list files
    pub fn test_storage_access(&self) -> Result<(), StorageSetupError> {
        if !self.permission_manager.has_storage_permission() {
            return Err(StorageSetupError::PermissionDenied);
        }

        log::info!("Testing storage access from within proot environment");

        let test_command = "ls -la /mnt/android/shared/ | head -10";
        let process = ArchProcess::exec(test_command);

        match process.wait_with_output() {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    log::info!("Storage access test successful:\n{}", stdout);
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::error!("Storage access test failed: {}", stderr);
                    Err(StorageSetupError::ProotError(stderr.to_string()))
                }
            }
            Err(e) => {
                log::error!("Failed to run storage test command: {}", e);
                Err(StorageSetupError::ProotError(e.to_string()))
            }
        }
    }

    /// Check if storage is already set up
    pub fn is_storage_setup(&self) -> bool {
        let fs_root = Path::new(ARCH_FS_ROOT);
        let mount_base = fs_root.join("mnt").join("android");
        let storage_dir = fs_root.join("home").join("user").join("storage");

        mount_base.exists()
            && storage_dir.exists()
            && self.permission_manager.has_storage_permission()
    }
    
    /// Debug function to get detailed storage setup status
    pub fn debug_storage_status(&self) -> String {
        let mut debug_info = String::new();
        debug_info.push_str("=== Storage Setup Debug Info ===\n");
        
        // Check permissions first
        debug_info.push_str(&format!("Has storage permission: {}\n", self.permission_manager.has_storage_permission()));
        
        if !self.permission_manager.has_storage_permission() {
            debug_info.push_str("Permission details:\n");
            debug_info.push_str(&self.permission_manager.get_permission_status());
            return debug_info;
        }
        
        // Check filesystem structure
        let context = get_application_context();
        let termux_storage = context.data_dir.join("storage");
        let fs_root = Path::new(ARCH_FS_ROOT);
        let mount_base = fs_root.join("mnt").join("android");
        let storage_dir = fs_root.join("home").join("user").join("storage");
        
        debug_info.push_str(&format!("Termux storage dir: {} (exists: {})\n", 
            termux_storage.display(), termux_storage.exists()));
        debug_info.push_str(&format!("Mount base dir: {} (exists: {})\n", 
            mount_base.display(), mount_base.exists()));
        debug_info.push_str(&format!("Arch storage dir: {} (exists: {})\n", 
            storage_dir.display(), storage_dir.exists()));
            
        // Check individual storage paths
        let storage_paths = vec![
            ("shared", "/storage/emulated/0"),
            ("downloads", "/storage/emulated/0/Download"),
            ("dcim", "/storage/emulated/0/DCIM"),
        ];
        
        debug_info.push_str("\nStorage path status:\n");
        for (name, android_path) in storage_paths {
            let symlink_path = termux_storage.join(name);
            let mount_point = mount_base.join(name);
            let android_exists = std::path::Path::new(android_path).exists();
            
            debug_info.push_str(&format!("  {}: symlink={}, android_path={}, mount_point={}\n", 
                name, 
                symlink_path.exists() || symlink_path.is_symlink(),
                android_exists,
                mount_point.exists()
            ));
        }
        
        // Check bind mounts
        let bind_mounts = self.get_storage_bind_mounts();
        debug_info.push_str(&format!("\nBind mounts ({}):\n", bind_mounts.len()));
        for bind_mount in &bind_mounts {
            debug_info.push_str(&format!("  - {}\n", bind_mount));
        }
        
        debug_info
    }
}
