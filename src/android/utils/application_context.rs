use crate::core::{
    config::{parse_config, LocalConfig, CONFIG_FILE, ROOT_FS_ROOT},
    logging::PolarBearExpectation,
};
use jni::{
    objects::{JObject, JString},
    JNIEnv, JavaVM,
};
use std::path::PathBuf;
use std::sync::RwLock;
use winit::platform::android::activity::AndroidApp;

#[derive(Debug, Clone)]
pub struct ApplicationContext {
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub native_library_dir: PathBuf,
    pub local_config: LocalConfig,
}

impl ApplicationContext {
    pub fn build(android_app: &AndroidApp) {
        let vm = unsafe {
            JavaVM::from_raw(android_app.vm_as_ptr() as *mut _).pb_expect("Failed to get JavaVM")
        };
        let mut env = vm
            .attach_current_thread()
            .pb_expect("Failed to attach current thread");

        let activity = unsafe { JObject::from_raw(android_app.activity_as_ptr() as *mut _) };

        let cache_dir = Self::get_path(&mut env, &activity, "getCacheDir");
        let data_dir = Self::get_path(&mut env, &activity, "getFilesDir");
        let native_library_dir = Self::get_native_library_dir(&mut env, &activity);
        let full_config_path = format!("{}{}", ROOT_FS_ROOT, CONFIG_FILE);
        let local_config = parse_config(full_config_path);

        {
            let mut context = APPLICATION_CONTEXT
                .write()
                .pb_expect("Failed to write application context");
            *context = Some(ApplicationContext {
                cache_dir,
                data_dir,
                native_library_dir,
                local_config,
            });
            log::info!(
                "ApplicationContext initialized: {:?}",
                context.as_ref().unwrap()
            );
        }
    }

    fn get_path(env: &mut JNIEnv, activity: &JObject, method: &str) -> PathBuf {
        let path_obj = env
            .call_method(activity, method, "()Ljava/io/File;", &[])
            .pb_expect("Failed to call method")
            .l()
            .pb_expect("Failed to get path object");
        let path_str = env
            .call_method(path_obj, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .pb_expect("Failed to get absolute path")
            .l()
            .pb_expect("Failed to get path string");
        let path: String = env
            .get_string(&JString::from(path_str))
            .pb_expect("Failed to convert path to string")
            .into();
        PathBuf::from(path)
    }

    fn get_native_library_dir(env: &mut JNIEnv, activity: &JObject) -> PathBuf {
        let app_info = env
            .call_method(
                activity,
                "getApplicationInfo",
                "()Landroid/content/pm/ApplicationInfo;",
                &[],
            )
            .pb_expect("Failed to get application info")
            .l()
            .pb_expect("Failed to get application info object");
        let native_library_dir = env
            .get_field(app_info, "nativeLibraryDir", "Ljava/lang/String;")
            .pb_expect("Failed to get native library dir field")
            .l()
            .pb_expect("Failed to get native library dir object");
        let path: String = env
            .get_string(&JString::from(native_library_dir))
            .pb_expect("Failed to convert native library dir to string")
            .into();
        PathBuf::from(path)
    }
}

static APPLICATION_CONTEXT: RwLock<Option<ApplicationContext>> = RwLock::new(None);
pub fn get_application_context() -> ApplicationContext {
    return APPLICATION_CONTEXT
        .read()
        .pb_expect("Failed to read application context")
        .clone()
        .pb_expect("ApplicationContext is not initialized. Please make sure `ApplicationContext::build(&android_app);` is called in `android_main`.");
}
