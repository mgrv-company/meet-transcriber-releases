//! 이 프로세스에서 whisper.cpp가 쓸 수 있는 GPU. 빌드에 GPU 지원(`metal`, `cuda`, `vulkan`)이 들었는지와
//! 실행 환경에 GPU가 실제로 있는지는 별개라(CUDA 빌드를 GPU 없는 곳에서 띄우면 조용히 CPU로 돈다) 따로 알려 준다.

use std::ffi::CStr;
use whisper_rs::whisper_rs_sys as sys;

/// 이 빌드에 GPU 지원이 들어 있는가. 없으면 GPU가 있어도 CPU로만 전사한다.
pub const GPU_BUILD: bool = cfg!(any(feature = "metal", feature = "cuda", feature = "vulkan"));

/// ggml이 찾은 GPU의 이름들. 빌드에 GPU 지원이 없거나 장치가 없으면 비어 있다.
pub fn devices() -> Vec<String> {
    // SAFETY: 백엔드 목록은 ggml이 처음 부를 때 만들어 두고 이후 읽기만 한다. 설명 문자열은 ggml이 소유한다.
    (0..unsafe { sys::ggml_backend_dev_count() })
        .map(|i| unsafe { sys::ggml_backend_dev_get(i) })
        .filter(|&dev| {
            let kind = unsafe { sys::ggml_backend_dev_type(dev) };
            kind == sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU
                || kind == sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU
        })
        .map(|dev| {
            unsafe { CStr::from_ptr(sys::ggml_backend_dev_description(dev)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}
