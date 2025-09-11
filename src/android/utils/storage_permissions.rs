use crate::android::utils::application_context::get_application_context;
use crate::core::logging::PolarBearExpectation;
use jni::{
    objects::{JObject, JString, JValue},
    sys::jint,
    JNIEnv, JavaVM,
};
use std::sync::{Arc, Mutex, OnceLock};
use winit::platform::android::activity::AndroidApp;

static PERMISSION_GRANTED: OnceLock<Arc<Mutex<bool>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct StoragePermissionManager {
    android_app: AndroidApp,
}

impl StoragePermissionManager {
    pub fn new(android_app: AndroidApp) -> Self {
        // Initialize the permission state
        PERMISSION_GRANTED.get_or_init(|| Arc::new(Mutex::new(false)));
        
        Self { android_app }
    }

    /// Check if storage permissions are already granted
    pub fn has_storage_permission(&self) -> bool {
        let vm = unsafe {
            JavaVM::from_raw(self.android_app.vm_as_ptr() as *mut _)
                .pb_expect("Failed to get JavaVM")
        };
        let mut env = vm
            .attach_current_thread()
            .pb_expect("Failed to attach current thread");

        let activity = unsafe { JObject::from_raw(self.android_app.activity_as_ptr() as *mut _) };

        // Get the SDK version to determine which permissions to check
        let sdk_int = self.get_sdk_version(&mut env);
        
        if sdk_int >= 33 {
            // Android 13+: Use granular media permissions OR all files access
            let manage_storage = self.check_manage_external_storage(&mut env, &activity);
            if manage_storage {
                // If we have MANAGE_EXTERNAL_STORAGE, we have broad file access
                return true;
            }
            
            // Otherwise check granular media permissions for basic functionality
            let media_images = self.check_single_permission(
                &mut env,
                &activity,
                "android.permission.READ_MEDIA_IMAGES",
            );
            let media_video = self.check_single_permission(
                &mut env,
                &activity,
                "android.permission.READ_MEDIA_VIDEO",
            );
            let media_audio = self.check_single_permission(
                &mut env,
                &activity,
                "android.permission.READ_MEDIA_AUDIO",
            );
            
            // For Linux desktop environment, we need at least one media permission
            media_images || media_video || media_audio
        } else if sdk_int >= 30 {
            // Android 11-12: MANAGE_EXTERNAL_STORAGE for broad access, or READ_EXTERNAL_STORAGE for limited
            let manage_storage = self.check_manage_external_storage(&mut env, &activity);
            if manage_storage {
                return true;
            }
            
            // Fall back to READ_EXTERNAL_STORAGE for scoped access
            self.check_single_permission(&mut env, &activity, "android.permission.READ_EXTERNAL_STORAGE")
        } else if sdk_int >= 23 {
            // Android 6-10: Traditional external storage permissions
            let read_permission = self.check_single_permission(
                &mut env,
                &activity,
                "android.permission.READ_EXTERNAL_STORAGE",
            );
            let write_permission = self.check_single_permission(
                &mut env,
                &activity,
                "android.permission.WRITE_EXTERNAL_STORAGE",
            );
            
            read_permission && write_permission
        } else {
            // Android < 6: Permissions granted at install time
            true
        }
    }

    fn check_single_permission(&self, env: &mut JNIEnv, activity: &JObject, permission: &str) -> bool {
        let permission_jstring = env
            .new_string(permission)
            .pb_expect("Failed to create permission string");

        // Use Activity.checkSelfPermission (available since API 23) or 
        // Context.checkCallingOrSelfPermission (available on all API levels)
        let sdk_int = self.get_sdk_version(env);
        
        if sdk_int >= 23 {
            // Use Activity.checkSelfPermission (API 23+)
            let result = env
                .call_method(
                    activity,
                    "checkSelfPermission",
                    "(Ljava/lang/String;)I",
                    &[JValue::Object(&permission_jstring)],
                )
                .pb_expect("Failed to call Activity.checkSelfPermission")
                .i()
                .pb_expect("Failed to get permission result");

            // PackageManager.PERMISSION_GRANTED = 0
            result == 0
        } else {
            // For API < 23, use Context.checkCallingOrSelfPermission
            let result = env
                .call_method(
                    activity,
                    "checkCallingOrSelfPermission",
                    "(Ljava/lang/String;)I",
                    &[JValue::Object(&permission_jstring)],
                )
                .pb_expect("Failed to call Context.checkCallingOrSelfPermission")
                .i()
                .pb_expect("Failed to get permission result");

            // PackageManager.PERMISSION_GRANTED = 0
            result == 0
        }
    }

