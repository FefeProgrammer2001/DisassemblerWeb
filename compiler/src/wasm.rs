use crate::compile;
use crate::stepper::Stepper;

#[no_mangle]
pub extern "C" fn wasm_alloc(size: usize) -> *mut u8 {
    let mut buf = Vec::with_capacity(size);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn wasm_free(ptr: *mut u8, size: usize) {
    if !ptr.is_null() && size > 0 {
        unsafe {
            let _ = Vec::from_raw_parts(ptr, 0, size);
        }
    }
}

/// Compiles C source code.
/// Returns pointer to buffer formatted as: [4 bytes little-endian length] + [UTF-8 JSON string]
#[no_mangle]
pub unsafe extern "C" fn compile_c(source_ptr: *const u8, source_len: usize) -> *mut u8 {
    if source_ptr.is_null() || source_len == 0 {
        let err_json = r#"{"success":false,"assembly":"","lines":[],"functions":[],"globals":[],"errors":[{"message":"Empty source code","line":1,"column":1}]}"#;
        return unsafe { package_json_response(err_json) };
    }

    let slice = unsafe { std::slice::from_raw_parts(source_ptr, source_len) };
    let source = match std::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => {
            let err_json = r#"{"success":false,"assembly":"","lines":[],"functions":[],"globals":[],"errors":[{"message":"Invalid UTF-8 source","line":1,"column":1}]}"#;
            return unsafe { package_json_response(err_json) };
        }
    };

    let output = compile(source);
    let json_string = match serde_json::to_string(&output) {
        Ok(s) => s,
        Err(e) => format!(
            r#"{{"success":false,"assembly":"","lines":[],"functions":[],"globals":[],"errors":[{{"message":"JSON serialization error: {}","line":1,"column":1}}]}}"#,
            e
        ),
    };

    unsafe { package_json_response(&json_string) }
}

/// Creates a new Stepper virtual machine instance from C source code.
/// Returns a raw pointer to Stepper, or NULL on failure.
#[no_mangle]
pub unsafe extern "C" fn stepper_create(source_ptr: *const u8, source_len: usize) -> *mut Stepper {
    if source_ptr.is_null() || source_len == 0 {
        return std::ptr::null_mut();
    }
    let slice = unsafe { std::slice::from_raw_parts(source_ptr, source_len) };
    let source = match std::str::from_utf8(slice) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    match Stepper::new(source) {
        Ok(stepper) => Box::into_raw(Box::new(stepper)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Executes one instruction in the Stepper.
/// Returns pointer to buffer formatted as: [4 bytes little-endian length] + [UTF-8 JSON StepResult]
#[no_mangle]
pub unsafe extern "C" fn stepper_step(stepper_ptr: *mut Stepper) -> *mut u8 {
    if stepper_ptr.is_null() {
        return unsafe { package_json_response(r#"{"error":"Null stepper pointer"}"#) };
    }
    let stepper = unsafe { &mut *stepper_ptr };
    match stepper.step() {
        Ok(res) => {
            let json = serde_json::to_string(&res).unwrap_or_else(|e| format!(r#"{{"error":"{}"}}"#, e));
            unsafe { package_json_response(&json) }
        }
        Err(e) => {
            let json = format!(r#"{{"error":"{}"}}"#, e);
            unsafe { package_json_response(&json) }
        }
    }
}

/// Steps back one instruction in the Stepper.
/// Returns 1 on success, 0 if at beginning of program.
#[no_mangle]
pub unsafe extern "C" fn stepper_step_back(stepper_ptr: *mut Stepper) -> u32 {
    if stepper_ptr.is_null() {
        return 0;
    }
    let stepper = unsafe { &mut *stepper_ptr };
    match stepper.step_back() {
        Ok(true) => 1,
        _ => 0,
    }
}

/// Deallocates the Stepper virtual machine instance.
#[no_mangle]
pub unsafe extern "C" fn stepper_free(stepper_ptr: *mut Stepper) {
    if !stepper_ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(stepper_ptr);
        }
    }
}

unsafe fn package_json_response(json: &str) -> *mut u8 {
    let bytes = json.as_bytes();
    let total_len = 4 + bytes.len();
    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
    let ptr = out.as_mut_ptr();
    std::mem::forget(out);
    ptr
}
