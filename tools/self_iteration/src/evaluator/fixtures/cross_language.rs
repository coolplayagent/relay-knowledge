const CROSS_LANGUAGE_BRIDGE_H: &str = r#"#pragma once

#ifdef __cplusplus
extern "C" {
#endif

int rk_c_decode(const char *payload);
int rk_cpp_score(const char *payload);
int rk_c_entry_process(const char *payload);

#ifdef __cplusplus
}
#endif
"#;

const CROSS_LANGUAGE_C_ENTRY: &str = r#"#include "rk_bridge.h"

static int rk_c_weight(char value)
{
    return (int)value;
}

int rk_c_decode(const char *payload)
{
    if (payload == 0 || payload[0] == '\0') {
        return 0;
    }
    return rk_c_weight(payload[0]);
}

int rk_c_entry_process(const char *payload)
{
    int native = rk_c_decode(payload);
    int bridged = rk_cpp_score(payload);
    return native + bridged;
}
"#;

const CROSS_LANGUAGE_CPP_BRIDGE: &str = r#"#include "rk_bridge.h"

#include <string_view>

namespace rk::bridge {

class BridgeHelper {
 public:
    int Normalize(const char *payload) const
    {
        std::string_view view(payload == nullptr ? "" : payload);
        return static_cast<int>(view.size());
    }
};

}  // namespace rk::bridge

extern "C" int rk_cpp_score(const char *payload)
{
    rk::bridge::BridgeHelper helper;
    return helper.Normalize(payload) + rk_c_decode(payload);
}
"#;

const CROSS_LANGUAGE_GO_BRIDGE: &str = r#"package bridge

/*
#cgo CFLAGS: -I../include
#include <stdlib.h>
#include "rk_bridge.h"
*/
import "C"

import "unsafe"

func RunCgoBridge(payload string) int {
    cPayload := C.CString(payload)
    defer C.free(unsafe.Pointer(cPayload))
    return int(C.rk_c_decode(cPayload))
}
"#;

const CROSS_LANGUAGE_RUST_BRIDGE: &str = r#"use std::ffi::CString;
use std::os::raw::{c_char, c_int};

unsafe extern "C" {
    fn rk_c_decode(payload: *const c_char) -> c_int;
}

pub fn run_rust_bridge(payload: &str) -> i32 {
    let c_payload = CString::new(payload).expect("fixture payload should not contain nul bytes");
    unsafe { rk_c_decode(c_payload.as_ptr()) as i32 }
}
"#;

const CROSS_LANGUAGE_FAKE_BRIDGE: &str = r#"#include "rk_bridge.h"

int rk_cpp_score_fake(const char *payload)
{
    (void)payload;
    return 0;
}
"#;

const PROJECT_ALIAS_LIB_RS: &str = r#"
pub fn stable_project_entry() -> &'static str {
    "project-name-default-alias"
}

pub fn stable_project_session_entry() -> &'static str {
    stable_project_entry()
}
"#;