    /// Check MANAGE_EXTERNAL_STORAGE permission using Environment.isExternalStorageManager()
    /// This is the proper way to check this permission since it's not a runtime permission
    fn check_manage_external_storage(&self, env: &mut JNIEnv, _activity: &JObject) -> bool {
        let sdk_int = self.get_sdk_version(env);
        
        if sdk_int >= 30 {
            // Use Environment.isExternalStorageManager() for API 30+
            match env.find_class("android/os/Environment") {
                Ok(environment_class) => {
                    match env.call_static_method(
                        environment_class,
                        "isExternalStorageManager",
                        "()Z",
                        &[],
                    ) {
                        Ok(result) => result.z().unwrap_or(false),
                        Err(e) => {
                            log::warn!("Failed to call Environment.isExternalStorageManager: {}", e);
                            false
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to find Environment class: {}", e);
                    false
                }
            }
        } else {
            // MANAGE_EXTERNAL_STORAGE doesn't exist before API 30
            false
        }
    }

    /// Request MANAGE_EXTERNAL_STORAGE permission by opening Settings
    /// This permission cannot be requested via runtime permissions
    fn request_manage_external_storage(&self, env: &mut JNIEnv, activity: &JObject) {
        log::info!("Requesting MANAGE_EXTERNAL_STORAGE via Settings intent");
        
        // Create Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION intent
        match env.find_class("android/content/Intent") {
            Ok(intent_class) => {
                // Create Intent with Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION
                let action_string = env
                    .new_string("android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION")
                    .pb_expect("Failed to create action string");
                
                let intent = env
                    .new_object(
                        intent_class,
                        "(Ljava/lang/String;)V",
                        &[JValue::Object(&action_string)],
                    )
                    .pb_expect("Failed to create Intent");

                // Get package name from activity context
                let package_name = env
                    .call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])
                    .pb_expect("Failed to get package name")
                    .l()
                    .pb_expect("Failed to get package name string");

                // Create URI for package-specific settings
                match env.find_class("android/net/Uri") {
                    Ok(uri_class) => {
                        // Convert package name to Rust string first to avoid borrow issues
                        let package_name_str = env.get_string(&JString::from(package_name))
                            .pb_expect("Failed to convert package name")
                            .to_string_lossy()
                            .to_string();
                        
                        let uri_string = env
                            .new_string(&format!("package:{}", package_name_str))
                            .pb_expect("Failed to create URI string");

                        let uri = env
                            .call_static_method(
                                uri_class,
                                "parse",
                                "(Ljava/lang/String;)Landroid/net/Uri;",
                                &[JValue::Object(&uri_string)],
                            )
                            .pb_expect("Failed to parse URI")
                            .l()
                            .pb_expect("Failed to get URI object");

                        // Set data on intent
                        env.call_method(
                            &intent,
                            "setData",
                            "(Landroid/net/Uri;)Landroid/content/Intent;",
                            &[JValue::Object(&uri)],
                        )
                        .pb_expect("Failed to set data on Intent");
                    }
                    Err(e) => {
                        log::warn!("Failed to find Uri class, proceeding without package-specific intent: {}", e);
                    }
                }

                // Start the activity
                env.call_method(
                    activity,
                    "startActivity",
                    "(Landroid/content/Intent;)V",
                    &[JValue::Object(&intent)],
                )
                .pb_expect("Failed to start Settings activity");

                log::info!("Opened MANAGE_EXTERNAL_STORAGE settings for user");
            }
            Err(e) => {
                log::error!("Failed to create Settings intent for MANAGE_EXTERNAL_STORAGE: {}", e);
            }
        }
    }
    
    fn get_sdk_version(&self, env: &mut JNIEnv) -> i32 {
        let build_class = env
            .find_class("android/os/Build$VERSION")
            .pb_expect("Failed to find Build.VERSION class");
            
        let sdk_int = env
            .get_static_field(build_class, "SDK_INT", "I")
            .pb_expect("Failed to get SDK_INT")
            .i()
            .pb_expect("Failed to convert SDK_INT to int");
            
        sdk_int
    }

    /// Request storage permissions from the user
    pub fn request_storage_permissions(&self) -> bool {
        if self.has_storage_permission() {
            log::info!("Storage permissions already granted");
            return true;
        }

        log::info!("Requesting storage permissions from user");

        let vm = unsafe {
            JavaVM::from_raw(self.android_app.vm_as_ptr() as *mut _)
                .pb_expect("Failed to get JavaVM")
        };
        let mut env = vm
            .attach_current_thread()
            .pb_expect("Failed to attach current thread");

        let activity = unsafe { JObject::from_raw(self.android_app.activity_as_ptr() as *mut _) };
        
        let sdk_int = self.get_sdk_version(&mut env);
        
        // Build permissions array based on Android version
        // Note: MANAGE_EXTERNAL_STORAGE requires special intent-based request, not runtime permission
        let permissions = if sdk_int >= 33 {
            // Android 13+: Request granular media permissions
            vec![
                "android.permission.READ_MEDIA_IMAGES",
                "android.permission.READ_MEDIA_VIDEO",
                "android.permission.READ_MEDIA_AUDIO",
            ]
        } else if sdk_int >= 23 {
            // Android 6-12: Request traditional storage permissions
            vec![
                "android.permission.READ_EXTERNAL_STORAGE",
                "android.permission.WRITE_EXTERNAL_STORAGE",
            ]
        } else {
            // Android < 6: No runtime permissions needed
            vec![]
        };

        // Handle runtime permission request
        if !permissions.is_empty() && sdk_int >= 23 {
            let permissions_array = env
                .new_object_array(
                    permissions.len() as jint,
                    "java/lang/String",
                    JObject::null(),
                )
                .pb_expect("Failed to create permissions array");

            for (i, permission) in permissions.iter().enumerate() {
                let permission_string = env
                    .new_string(permission)
                    .pb_expect("Failed to create permission string");
                env.set_object_array_element(&permissions_array, i as jint, permission_string)
                    .pb_expect("Failed to set permission in array");
            }

            let request_code = 1001;
            env.call_method(
                &activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[
                    JValue::Object(&permissions_array),
                    JValue::Int(request_code),
                ],
            )
            .pb_expect("Failed to request permissions via Activity");

            log::info!("Requested {} runtime permissions", permissions.len());
        } else if sdk_int < 23 {
            log::info!("API level {} < 23, permissions should be granted at install time", sdk_int);
        }

        // For Android 11+, also try to request MANAGE_EXTERNAL_STORAGE via Settings intent
        if sdk_int >= 30 && !self.check_manage_external_storage(&mut env, &activity) {
            self.request_manage_external_storage(&mut env, &activity);
        }

        log::info!("Permission request sent to Android system");
        
        // Note: The actual permission result will be handled by the Android system
        // and should be checked again later using has_storage_permission()
        false
    }

    /// Set up storage symlinks similar to Termux's termux-setup-storage
    pub fn setup_storage_symlinks(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.has_storage_permission() {
            return Err("Storage permission not granted".into());
        }

        let context = get_application_context();
        let storage_dir = context.data_dir.join("storage");
        
        // Create storage directory if it doesn't exist
        std::fs::create_dir_all(&storage_dir)?;

        // Get Android API version to determine appropriate storage paths  
        let vm = unsafe {
            JavaVM::from_raw(self.android_app.vm_as_ptr() as *mut _)
                .map_err(|e| format!("Failed to get JavaVM: {}", e))?
        };
        let mut env = vm
            .attach_current_thread()
            .map_err(|e| format!("Failed to attach current thread: {}", e))?;
        let sdk_int = self.get_sdk_version(&mut env);
        log::info!("Setting up storage symlinks for Android API {}", sdk_int);

        // Android version-specific storage paths
        let storage_paths = if sdk_int >= 35 {
            // Android 15+: Use app-accessible paths and Environment.getExternalStorageDirectory()
            self.get_android_15_storage_paths_with_env(&mut env)?
        } else if sdk_int >= 30 {
            // Android 11-14: Scoped storage with some legacy paths
            vec![
                ("shared", "/storage/emulated/0".to_string()),
                ("downloads", "/storage/emulated/0/Download".to_string()), 
                ("dcim", "/storage/emulated/0/DCIM".to_string()),
                ("pictures", "/storage/emulated/0/Pictures".to_string()),
                ("music", "/storage/emulated/0/Music".to_string()),
                ("movies", "/storage/emulated/0/Movies".to_string()),
                ("documents", "/storage/emulated/0/Documents".to_string()),
            ]
        } else {
            // Android <11: Legacy storage access
            vec![
                ("shared", "/storage/emulated/0".to_string()),
                ("downloads", "/storage/emulated/0/Download".to_string()), 
                ("dcim", "/storage/emulated/0/DCIM".to_string()),
                ("pictures", "/storage/emulated/0/Pictures".to_string()),
                ("music", "/storage/emulated/0/Music".to_string()),
                ("movies", "/storage/emulated/0/Movies".to_string()),
                ("documents", "/storage/emulated/0/Documents".to_string()),
            ]
        };

        // Create symlinks with enhanced error handling for Android 15
        let mut successful_symlinks = 0;
        let total_paths = storage_paths.len();
        
        for (name, android_path) in storage_paths {
            let symlink_path = storage_dir.join(name);
            
            // Check if the Android path actually exists before creating symlink
            if !std::path::Path::new(&android_path).exists() {
                log::warn!("Android path does not exist, skipping symlink for {}: {}", name, android_path);
                continue;
            }
            
            // Remove existing symlink if it exists
            if symlink_path.exists() || symlink_path.is_symlink() {
                if let Err(e) = std::fs::remove_file(&symlink_path) {
                    log::warn!("Failed to remove existing symlink for {}: {}", name, e);
                }
            }

            // Create new symlink with detailed error reporting
            match std::os::unix::fs::symlink(&android_path, &symlink_path) {
                Ok(()) => {
                    log::info!("Created storage symlink: {} -> {}", symlink_path.display(), android_path);
                    successful_symlinks += 1;
                },
                Err(e) => {
                    log::warn!("Failed to create symlink for {} ({}->{}): {}", 
                        name, android_path, symlink_path.display(), e);
                    
                    // For Android 15, provide additional context
                    if sdk_int >= 35 {
                        log::info!("Android 15+ detected: This may be due to scoped storage restrictions. Consider using SAF or MediaStore APIs for {} access.", name);
                    }
                }
            }
        }
        
        log::info!("Storage symlink setup completed: {}/{} symlinks created successfully", 
            successful_symlinks, total_paths);
            
        if successful_symlinks == 0 {
            return Err(format!("Failed to create any storage symlinks on Android API {}", sdk_int).into());
        }

        // Try to detect external SD card
        self.detect_external_storage(&storage_dir)?;

        Ok(())
    }

    /// Detect and symlink external storage devices
    fn detect_external_storage(&self, storage_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let external_storage_paths = vec![
            "/storage/extSdCard",
            "/storage/sdcard1", 
            "/storage/external_sd",
            "/mnt/external_sd",
            "/mnt/extSdCard",
        ];

        for (i, path) in external_storage_paths.iter().enumerate() {
            if std::path::Path::new(path).exists() {
                let symlink_name = if i == 0 { "external".to_string() } else { format!("external{}", i) };
                let symlink_path = storage_dir.join(&symlink_name);
                
                if let Err(e) = std::os::unix::fs::symlink(path, &symlink_path) {
                    log::warn!("Failed to create external storage symlink {}: {}", symlink_name, e);
                } else {
                    log::info!("Created external storage symlink: {} -> {}", symlink_path.display(), path);
                }
            }
        }

        Ok(())
    }

    /// Get the path to the storage directory with symlinks
    pub fn get_storage_directory(&self) -> std::path::PathBuf {
        let context = get_application_context();
        context.data_dir.join("storage")
    }

    /// Check if the app can request MANAGE_EXTERNAL_STORAGE permission
    /// This permission can only be granted to apps that meet certain criteria
    pub fn can_request_manage_external_storage(&self) -> bool {
        let vm = unsafe {
            JavaVM::from_raw(self.android_app.vm_as_ptr() as *mut _)
                .pb_expect("Failed to get JavaVM")
        };
        let mut env = vm
            .attach_current_thread()
            .pb_expect("Failed to attach current thread");

        let sdk_int = self.get_sdk_version(&mut env);
        
        if sdk_int >= 30 {
            // On Android 11+, check if we can use MANAGE_EXTERNAL_STORAGE
            // This is primarily for file managers, but desktop environments qualify
            true
        } else {
            // MANAGE_EXTERNAL_STORAGE doesn't exist before API 30
            false
        }
    }

    /// Get Android 15+ compatible storage paths using Java APIs
    fn get_android_15_storage_paths_with_env(&self, env: &mut jni::JNIEnv) -> Result<Vec<(&'static str, String)>, Box<dyn std::error::Error>> {
        let activity = unsafe { JObject::from_raw(self.android_app.activity_as_ptr() as *mut _) };
        
        let mut storage_paths = Vec::new();
        
        // First try to get app-specific external storage directory (always accessible)
        match env.call_method(&activity, "getExternalFilesDir", "(Ljava/lang/String;)Ljava/io/File;", &[JValue::Object(&JObject::null())]) {
            Ok(result) => {
                if let Ok(file_obj) = result.l() {
                    if !file_obj.is_null() {
                        let path_result = env.call_method(&file_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])?;
                        let path_jstring = JString::from(path_result.l()?);
                        let app_external_path = env.get_string(&path_jstring)?.to_string_lossy().to_string();
                        
                        // Extract the base external storage path (remove /Android/data/package/files)
                        if let Some(base_path) = app_external_path.split("/Android/").next() {
                            log::info!("Android 15 detected base external storage: {}", base_path);
                            
                            // Build paths based on the detected external storage location
                            storage_paths.push(("shared", base_path.to_string()));
                            storage_paths.push(("downloads", format!("{}/Download", base_path)));
                            storage_paths.push(("dcim", format!("{}/DCIM", base_path)));
                            storage_paths.push(("pictures", format!("{}/Pictures", base_path)));
                            storage_paths.push(("music", format!("{}/Music", base_path)));
                            storage_paths.push(("movies", format!("{}/Movies", base_path)));
                            storage_paths.push(("documents", format!("{}/Documents", base_path)));
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to get external files directory: {}", e);
            }
        }
        
        // If the above didn't work, try Environment.getExternalStorageDirectory()
        if storage_paths.is_empty() {
            match env.find_class("android/os/Environment") {
                Ok(environment_class) => {
                    match env.call_static_method(
                        environment_class,
                        "getExternalStorageDirectory",
                        "()Ljava/io/File;",
                        &[],
                    ) {
                        Ok(result) => {
                            let file_obj = result.l()?;
                            let path_result = env.call_method(
                                &file_obj,
                                "getAbsolutePath",
                                "()Ljava/lang/String;",
                                &[],
                            )?;
                            let path_jstring = JString::from(path_result.l()?);
                            let external_storage_path = env.get_string(&path_jstring)?.to_string_lossy().to_string();
                            
                            log::info!("Android 15 external storage path (fallback): {}", external_storage_path);
                            
                            // Build paths based on the actual external storage location
                            storage_paths.push(("shared", external_storage_path.clone()));
                            storage_paths.push(("downloads", format!("{}/Download", external_storage_path)));
                            storage_paths.push(("dcim", format!("{}/DCIM", external_storage_path)));
                            storage_paths.push(("pictures", format!("{}/Pictures", external_storage_path)));
                            storage_paths.push(("music", format!("{}/Music", external_storage_path)));
                            storage_paths.push(("movies", format!("{}/Movies", external_storage_path)));
                            storage_paths.push(("documents", format!("{}/Documents", external_storage_path)));
                        }
                        Err(e) => {
                            log::warn!("Failed to get external storage directory via Environment: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to find Environment class: {}", e);
                }
            }
        }
        
        // Final fallback to hardcoded paths if all else fails
        if storage_paths.is_empty() {
            log::warn!("All Android 15 storage detection methods failed, using fallback paths");
            storage_paths = vec![
                ("shared", "/storage/emulated/0".to_string()),
                ("downloads", "/storage/emulated/0/Download".to_string()),
                ("dcim", "/storage/emulated/0/DCIM".to_string()),
                ("pictures", "/storage/emulated/0/Pictures".to_string()),
            ];
        }
        
        Ok(storage_paths)
    }
    

    /// Get a detailed status of storage permissions for debugging
    pub fn get_permission_status(&self) -> String {
        let vm = unsafe {
            JavaVM::from_raw(self.android_app.vm_as_ptr() as *mut _)
                .pb_expect("Failed to get JavaVM")
        };
        let mut env = vm
            .attach_current_thread()
            .pb_expect("Failed to attach current thread");

        let activity = unsafe { JObject::from_raw(self.android_app.activity_as_ptr() as *mut _) };
        let sdk_int = self.get_sdk_version(&mut env);
        
        let mut status = format!("Android API Level: {}\n", sdk_int);
        
        if sdk_int >= 30 {
            let manage_storage = self.check_manage_external_storage(&mut env, &activity);
            status.push_str(&format!("MANAGE_EXTERNAL_STORAGE: {}\n", manage_storage));
        }
        
        if sdk_int >= 33 {
            // Android 13+ granular permissions
            let media_images = self.check_single_permission(&mut env, &activity, "android.permission.READ_MEDIA_IMAGES");
            let media_video = self.check_single_permission(&mut env, &activity, "android.permission.READ_MEDIA_VIDEO");
            let media_audio = self.check_single_permission(&mut env, &activity, "android.permission.READ_MEDIA_AUDIO");
            
            status.push_str(&format!("READ_MEDIA_IMAGES: {}\n", media_images));
            status.push_str(&format!("READ_MEDIA_VIDEO: {}\n", media_video));
            status.push_str(&format!("READ_MEDIA_AUDIO: {}\n", media_audio));
        }
        
        if sdk_int >= 16 {
            // Legacy storage permissions
            let read_storage = self.check_single_permission(&mut env, &activity, "android.permission.READ_EXTERNAL_STORAGE");
            let write_storage = self.check_single_permission(&mut env, &activity, "android.permission.WRITE_EXTERNAL_STORAGE");
            
            status.push_str(&format!("READ_EXTERNAL_STORAGE: {}\n", read_storage));
            status.push_str(&format!("WRITE_EXTERNAL_STORAGE: {}\n", write_storage));
        }
        
        status.push_str(&format!("Overall storage access: {}\n", self.has_storage_permission()));
        status
    }
}

/// Callback for handling permission request results (to be called from Java)
#[no_mangle]
pub extern "C" fn handle_permission_result(
    mut env: JNIEnv,
    _class: JObject,
    request_code: jint,
    permissions: JObject,
    grant_results: JObject,
) {
    if request_code != 1001 {
        return;
    }

    log::info!("Processing permission request result with code: {}", request_code);
    
    // Convert permissions JObject to JObjectArray
    let permissions_array = unsafe { jni::objects::JObjectArray::from_raw(permissions.as_raw()) };
    
    // Check grant results array
    let grant_results_array = unsafe { jni::objects::JIntArray::from_raw(grant_results.as_raw()) };
    match env.get_array_length(&grant_results_array) {
        Ok(length) => {
            let mut granted_count = 0;
            let mut total_count = 0;
            
            // Get the raw array data
            let mut results_vec = vec![0i32; length as usize];
            env.get_int_array_region(&grant_results_array, 0, &mut results_vec)
                .pb_expect("Failed to get grant results array region");
                
            for (i, &result) in results_vec.iter().enumerate() {
                total_count += 1;
                if result == 0 { // PackageManager.PERMISSION_GRANTED
                    granted_count += 1;
                }
                
                // Log individual permission results
                if let Ok(perm_array) = env.get_object_array_element(&permissions_array, i as jint) {
                    if let Ok(perm_string) = env.get_string(&perm_array.into()) {
                        let permission_name = perm_string.to_string_lossy();
                        let status = if result == 0 { "GRANTED" } else { "DENIED" };
                        log::info!("Permission {} was {}", permission_name, status);
                    }
                }
            }
            
            log::info!("Permission results: {}/{} permissions granted", granted_count, total_count);
            
            // Update global permission state if we have some permissions
            if granted_count > 0 {
                if let Some(permission_state) = PERMISSION_GRANTED.get() {
                    if let Ok(mut granted) = permission_state.lock() {
                        *granted = true;
                        log::info!("Updated global permission state to granted");
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Failed to process permission results: {}", e);
        }
    }
}